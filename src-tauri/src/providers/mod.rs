pub mod anthropic;
pub mod balance;
pub mod openai;
pub mod openrouter;
pub mod registry;
pub mod script_engine;
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
    /// 查询链路：从 registry 取 template.queries，过滤出 is_usage_query() 的，
    /// 按 QueryType 分发到 balance / coding_plan / script_engine，
    /// 依次尝试，鉴权错立即返回，其它错继续尝试下一条。
    pub async fn fetch_api_usage(
        &self,
        provider_id: &str,
        api_key: &str,
        custom_config: Option<&CustomProviderConfig>,
    ) -> Result<(UsageData, Option<RateLimitData>), String> {
        let template = self.resolve_template_for_query(provider_id, custom_config)?;

        let mut last_error: Option<ProviderError> = None;
        for spec in template.queries.iter().filter(|q| q.is_usage_query()) {
            match self.execute_usage_query(spec, api_key, custom_config).await {
                Ok(usage) => {
                    // 旧版 provider 提供 rate_limit 查询（阶段 1 临时方案）
                    let rate_limit = if let Some(legacy) = self.legacy_providers.get(provider_id) {
                        legacy.fetch_rate_limits(api_key).await.ok().flatten()
                    } else {
                        None
                    };
                    return Ok((usage, rate_limit));
                }
                Err(ProviderError::AuthError(_)) => {
                    return Err(ProviderError::AuthError(
                        last_error.map(|e| e.to_string()).unwrap_or_default(),
                    )
                    .to_string());
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
    pub async fn fetch_subscription_usage(
        &self,
        provider_id: &str,
        oauth_token: &str,
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
                return self.subscription_fetcher.fetch(provider, oauth_token).await;
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
            QueryType::CodingPlan { provider: _ } => {
                // 阶段 2 实现
                Err(ProviderError::RequestError(
                    "CodingPlan 查询将在阶段 2 实现".into(),
                ))
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
                script_engine::run(
                    &self.http_client,
                    code,
                    api_key,
                    base_url,
                    allow_http,
                    timeout_ms,
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
        // 新版：尝试执行一次用量查询，成功即有效
        match self
            .fetch_api_usage(provider_id, api_key, custom_config)
            .await
        {
            Ok(_) => Ok(true),
            Err(e) if e.contains("认证失败") || e.contains("AuthError") => Ok(false),
            Err(e) if e.contains("401") || e.contains("403") => Ok(false),
            Err(_) => Ok(false),
        }
    }
}

impl Default for ProviderManager {
    fn default() -> Self {
        Self::new()
    }
}

/// 从自定义供应商配置构造一个临时 ProviderTemplate
fn template_from_custom(provider_id: &str, cfg: &CustomProviderConfig) -> ProviderTemplate {
    let auth_scheme = match cfg.auth_scheme {
        AuthSchemeConfig::Bearer => AuthScheme::Bearer,
        AuthSchemeConfig::XApiKey => AuthScheme::XApiKey,
        AuthSchemeConfig::RawKey => AuthScheme::RawKey,
    };

    let query_spec = match cfg.query_type {
        QueryTypeConfig::Balance => {
            // 自定义 Balance 查询：URL 由 base_url + 用户脚本里的 field_map 决定
            // 阶段 1 自定义供应商若选 Balance，必须提供脚本（简化：Balance 也走 script_engine）
            QuerySpec {
                query_type: QueryType::Script {
                    default_template: cfg.script.as_ref().map(|s| s.code.clone()),
                },
                base_url: Some(cfg.base_url.clone()),
            }
        }
        QueryTypeConfig::Script => QuerySpec {
            query_type: QueryType::Script {
                default_template: cfg.script.as_ref().map(|s| s.code.clone()),
            },
            base_url: Some(cfg.base_url.clone()),
        },
    };

    let _ = auth_scheme; // 自定义 Balance 暂走 script，auth_scheme 在 script 里处理

    ProviderTemplate {
        id: provider_id.to_string(),
        display_name: cfg.display_name.clone(),
        env_key_name: cfg.env_key_name.clone().unwrap_or_default(),
        env_oauth_token_name: None,
        queries: vec![query_spec],
        capabilities: ProviderCapabilities {
            has_balance: matches!(cfg.query_type, QueryTypeConfig::Balance),
            has_usage: true,
            has_rate_limit: false,
            has_subscription: false,
        },
        icon: cfg.icon.clone().unwrap_or_else(|| "custom".to_string()),
        docs_url: None,
        oauth_detect: None,
    }
}
