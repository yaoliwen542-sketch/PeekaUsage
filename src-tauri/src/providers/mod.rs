pub mod anthropic;
pub mod balance;
pub mod coding_plan;
pub mod oauth_detect;
pub mod openai;
pub mod openrouter;
pub mod registry;
pub mod script_engine;
pub mod sigv4;
pub mod subscription;
pub mod traits;
pub mod types;

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use traits::{ProviderError, UsageProvider};
use types::*;

/// 供应商管理器：通过 ProviderRegistry 路由用量/订阅查询
///
/// 架构说明：
/// - 内置供应商的查询规格由 ProviderRegistry 提供（配置驱动）
/// - 旧版 openai/anthropic/openrouter 三个 UsageProvider 实现暂保留，
///   用于 fetch_rate_limits / validate_key 等还未来得及迁移的方法
/// - 新增供应商（deepseek/newapi/自定义）完全走 registry 的 QueryType 分发
pub struct ProviderManager {
    /// 旧版 provider 实例（仅 openai/anthropic/openrouter，用于 rate_limit/validate）
    legacy_providers: HashMap<String, Arc<dyn UsageProvider>>,
    /// HTTP 客户端（Balance / Script 查询共用）
    http_client: reqwest::Client,
    /// 订阅查询器
    subscription_fetcher: subscription::SubscriptionFetcher,
    /// 缓存
    cache: RwLock<HashMap<String, UsageSummary>>,
}

impl ProviderManager {
    pub fn new() -> Self {
        let mut legacy_providers: HashMap<String, Arc<dyn UsageProvider>> = HashMap::new();

        let openai = Arc::new(openai::OpenAIProvider::new());
        let anthropic = Arc::new(anthropic::AnthropicProvider::new());
        let openrouter = Arc::new(openrouter::OpenRouterProvider::new());

        legacy_providers.insert(openai.id(), openai);
        legacy_providers.insert(anthropic.id(), anthropic);
        legacy_providers.insert(openrouter.id(), openrouter);

        Self {
            legacy_providers,
            http_client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("无法创建 HTTP 客户端"),
            subscription_fetcher: subscription::SubscriptionFetcher::new(),
            cache: RwLock::new(HashMap::new()),
        }
    }

    /// 暴露 HTTP 客户端引用（供命令层调用 script_engine）
    pub fn http_client_ref(&self) -> &reqwest::Client {
        &self.http_client
    }

    /// 解析供应商模板：优先从 registry 取，找不到则返回错误
    fn resolve_template(&self, provider_id: &str) -> Result<ProviderTemplate, String> {
        registry::get(provider_id).ok_or_else(|| format!("未知供应商: {}", provider_id))
    }

    /// 获取所有已注册内置供应商的模板（用于设置页"新增供应商"下拉）
    pub fn get_provider_templates(&self) -> Vec<ProviderTemplate> {
        registry::all()
    }

    /// 获取单个内置供应商模板
    pub fn get_provider_template(&self, provider_id: &str) -> Option<ProviderTemplate> {
        registry::get(provider_id)
    }

    /// 获取所有已注册供应商的信息（兼容旧接口，返回 registry 全部 + capabilities）
    pub fn get_provider_config_items(&self) -> Vec<ProviderConfigItem> {
        let mut items: Vec<ProviderConfigItem> = registry::all()
            .iter()
            .map(|template| ProviderConfigItem {
                provider_id: template.id.clone(),
                display_name: template.display_name.clone(),
                enabled: false,
                api_keys: Vec::new(),
                subscriptions: Vec::new(),
                capabilities: template.capabilities.clone(),
                environment_variable_name: template.env_key_name.clone(),
                active_api_key_id: None,
                provider_template_id: Some(template.id.clone()),
                custom_config: None,
            })
            .collect();

        items.sort_by(|left, right| left.provider_id.cmp(&right.provider_id));
        items
    }

    pub fn get_provider_config_item(&self, provider_id: &str) -> Option<ProviderConfigItem> {
        registry::get(provider_id).map(|template| ProviderConfigItem {
            provider_id: template.id.clone(),
            display_name: template.display_name.clone(),
            enabled: false,
            api_keys: Vec::new(),
            subscriptions: Vec::new(),
            capabilities: template.capabilities.clone(),
            environment_variable_name: template.env_key_name.clone(),
            active_api_key_id: None,
            provider_template_id: Some(template.id.clone()),
            custom_config: None,
        })
    }

    /// 获取单个供应商的按量 API 数据
    ///
    /// 查询路由（阶段 1 修复）：
    /// 1. 优先查 legacy_providers：openai / anthropic / openrouter 三家按量查询走旧版
    ///    已验证可用的 fetch_usage（避免 registry JSONPath 模板错误），同时附带 rate_limit
    /// 2. 不在 legacy_providers 里的（deepseek / newapi 预设 / 自定义）走 registry 模板分发：
    ///    从 template.queries 过滤出 is_usage_query() 的，按 QueryType 分发到
    ///    balance / coding_plan / script_engine，依次尝试
    pub async fn fetch_api_usage(
        &self,
        provider_id: &str,
        api_key: &str,
        custom_config: Option<&CustomProviderConfig>,
    ) -> Result<(UsageData, Option<RateLimitData>), String> {
        // 优先走 legacy provider（openai/anthropic/openrouter，已验证的旧实现）
        // 修复 C-1/C-2：registry 不再为这三家配置 Balance 模板，按量查询直接复用 legacy fetch_usage
        if let Some(legacy) = self.legacy_providers.get(provider_id) {
            let usage = legacy
                .fetch_usage(api_key)
                .await
                .map_err(|e| e.to_string())?;
            let rate_limit = legacy.fetch_rate_limits(api_key).await.ok().flatten();
            return Ok((usage, rate_limit));
        }

        // 新增供应商（deepseek/newapi/custom）走 registry 模板分发
        let template = self.resolve_template_for_query(provider_id, custom_config)?;

        let mut last_error: Option<ProviderError> = None;
        for spec in template.queries.iter().filter(|q| q.is_usage_query()) {
            match self.execute_usage_query(spec, api_key, custom_config).await {
                Ok(usage) => {
                    // 新版供应商暂无 rate_limit 查询能力（阶段 2 扩展）
                    return Ok((usage, None));
                }
                // 修复 I-1：AuthError 应携带当前触发的错误 e（而非累积的 last_error），
                // 否则会把之前查询路径的错误信息误当作鉴权失败原因返回。
                Err(ProviderError::AuthError(e)) => {
                    return Err(ProviderError::AuthError(e).to_string());
                }
                Err(e) => {
                    last_error = Some(e);
                    continue;
                }
            }
        }

        Err(last_error
            .map(|e| e.to_string())
            .unwrap_or_else(|| "所有查询路径都失败".to_string()))
    }

    /// 获取单个供应商的订阅数据
    ///
    /// `account_id` 仅 openai_wham 使用：透传给 SubscriptionFetcher::fetch，
    /// 作为 `ChatGPT-Account-Id` header。传 None 时由 fetcher 内部自动检测
    /// （从 `~/.codex/auth.json` 的 `tokens.account_id` 读取）。
    pub async fn fetch_subscription_usage(
        &self,
        provider_id: &str,
        oauth_token: &str,
        custom_config: Option<&CustomProviderConfig>,
    ) -> SubscriptionUsage {
        self.fetch_subscription_usage_with_account(provider_id, oauth_token, None, custom_config)
            .await
    }

    /// 获取单个供应商的订阅数据（显式传入 account_id）
    pub async fn fetch_subscription_usage_with_account(
        &self,
        provider_id: &str,
        oauth_token: &str,
        account_id: Option<&str>,
        custom_config: Option<&CustomProviderConfig>,
    ) -> SubscriptionUsage {
        let template = match self.resolve_template_for_query(provider_id, custom_config) {
            Ok(t) => t,
            Err(_) => {
                return SubscriptionUsage {
                    plan_name: None,
                    windows: vec![],
                    extra_usage: None,
                    status: ProviderStatus::Error,
                    error_message: Some("当前供应商不支持订阅查询".into()),
                }
            }
        };

        for spec in template
            .queries
            .iter()
            .filter(|q| q.is_subscription_query())
        {
            if let QueryType::Subscription { provider } = &spec.query_type {
                return self
                    .subscription_fetcher
                    .fetch(provider, oauth_token, account_id)
                    .await;
            }
        }

        SubscriptionUsage {
            plan_name: None,
            windows: vec![],
            extra_usage: None,
            status: ProviderStatus::Error,
            error_message: Some("当前供应商不支持订阅查询".into()),
        }
    }

    /// 解析查询用模板：内置供应商从 registry 取，自定义供应商从 custom_config 构造
    fn resolve_template_for_query(
        &self,
        provider_id: &str,
        custom_config: Option<&CustomProviderConfig>,
    ) -> Result<ProviderTemplate, String> {
        if let Some(cfg) = custom_config {
            return Ok(template_from_custom(provider_id, cfg));
        }
        self.resolve_template(provider_id)
    }

    /// 执行单条用量查询
    async fn execute_usage_query(
        &self,
        spec: &QuerySpec,
        api_key: &str,
        custom_config: Option<&CustomProviderConfig>,
    ) -> Result<UsageData, ProviderError> {
        match &spec.query_type {
            QueryType::Balance {
                url,
                auth,
                field_map,
            } => {
                balance::execute_balance_query(&self.http_client, url, auth, field_map, api_key)
                    .await
            }
            QueryType::CodingPlan { provider } => {
                coding_plan::execute_coding_plan_query(&self.http_client, provider, api_key).await
            }
            QueryType::Subscription { .. } => {
                Err(ProviderError::RequestError("订阅查询不应走用量链路".into()))
            }
            QueryType::Script { default_template } => {
                let code = custom_config
                    .and_then(|c| c.script.as_ref())
                    .map(|s| s.code.as_str())
                    .or(default_template.as_deref())
                    .ok_or_else(|| ProviderError::RequestError("未提供脚本代码".into()))?;
                let base_url = custom_config.map(|c| c.base_url.as_str());
                let allow_http = custom_config.map(|c| c.allow_http).unwrap_or(false);
                let timeout_ms = custom_config
                    .and_then(|c| c.script.as_ref())
                    .map(|s| s.timeout_ms)
                    .unwrap_or(15000);
                // 修复 C-3：从 custom_config 读取 accessToken / userId（阶段 1 临时方案：
                // 明文存储于 custom_config，阶段 2 迁移到 KeyStore）
                let access_token = custom_config.and_then(|c| c.access_token.as_deref());
                let user_id = custom_config.and_then(|c| c.user_id.as_deref());
                script_engine::run(
                    &self.http_client,
                    code,
                    api_key,
                    base_url,
                    allow_http,
                    timeout_ms,
                    access_token,
                    user_id,
                )
                .await
            }
        }
    }

    /// 缓存汇总结果
    pub async fn cache_summary(&self, provider_id: &str, summary: UsageSummary) {
        let mut cache = self.cache.write().await;
        cache.insert(provider_id.to_string(), summary);
    }

    /// 验证 Key
    ///
    /// 旧版 provider 走 trait，新版（deepseek/newapi/自定义）走 registry 的 validate 逻辑。
    pub async fn validate_key(
        &self,
        provider_id: &str,
        api_key: &str,
        custom_config: Option<&CustomProviderConfig>,
    ) -> Result<bool, String> {
        // 旧版 provider 优先
        if let Some(legacy) = self.legacy_providers.get(provider_id) {
            return legacy
                .validate_key(api_key)
                .await
                .map_err(|e| e.to_string());
        }
        // 新版：尝试执行一次用量查询，成功即有效。
        //
        // 只有鉴权类错误（AuthError / 401 / 403）才判定为 "key 无效"，返回 Ok(false)；
        // 瞬时错误（网络 RequestError / 限流 RateLimited）以及其它错误（ParseError 等）
        // 一律向上抛 Err，让前端展示 "无法验证（网络错误）" 而不是误判 key 无效。
        match self
            .fetch_api_usage(provider_id, api_key, custom_config)
            .await
        {
            Ok(_) => Ok(true),
            Err(e) if is_auth_error_message(&e) => Ok(false),
            Err(e) => Err(format!("无法验证 API Key: {}", e)),
        }
    }
}

impl Default for ProviderManager {
    fn default() -> Self {
        Self::new()
    }
}

/// 判断 fetch_api_usage 返回的错误字符串是否为鉴权类错误。
///
/// fetch_api_usage 返回的是 `String`（ProviderError 经 `to_string()` 转换），
/// 鉴权类错误的 Display 文案固定以 "认证失败" 开头（见 ProviderError::AuthError 的
/// `#[error("认证失败: {0}")]`）。
///
/// 修复 I-2：不再匹配裸 "401"/"403" 子串（会误判任何包含这串数字的普通错误，
/// 例如 "解析响应失败: Unexpected token at 4012"），改为精确匹配 "HTTP 401" / "HTTP 403"
/// （script_engine 与 balance 在产生鉴权错误时统一用 "HTTP {}" 格式）。
/// "AuthError" 标识保留以兼容旧版 provider 直接 Display 的场景。
fn is_auth_error_message(msg: &str) -> bool {
    msg.starts_with("认证失败")
        || msg.contains("AuthError")
        || msg.contains("HTTP 401")
        || msg.contains("HTTP 403")
}

/// 从自定义供应商配置构造一个临时 ProviderTemplate
///
/// 修复 I-3：阶段 1 自定义供应商只支持 Script 查询。
/// 原实现区分 Balance / Script 分支，但 Balance 分支实际也走 script_engine（半成品），
/// 永远无法正常工作。这里统一走 Script，避免向用户暴露一个失败的 Balance 选项。
/// `QueryTypeConfig::Balance` 枚举值保留（阶段 2 实现真正的 Balance 查询后开放），
/// 但向导 UI 只暴露 Script（见 ProviderWizardDialog）。
fn template_from_custom(provider_id: &str, cfg: &CustomProviderConfig) -> ProviderTemplate {
    let auth_scheme = match cfg.auth_scheme {
        AuthSchemeConfig::Bearer => AuthScheme::Bearer,
        AuthSchemeConfig::XApiKey => AuthScheme::XApiKey,
        AuthSchemeConfig::RawKey => AuthScheme::RawKey,
    };

    // 自定义供应商统一走 Script 查询（Balance 阶段 1 不开放）
    let query_spec = QuerySpec {
        query_type: QueryType::Script {
            default_template: cfg.script.as_ref().map(|s| s.code.clone()),
        },
        base_url: Some(cfg.base_url.clone()),
    };

    let _ = auth_scheme; // 自定义 Script 查询的认证在脚本内部处理（headers 由脚本生成）

    ProviderTemplate {
        id: provider_id.to_string(),
        display_name: cfg.display_name.clone(),
        env_key_name: cfg.env_key_name.clone().unwrap_or_default(),
        env_oauth_token_name: None,
        queries: vec![query_spec],
        capabilities: ProviderCapabilities {
            has_balance: false,
            has_usage: true,
            has_rate_limit: false,
            has_subscription: false,
        },
        icon: cfg.icon.clone().unwrap_or_else(|| "custom".to_string()),
        docs_url: None,
        oauth_detect: None,
    }
}
