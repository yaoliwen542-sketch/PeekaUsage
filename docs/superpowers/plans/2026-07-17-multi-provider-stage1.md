# 多供应商配置驱动架构改造（阶段 1）实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 PeekaUsage 从"枚举 + 硬编码 match"的封闭架构改造为"配置驱动 provider registry + 查询模板 + JS 脚本兜底"的开放架构，迁移现有 3 家供应商，新增 DeepSeek 和 NewAPI，支持自定义供应商。

**Architecture:** `ProviderId` 从枚举改为 `String`；新增 `registry.rs` 存储内置供应商模板（`ProviderTemplate` + `QueryType` 枚举分发 Balance/CodingPlan/Subscription/Script 四类查询）；`ProviderManager` 改为查 registry 路由，删除所有 `match provider_id`；新增 `script_engine.rs`（rquickjs 沙箱）执行用户自定义 JS 脚本；前端 `ProviderId` 同步改 `string`，设置页新增自定义供应商向导。

**Tech Stack:** Rust（rquickjs、jsonpath-rust、reqwest、serde、async-trait）、TypeScript（React）、Tauri v2

## Global Constraints

- 语言规范：所有对话回复、代码注释、错误提示、新增文档必须使用中文（AGENTS.md 语言规范）
- 类型同步：`src-tauri/src/providers/types.rs` 和 `src/types/provider.ts` 必须同步修改，Rust snake_case，TS camelCase，通过 serde 映射
- 跨平台：必须兼容 Windows / Linux / macOS，不依赖单平台特有 API
- 向后兼容：旧 `config.json` 缺少新字段时必须兼容，所有新字段用 `#[serde(default)]`
- 图标约束：统一通过 `ProviderIcon.tsx` 渲染，图标资源放 `src/assets/provider-icons/`
- 下拉组件约束：核心交互下拉复用 `AppSelect.tsx`，不用原生 `<select>`
- 提交约束：不要把"代码改了但文档没更新"的状态提交；每次提交前至少跑 `npx tsc --noEmit` + `cargo fmt --all --check` + `cargo check`
- rquickjs 依赖：约 +2MB 编译产物，跨平台有预编译
- jsonpath-rust 依赖：约 +50KB 编译产物

## File Structure

### Rust 新建文件
- `src-tauri/src/providers/registry.rs` — Provider Registry，内置供应商模板表 + 查询分发
- `src-tauri/src/providers/balance.rs` — Balance 查询通用实现（bytes-then-parse + jsonpath 提取）
- `src-tauri/src/providers/script_engine.rs` — rquickjs 沙箱 + JS 脚本执行 + 安全边界

### Rust 改造文件
- `src-tauri/Cargo.toml` — 加 rquickjs、jsonpath-rust 依赖
- `src-tauri/src/providers/types.rs` — ProviderId 改 String，新增 QueryType/AuthScheme 等类型，ProviderError 加 is_transient
- `src-tauri/src/providers/traits.rs` — UsageProvider trait 适配 String ProviderId
- `src-tauri/src/providers/mod.rs` — ProviderManager 改为查 registry 路由
- `src-tauri/src/providers/openai.rs` — 适配新架构
- `src-tauri/src/providers/anthropic.rs` — 适配新架构
- `src-tauri/src/providers/openrouter.rs` — 适配新架构
- `src-tauri/src/providers/subscription.rs` — 函数重命名，由 registry 调度
- `src-tauri/src/commands/provider_commands.rs` — 删 parse_provider_id，改 save_provider_order，新增命令，支持 customConfig
- `src-tauri/src/config/app_config.rs` — ProviderEntry 加字段，改 is_supported_provider_id
- `src-tauri/src/config/system_env.rs` — 改 supported_provider_ids，支持 customConfig
- `src-tauri/src/lib.rs` — 注册新命令

### 前端新建文件
- `src/components/settings/ProviderWizardDialog.tsx` — 自定义供应商配置向导（3 步）

### 前端改造文件
- `src/types/provider.ts` — ProviderId 改 string，新增类型
- `src/utils/ipc.ts` — 新增 3 个 IPC
- `src/components/common/AppSelect.tsx` — 加 grouped 模式
- `src/components/common/ProviderIcon.tsx` — 支持新图标
- `src/components/settings/ProviderConfig.tsx` — 字段动态显隐，NewAPI 字段
- `src/components/settings/SettingsPanel.tsx` — 下拉接 getProviderTemplates，分组渲染
- `src/components/widget/ProviderCard.tsx` — 适配 Balance/Script 展示，window label i18n
- `src/components/widget/WidgetContainer.tsx` — 适配 string ProviderId
- `src/i18n/messages.ts` — 加 windowLabels + 新供应商名称 + 向导文案

### 资源
- `src/assets/provider-icons/deepseek.svg` — DeepSeek 图标
- `src/assets/provider-icons/newapi.svg` — NewAPI 图标
- `src/assets/provider-icons/custom.svg` — 自定义供应商默认图标

### 文档
- `AGENTS.md` — 加第 23 章
- `CLAUDE.md` — 同步

---

## Task 1: 添加依赖与窗口常量

**Files:**
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/src/providers/types.rs:1-41`

**Interfaces:**
- Produces: `ProviderId` 类型别名、窗口常量集（供后续所有任务使用）

- [ ] **Step 1: 在 Cargo.toml 添加依赖**

在 `src-tauri/Cargo.toml` 的 `[dependencies]` 末尾追加：

```toml
rquickjs = { version = "0.7", features = ["loader", "allocator"] }
jsonpath-rust = "0.7"
```

- [ ] **Step 2: 把 ProviderId 从枚举改为类型别名，删除旧 impl 块**

在 `src-tauri/src/providers/types.rs` 中，把第 1-41 行（`ProviderId` 枚举定义 + `impl ProviderId` 块）替换为：

```rust
use serde::{Deserialize, Serialize};

/// 供应商 ID（配置驱动，不再是枚举）
///
/// 内置供应商 ID 为 "openai" / "anthropic" / "openrouter" / "deepseek" / "newapi" 等；
/// 自定义供应商 ID 形如 "custom_xxx"。
/// 对应的环境变量名、OAuth token 名等通过 ProviderRegistry 查询。
pub type ProviderId = String;

/// 订阅窗口标签常量（机器可枚举，前端通过 i18n 映射成显示文案）
pub mod window_labels {
    pub const FIVE_HOUR: &str = "five_hour";
    pub const SEVEN_DAY: &str = "seven_day";
    pub const SEVEN_DAY_SONNET: &str = "seven_day_sonnet";
    pub const SEVEN_DAY_OPUS: &str = "seven_day_opus";
    pub const WEEKLY_LIMIT: &str = "weekly_limit";
    pub const MONTHLY: &str = "monthly";
}
```

- [ ] **Step 3: 运行 cargo check 确认编译错误集中爆发（预期）**

Run: `cd src-tauri && cargo check 2>&1 | head -40`
Expected: 大量编译错误，因为 `ProviderId::OpenAI` 等枚举变体不再存在。这是预期的，后续任务会逐个修复。记录错误数量作为基线。

- [ ] **Step 4: 暂不提交（本任务是改造起点，后续任务一起提交）**

本任务不单独提交，因为代码处于不可编译状态。Task 2-6 会修复所有编译错误，到 Task 7 一起提交。

---

## Task 2: 定义新的查询模板类型

**Files:**
- Modify: `src-tauri/src/providers/types.rs`（在 Task 1 基础上追加）

**Interfaces:**
- Consumes: `ProviderId`（Task 1）
- Produces: `QueryType`、`AuthScheme`、`BalanceFieldMap`、`QuerySpec`、`ProviderTemplate`、`OAuthDetectConfig`、`CustomProviderConfig`、`ScriptConfig`、`AuthSchemeConfig`、`QueryTypeConfig`

- [ ] **Step 1: 在 types.rs 末尾追加查询模板相关类型**

在 `src-tauri/src/providers/types.rs` 末尾追加：

```rust
/// 查询类型：决定如何获取供应商用量
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum QueryType {
    /// 余额查询（货币型，如 DeepSeek、OpenRouter）
    Balance {
        url: String,
        auth: AuthScheme,
        field_map: BalanceFieldMap,
    },
    /// Coding Plan 查询（百分比型，如 Kimi、GLM、MiniMax）-- 阶段 2 实现
    CodingPlan {
        provider: String,
    },
    /// OAuth 订阅查询（如 Claude、Codex、Gemini）
    Subscription {
        provider: String,
    },
    /// JS 脚本查询（NewAPI、自定义）
    Script {
        default_template: Option<String>,
    },
}

/// 认证方案
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthScheme {
    /// Authorization: Bearer {key}
    Bearer,
    /// x-api-key: {key}
    XApiKey,
    /// 裸 key（如 GLM，Authorization: {key} 无 Bearer 前缀）
    RawKey,
    /// 自定义 header 集合
    Custom(Vec<(String, String)>),
}

/// Balance 查询的字段映射（JSONPath）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BalanceFieldMap {
    /// 总额 JSONPath，如 "$.data.total_credits"
    pub total: String,
    /// 已用 JSONPath（可选）
    pub used: Option<String>,
    /// 剩余 JSONPath（可选，若 None 则 total - used 计算）
    pub remaining: Option<String>,
    /// 货币单位，如 "USD" / "CNY"
    pub currency: String,
}

/// 单条查询规格
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuerySpec {
    pub query_type: QueryType,
    /// 覆盖默认 base url（自定义供应商用）
    #[serde(default)]
    pub base_url: Option<String>,
}

impl QuerySpec {
    /// 是否为按量 API 用量查询（Balance / CodingPlan / Script）
    pub fn is_usage_query(&self) -> bool {
        matches!(
            self.query_type,
            QueryType::Balance { .. } | QueryType::CodingPlan { .. } | QueryType::Script { .. }
        )
    }

    /// 是否为订阅查询
    pub fn is_subscription_query(&self) -> bool {
        matches!(self.query_type, QueryType::Subscription { .. })
    }
}

/// 内置供应商模板（注册表条目）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderTemplate {
    /// 供应商 ID，如 "openai"
    pub id: String,
    /// 显示名称，如 "OpenAI"
    pub display_name: String,
    /// 按量 API Key 对应的环境变量名，如 "OPENAI_API_KEY"
    pub env_key_name: String,
    /// 订阅 OAuth Token 对应的环境变量名（可选）
    #[serde(default)]
    pub env_oauth_token_name: Option<String>,
    /// 查询规格列表（一个供应商可有多条查询路径，如 OpenAI 有 Balance + Subscription）
    pub queries: Vec<QuerySpec>,
    /// 供应商能力
    pub capabilities: ProviderCapabilities,
    /// 图标名（对应 src/assets/provider-icons/ 下的文件名）
    pub icon: String,
    /// "获取方式"按钮跳转的官方文档 URL
    #[serde(default)]
    pub docs_url: Option<String>,
    /// OAuth 凭据自动检测配置（阶段 2 填充，阶段 1 预留）
    #[serde(default)]
    pub oauth_detect: Option<OAuthDetectConfig>,
}

/// OAuth 凭据自动检测配置（阶段 2 实现，阶段 1 仅占位）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthDetectConfig {
    /// 凭据文件路径（如 "~/.codex/auth.json"）
    pub file_path: String,
    /// 文件内 token 的 JSONPath
    pub token_path: String,
    /// macOS Keychain service 名（可选）
    #[serde(default)]
    pub keychain_service: Option<String>,
}

/// 自定义供应商配置（存于 config.json 的 customConfig 字段）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomProviderConfig {
    pub display_name: String,
    pub base_url: String,
    #[serde(default = "default_auth_scheme")]
    pub auth_scheme: AuthSchemeConfig,
    /// 自定义环境变量名（不填则不接管环境变量）
    #[serde(default)]
    pub env_key_name: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    pub query_type: QueryTypeConfig,
    #[serde(default)]
    pub script: Option<ScriptConfig>,
    /// 是否允许 HTTP（默认 false，强制 HTTPS）
    #[serde(default)]
    pub allow_http: bool,
}

fn default_auth_scheme() -> AuthSchemeConfig {
    AuthSchemeConfig::Bearer
}

/// 认证方案（配置层，前端可序列化）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthSchemeConfig {
    Bearer,
    XApiKey,
    RawKey,
}

/// 查询类型（配置层，前端可序列化）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryTypeConfig {
    Balance,
    Script,
}

/// JS 脚本配置
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptConfig {
    /// 脚本代码（返回 {request, extractor} 的 JS 表达式）
    pub code: String,
    #[serde(default = "default_script_language")]
    pub language: String,
    #[serde(default = "default_script_timeout_ms")]
    pub timeout_ms: u64,
}

fn default_script_language() -> String {
    "javascript".to_string()
}

fn default_script_timeout_ms() -> u64 {
    15000
}
```

- [ ] **Step 2: 在 ProviderError 上加 is_transient 方法**

在 `src-tauri/src/providers/traits.rs` 的 `ProviderError` 枚举后（`impl From<reqwest::Error>` 之前）追加：

```rust
impl ProviderError {
    /// 是否为瞬时错误（网络抖动 / 限流，前端应保留上次成功值并重试）
    pub fn is_transient(&self) -> bool {
        matches!(
            self,
            ProviderError::RequestError(_) | ProviderError::RateLimited(_)
        )
    }
}
```

- [ ] **Step 3: 运行 cargo check 确认新类型定义无误**

Run: `cd src-tauri && cargo check 2>&1 | grep "error\[" | wc -l`
Expected: 错误数应比 Task 1 之后略多或持平（因为新类型还没被使用，但旧引用还坏着）。重点确认没有新类型的语法错误。

Run: `cd src-tauri && cargo check 2>&1 | grep "types.rs"`
Expected: types.rs 本身不应有错误（新类型定义合法）。

- [ ] **Step 4: 暂不提交（与 Task 1 合并提交）**

---

## Task 3: 适配 UsageProvider trait 与 ProviderManager 容器

**Files:**
- Modify: `src-tauri/src/providers/traits.rs:7-29`
- Modify: `src-tauri/src/providers/mod.rs:1-135`

**Interfaces:**
- Consumes: `ProviderId`（String）、`ProviderTemplate`（Task 2）
- Produces: 改造后的 `UsageProvider` trait（id 返回 `String`）、`ProviderManager`（持有 registry，删除硬编码 insert）

本任务暂时保留 `UsageProvider` trait（让现有 3 家 provider 还能实现它），但 `ProviderManager` 改为不再硬编码 insert，而是从 registry 取模板。现有 3 家 provider 的 `fetch_usage` 等方法暂保留，Task 5-7 会把它们迁移到模板化。

- [ ] **Step 1: 修改 UsageProvider trait，id() 返回 String**

在 `src-tauri/src/providers/traits.rs` 中，把 trait 定义（第 7-29 行）改为：

```rust
/// 用量供应商核心抽象
///
/// 新增供应商只需实现此 trait，然后在 ProviderManager 中注册即可。
/// 注意：阶段 1 期间，此 trait 仍由 openai/anthropic/openrouter 三个旧 provider 实现，
/// 但 ProviderManager 已改为优先从 ProviderRegistry 取模板查询。
/// 阶段 2 起，新供应商不再实现此 trait，而是通过 registry 的 QueryType 配置。
#[async_trait]
pub trait UsageProvider: Send + Sync {
    /// 供应商唯一 ID
    fn id(&self) -> String;

    /// 显示名称
    fn display_name(&self) -> &str;

    /// 供应商支持的能力
    fn capabilities(&self) -> ProviderCapabilities;

    /// 获取用量数据
    async fn fetch_usage(&self, api_key: &str) -> Result<UsageData, ProviderError>;

    /// 获取速率限制数据
    async fn fetch_rate_limits(
        &self,
        api_key: &str,
    ) -> Result<Option<RateLimitData>, ProviderError>;

    /// 验证 API Key 是否有效
    async fn validate_key(&self, api_key: &str) -> Result<bool, ProviderError>;
}
```

唯一变化：`fn id(&self) -> ProviderId`（旧返回枚举）改为 `fn id(&self) -> String`（新返回 String）。

- [ ] **Step 2: 改造 ProviderManager，删除硬编码 insert，改为持有 registry 引用**

把 `src-tauri/src/providers/mod.rs` 全文替换为：

```rust
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

use traits::UsageProvider;
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
            match self
                .execute_usage_query(spec, api_key, custom_config)
                .await
            {
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
                    return Err(ProviderError::AuthError(last_error.map(|e| e.to_string()).unwrap_or_default()).to_string());
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

        for spec in template.queries.iter().filter(|q| q.is_subscription_query()) {
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
            QueryType::Balance { url, auth, field_map } => {
                balance::execute_balance_query(
                    &self.http_client,
                    url,
                    auth,
                    field_map,
                    api_key,
                )
                .await
            }
            QueryType::CodingPlan { provider: _ } => {
                // 阶段 2 实现
                Err(ProviderError::RequestError(
                    "CodingPlan 查询将在阶段 2 实现".into(),
                ))
            }
            QueryType::Subscription { .. } => {
                Err(ProviderError::RequestError(
                    "订阅查询不应走用量链路".into(),
                ))
            }
            QueryType::Script { default_template } => {
                let code = custom_config
                    .and_then(|c| c.script.as_ref())
                    .map(|s| s.code.as_str())
                    .or(default_template.as_deref())
                    .ok_or_else(|| {
                        ProviderError::RequestError("未提供脚本代码".into())
                    })?;
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
            return legacy.validate_key(api_key).await.map_err(|e| e.to_string());
        }
        // 新版：尝试执行一次用量查询，成功即有效
        match self.fetch_api_usage(provider_id, api_key, custom_config).await {
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
```

- [ ] **Step 3: 暂不创建 registry.rs / balance.rs / script_engine.rs（下一个任务创建）**

本任务后 cargo check 仍会失败（缺少 registry/balance/script_engine 模块）。Task 4-6 创建这些文件。

- [ ] **Step 4: 暂不提交（继续 Task 4）**

---

## Task 4: 创建 Provider Registry

**Files:**
- Create: `src-tauri/src/providers/registry.rs`

**Interfaces:**
- Consumes: `ProviderTemplate`、`QueryType`、`AuthScheme`、`BalanceFieldMap`、`QuerySpec`、`ProviderCapabilities`（Task 2）
- Produces: `registry::get(id) -> Option<ProviderTemplate>`、`registry::all() -> Vec<ProviderTemplate>`

- [ ] **Step 1: 创建 registry.rs，定义内置 5 家供应商模板**

创建 `src-tauri/src/providers/registry.rs`：

```rust
use super::types::*;

/// 内置供应商注册表
///
/// 新增内置供应商 = 在此函数返回的 Vec 里追加一条 ProviderTemplate。
/// 不再需要修改任何 match 分支。
fn builtin_templates() -> Vec<ProviderTemplate> {
    vec![
        // === OpenAI（复合型：Balance × 3 + Subscription × 1）===
        ProviderTemplate {
            id: "openai".to_string(),
            display_name: "OpenAI".to_string(),
            env_key_name: "OPENAI_API_KEY".to_string(),
            env_oauth_token_name: Some("OPENAI_OAUTH_TOKEN".to_string()),
            icon: "openai".to_string(),
            docs_url: Some("https://platform.openai.com/api-keys".to_string()),
            capabilities: ProviderCapabilities {
                has_balance: true,
                has_usage: true,
                has_rate_limit: false,
                has_subscription: true,
            },
            queries: vec![
                // 1. 预付费 credit grants
                QuerySpec {
                    query_type: QueryType::Balance {
                        url: "https://api.openai.com/v1/dashboard/billing/credit_grants".to_string(),
                        auth: AuthScheme::Bearer,
                        field_map: BalanceFieldMap {
                            total: "$.total_granted".to_string(),
                            used: Some("$.total_used".to_string()),
                            remaining: Some("$.total_available".to_string()),
                            currency: "USD".to_string(),
                        },
                    },
                    base_url: None,
                },
                // 2. 后付费 costs（本月）
                QuerySpec {
                    query_type: QueryType::Balance {
                        url: format!(
                            "https://api.openai.com/v1/organization/costs?start_time={}&end_time={}&group_by=line_item",
                            current_month_start_timestamp(),
                            current_timestamp()
                        ),
                        auth: AuthScheme::Bearer,
                        field_map: BalanceFieldMap {
                            total: "$.total".to_string(),
                            used: Some("$.used".to_string()),
                            remaining: None,
                            currency: "USD".to_string(),
                        },
                    },
                    base_url: None,
                },
                // 3. 限额 subscription
                QuerySpec {
                    query_type: QueryType::Balance {
                        url: "https://api.openai.com/v1/dashboard/billing/subscription".to_string(),
                        auth: AuthScheme::Bearer,
                        field_map: BalanceFieldMap {
                            total: "$.hard_limit_usd".to_string(),
                            used: None,
                            remaining: None,
                            currency: "USD".to_string(),
                        },
                    },
                    base_url: None,
                },
                // 4. OAuth 订阅（ChatGPT Plus/Pro/Max）
                QuerySpec {
                    query_type: QueryType::Subscription {
                        provider: "openai_wham".to_string(),
                    },
                    base_url: None,
                },
            ],
            oauth_detect: None, // 阶段 2 填充
        },
        // === Anthropic（Balance × 1 + Subscription × 1）===
        ProviderTemplate {
            id: "anthropic".to_string(),
            display_name: "Anthropic".to_string(),
            env_key_name: "ANTHROPIC_API_KEY".to_string(),
            env_oauth_token_name: Some("ANTHROPIC_OAUTH_TOKEN".to_string()),
            icon: "anthropic".to_string(),
            docs_url: Some("https://docs.anthropic.com/en/api/getting-started".to_string()),
            capabilities: ProviderCapabilities {
                has_balance: false,
                has_usage: true,
                has_rate_limit: true,
                has_subscription: true,
            },
            queries: vec![
                // 1. cost_report（按量，x-api-key 认证）
                QuerySpec {
                    query_type: QueryType::Balance {
                        url: format!(
                            "https://api.anthropic.com/v1/organizations/cost_report?start_date={}&end_date={}",
                            current_month_start_date(),
                            current_date()
                        ),
                        auth: AuthScheme::XApiKey,
                        field_map: BalanceFieldMap {
                            total: "$.total".to_string(),
                            used: Some("$.used".to_string()),
                            remaining: None,
                            currency: "USD".to_string(),
                        },
                    },
                    base_url: None,
                },
                // 2. OAuth 订阅
                QuerySpec {
                    query_type: QueryType::Subscription {
                        provider: "anthropic_oauth".to_string(),
                    },
                    base_url: None,
                },
            ],
            oauth_detect: None,
        },
        // === OpenRouter（Balance × 2，回退链路）===
        ProviderTemplate {
            id: "openrouter".to_string(),
            display_name: "OpenRouter".to_string(),
            env_key_name: "OPENROUTER_API_KEY".to_string(),
            env_oauth_token_name: None,
            icon: "openrouter".to_string(),
            docs_url: Some("https://openrouter.ai/keys".to_string()),
            capabilities: ProviderCapabilities {
                has_balance: true,
                has_usage: true,
                has_rate_limit: true,
                has_subscription: false,
            },
            queries: vec![
                // 主链路：/api/v1/credits
                QuerySpec {
                    query_type: QueryType::Balance {
                        url: "https://openrouter.ai/api/v1/credits".to_string(),
                        auth: AuthScheme::Bearer,
                        field_map: BalanceFieldMap {
                            total: "$.data.total_credits".to_string(),
                            used: Some("$.data.total_usage".to_string()),
                            remaining: None, // = total - used
                            currency: "USD".to_string(),
                        },
                    },
                    base_url: None,
                },
                // 回退链路：/api/v1/key
                QuerySpec {
                    query_type: QueryType::Balance {
                        url: "https://openrouter.ai/api/v1/key".to_string(),
                        auth: AuthScheme::Bearer,
                        field_map: BalanceFieldMap {
                            total: "$.data.limit".to_string(),
                            used: Some("$.data.usage".to_string()),
                            remaining: None,
                            currency: "USD".to_string(),
                        },
                    },
                    base_url: None,
                },
            ],
            oauth_detect: None,
        },
        // === DeepSeek（Balance × 1，新增）===
        ProviderTemplate {
            id: "deepseek".to_string(),
            display_name: "DeepSeek".to_string(),
            env_key_name: "DEEPSEEK_API_KEY".to_string(),
            env_oauth_token_name: None,
            icon: "deepseek".to_string(),
            docs_url: Some("https://platform.deepseek.com/api-keys".to_string()),
            capabilities: ProviderCapabilities {
                has_balance: true,
                has_usage: false,
                has_rate_limit: false,
                has_subscription: false,
            },
            queries: vec![QuerySpec {
                query_type: QueryType::Balance {
                    url: "https://api.deepseek.com/user/balance".to_string(),
                    auth: AuthScheme::Bearer,
                    field_map: BalanceFieldMap {
                        total: "$.balance_infos[0].total_balance".to_string(),
                        used: None,
                        remaining: "$.balance_infos[0].total_balance".to_string(),
                        currency: "USD".to_string(),
                    },
                },
                base_url: None,
            }],
            oauth_detect: None,
        },
        // === NewAPI（Script，预置脚本）===
        ProviderTemplate {
            id: "newapi".to_string(),
            display_name: "NewAPI".to_string(),
            env_key_name: "NEWAPI_API_KEY".to_string(),
            env_oauth_token_name: None,
            icon: "newapi".to_string(),
            docs_url: Some("https://github.com/Calcium-Ion/new-api".to_string()),
            capabilities: ProviderCapabilities {
                has_balance: true,
                has_usage: false,
                has_rate_limit: false,
                has_subscription: false,
            },
            queries: vec![QuerySpec {
                query_type: QueryType::Script {
                    default_template: Some(newapi_script_template().to_string()),
                },
                base_url: None,
            }],
            oauth_detect: None,
        },
    ]
}

/// NewAPI 预置 JS 脚本模板
pub fn newapi_script_template() -> &'static str {
    r#"({
  request: {
    url: "{{baseUrl}}/api/user/self",
    method: "GET",
    headers: {
      "Authorization": "Bearer {{accessToken}}",
      "Content-Type": "application/json",
      "User-Agent": "PeekaUsage/1.0",
      "New-Api-User": "{{userId}}"
    }
  },
  extractor: function(response) {
    if (response.success && response.data) {
      return {
        planName: response.data.group || "默认分组",
        remaining: response.data.quota / 500000,
        used: response.data.used_quota / 500000,
        total: (response.data.quota + response.data.used_quota) / 500000,
        currency: "USD"
      };
    }
    return { isValid: false, invalidMessage: response.message || "查询失败" };
  }
})"#
}

/// 按 ID 获取内置供应商模板
pub fn get(id: &str) -> Option<ProviderTemplate> {
    builtin_templates().into_iter().find(|t| t.id == id)
}

/// 获取所有内置供应商模板
pub fn all() -> Vec<ProviderTemplate> {
    builtin_templates()
}

// === 时间辅助函数 ===

fn current_timestamp() -> String {
    use chrono::Utc;
    Utc::now().timestamp().to_string()
}

fn current_month_start_timestamp() -> String {
    use chrono::{Datelike, Utc};
    let now = Utc::now();
    let start = now
        .with_day(1)
        .and_then(|d| d.with_hour(0))
        .and_then(|d| d.with_minute(0))
        .and_then(|d| d.with_second(0))
        .and_then(|d| d.with_nanosecond(0))
        .unwrap_or(now);
    start.timestamp().to_string()
}

fn current_date() -> String {
    use chrono::Utc;
    Utc::now().format("%Y-%m-%d").to_string()
}

fn current_month_start_date() -> String {
    use chrono::{Datelike, Utc};
    let now = Utc::now();
    now.with_day(1).map(|d| d.format("%Y-%m-%d").to_string()).unwrap_or_else(|| now.format("%Y-%m-%d").to_string())
}
```

- [ ] **Step 2: 运行 cargo check 确认 registry 模块编译通过**

Run: `cd src-tauri && cargo check 2>&1 | grep "registry.rs"`
Expected: registry.rs 本身不应有错误（除非 jsonpath 相关类型还没用上，但 registry 没直接用 jsonpath）。

- [ ] **Step 3: 暂不提交（继续 Task 5）**

---

## Task 5: 创建 Balance 查询模块

**Files:**
- Create: `src-tauri/src/providers/balance.rs`

**Interfaces:**
- Consumes: `AuthScheme`、`BalanceFieldMap`、`UsageData`、`ProviderError`（Task 2）
- Produces: `balance::execute_balance_query(client, url, auth, field_map, api_key) -> Result<UsageData, ProviderError>`

- [ ] **Step 1: 创建 balance.rs，实现 bytes-then-parse + jsonpath 提取**

创建 `src-tauri/src/providers/balance.rs`：

```rust
use reqwest::Client;
use serde_json::Value;

use super::traits::ProviderError;
use super::types::{AuthScheme, BalanceFieldMap, UsageData};

/// 执行 Balance 查询
///
/// 流程：
/// 1. 用 url + auth 构造请求
/// 2. 发请求（15s 超时），先 bytes() 再 serde_json::from_slice（区分网络错和解析错）
/// 3. 用 jsonpath 按 field_map 提取字段
/// 4. 组装 UsageData 返回
pub async fn execute_balance_query(
    client: &Client,
    url: &str,
    auth: &AuthScheme,
    field_map: &BalanceFieldMap,
    api_key: &str,
) -> Result<UsageData, ProviderError> {
    // 构造请求
    let req_builder = client.get(url);
    let req_builder = apply_auth(req_builder, auth, api_key);

    // 发请求
    let resp = req_builder
        .send()
        .await
        .map_err(|e| {
            if let Some(status) = e.status() {
                let code = status.as_u16();
                if code == 401 || code == 403 {
                    return ProviderError::AuthError(format!("认证失败 (HTTP {})", code));
                }
                if code == 429 {
                    return ProviderError::RateLimited("请求过于频繁".to_string());
                }
            }
            ProviderError::RequestError(e.to_string())
        })?;

    let status = resp.status();
    if status.as_u16() == 401 || status.as_u16() == 403 {
        return Err(ProviderError::AuthError(format!("认证失败 (HTTP {})", status.as_u16())));
    }
    if status.as_u16() == 429 {
        return Err(ProviderError::RateLimited("请求过于频繁".to_string()));
    }
    if !status.is_success() {
        return Err(ProviderError::RequestError(format!("HTTP {}", status)));
    }

    // bytes-then-parse 模式（抄 cc-switch，区分读体错和解析错）
    let body_bytes = resp
        .bytes()
        .await
        .map_err(|e| ProviderError::RequestError(format!("读取响应体失败: {}", e)))?;

    let json: Value = serde_json::from_slice(&body_bytes)
        .map_err(|e| ProviderError::ParseError(format!("解析 JSON 失败: {}", e)))?;

    // 用 jsonpath 提取字段
    let total = extract_field(&json, &field_map.total)?;
    let used = match &field_map.used {
        Some(path) => extract_field(&json, path)?,
        None => None,
    };
    let remaining = match &field_map.remaining {
        Some(path) => extract_field(&json, path)?,
        None => match (&total, &used) {
            (Some(t), Some(u)) => Some(t - u),
            _ => None,
        },
    };

    let total_budget = total;
    let total_used = used.unwrap_or(0.0);

    Ok(UsageData {
        total_used,
        total_budget,
        remaining,
        currency: field_map.currency.clone(),
        period_start: None,
        period_end: None,
    })
}

/// 给 reqwest 请求应用认证方案
fn apply_auth(
    mut builder: reqwest::RequestBuilder,
    auth: &AuthScheme,
    api_key: &str,
) -> reqwest::RequestBuilder {
    match auth {
        AuthScheme::Bearer => {
            builder = builder.bearer_auth(api_key);
        }
        AuthScheme::XApiKey => {
            builder = builder.header("x-api-key", api_key);
            // Anthropic 额外需要 anthropic-version header
            builder = builder.header("anthropic-version", "2023-06-01");
        }
        AuthScheme::RawKey => {
            builder = builder.header("Authorization", api_key);
        }
        AuthScheme::Custom(headers) => {
            for (key, value) in headers {
                // value 中的 {{apiKey}} 占位符替换为实际 key
                let resolved = value.replace("{{apiKey}}", api_key);
                builder = builder.header(key.as_str(), resolved);
            }
        }
    }
    builder
}

/// 用 JSONPath 从 JSON 提取 f64 字段
///
/// 使用 jsonpath-rust crate。支持简单路径如 "$.data.total" 和
/// 数组索引 "$.balance_infos[0].total_balance"。
/// 返回 None 表示字段不存在（非错误）。
fn extract_field(json: &Value, path: &str) -> Result<Option<f64>, ProviderError> {
    use jsonpath_rust::JsonPath;

    let jp = JsonPath::try_parse(path).map_err(|e| {
        ProviderError::ParseError(format!("无效的 JSONPath '{}': {}", path, e))
    })?;

    let results = jp.find(json);

    if results.is_empty() {
        return Ok(None);
    }

    let value = &results[0];

    // 支持 number / string 形式的数字
    let num = match value {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.parse::<f64>().ok(),
        _ => None,
    };

    Ok(num)
}
```

- [ ] **Step 2: 运行 cargo check 确认 balance 模块编译通过**

Run: `cd src-tauri && cargo check 2>&1 | grep "balance.rs"`
Expected: balance.rs 不应有错误。如果 `jsonpath_rust::JsonPath` 的 API 名字不对，根据编译错误修正（不同版本 API 略有差异，核心是 parse + find）。

- [ ] **Step 3: 暂不提交（继续 Task 6）**

---

## Task 6: 创建 JS 脚本引擎模块

**Files:**
- Create: `src-tauri/src/providers/script_engine.rs`

**Interfaces:**
- Consumes: `reqwest::Client`、`ProviderError`、`UsageData`（Task 2）
- Produces: `script_engine::run(client, code, api_key, base_url, allow_http, timeout_ms) -> Result<UsageData, ProviderError>`

- [ ] **Step 1: 创建 script_engine.rs，实现 rquickjs 沙箱**

创建 `src-tauri/src/providers/script_engine.rs`：

```rust
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::traits::ProviderError;
use super::types::UsageData;

/// 脚本提取结果（JS extractor 返回值）
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExtractResult {
    #[serde(default)]
    plan_name: Option<String>,
    #[serde(default)]
    total: Option<f64>,
    #[serde(default)]
    used: Option<f64>,
    #[serde(default)]
    remaining: Option<f64>,
    #[serde(default)]
    currency: Option<String>,
    #[serde(default)]
    is_valid: Option<bool>,
    #[serde(default)]
    invalid_message: Option<String>,
}

/// 执行 JS 脚本查询
///
/// 流程：
/// 1. 模板变量替换（{{apiKey}} / {{baseUrl}} / {{accessToken}} / {{userId}}）
/// 2. 在 rquickjs 沙箱中执行脚本，拿到 {request, extractor}
/// 3. Rust 端按 request 发 HTTP 请求（脚本不能自己发）
/// 4. 把响应喂回 extractor 函数
/// 5. 转换结果为 UsageData
pub async fn run(
    client: &Client,
    code: &str,
    api_key: &str,
    base_url: Option<&str>,
    allow_http: bool,
    timeout_ms: u64,
) -> Result<UsageData, ProviderError> {
    // 1. 模板变量替换
    let resolved_code = code
        .replace("{{apiKey}}", api_key)
        .replace("{{baseUrl}}", base_url.unwrap_or(""));

    // NewAPI 用 accessToken / userId，阶段 1 暂用 api_key 占位
    // （实际 accessToken/userId 通过 KeyStore 读取，在命令层注入，这里先留接口）
    let resolved_code = resolved_code
        .replace("{{accessToken}}", api_key)
        .replace("{{userId}}", "");

    // 2. 在沙箱中执行脚本，拿到 request 配置
    let request_spec = run_script_for_request(&resolved_code)?;

    // 3. 安全校验
    validate_request_url(&request_spec.url, base_url, allow_http)?;

    // 4. Rust 端发请求
    let response = send_request(client, &request_spec, timeout_ms).await?;

    // 5. 把响应喂回 extractor
    let result = run_extractor(&resolved_code, &response)?;

    // 6. 转换结果
    if result.is_valid == Some(false) {
        return Err(ProviderError::RequestError(
            result
                .invalid_message
                .unwrap_or_else(|| "脚本返回无效结果".to_string()),
        ));
    }

    Ok(UsageData {
        total_used: result.used.unwrap_or(0.0),
        total_budget: result.total,
        remaining: result.remaining,
        currency: result.currency.unwrap_or_else(|| "credits".to_string()),
        period_start: None,
        period_end: None,
    })
}

/// 脚本中的 request 配置
#[derive(Debug, Deserialize)]
struct RequestSpec {
    url: String,
    #[serde(default = "default_method")]
    method: String,
    #[serde(default)]
    headers: std::collections::HashMap<String, String>,
    #[serde(default)]
    body: Option<Value>,
}

fn default_method() -> String {
    "GET".to_string()
}

/// 脚本执行后的 HTTP 响应（传给 extractor）
#[derive(Debug, Serialize)]
struct ScriptResponse {
    status: u16,
    success: bool,
    data: Value,
    headers: std::collections::HashMap<String, String>,
}

/// 在 rquickjs 沙箱中执行脚本，返回 request 配置
fn run_script_for_request(code: &str) -> Result<RequestSpec, ProviderError> {
    use rquickjs::{Function, Object, Runtime, Value};

    let runtime = Runtime::new().map_err(|e| {
        ProviderError::ParseError(format!("无法创建 JS 运行时: {}", e))
    })?;

    let result = runtime
        .spawn::<Result<RequestSpec, ProviderError>, _>(move |ctx| {
            // 执行脚本代码（返回一个对象 {request, extractor}）
            let value: Value = ctx.eval(code).map_err(|e| {
                ProviderError::ParseError(format!("执行脚本失败: {}", e))
            })?;

            let obj: Object = value
                .into_object()
                .ok_or_else(|| ProviderError::ParseError("脚本未返回对象".into()))?;

            // 取 request 属性
            let request_value = obj
                .get("request")
                .map_err(|e| ProviderError::ParseError(format!("读取 request 失败: {}", e)))?;

            let request_obj = request_value
                .into_object()
                .ok_or_else(|| ProviderError::ParseError("request 不是对象".into()))?;

            // 提取字段
            let url: String = request_obj
                .get("url")
                .map_err(|e| ProviderError::ParseError(format!("读取 url 失败: {}", e)))?;

            let method: String = request_obj
                .get("method")
                .unwrap_or_else(|_| "GET".to_string());

            let headers = request_obj
                .get("headers")
                .ok()
                .and_then(|v| v.into_object())
                .map(|obj| {
                    let mut map = std::collections::HashMap::new();
                    for key in obj.keys::<String>() {
                        if let Ok(key) = key {
                            if let Ok(val) = obj.get::<String, String>(key.clone()) {
                                map.insert(key, val);
                            }
                        }
                    }
                    map
                })
                .unwrap_or_default();

            let body = request_obj.get("body").ok();

            Ok(RequestSpec {
                url,
                method,
                headers,
                body,
            })
        })
        .join()
        .map_err(|e| ProviderError::ParseError(format!("JS 运行时错误: {}", e)))??;

    Ok(result)
}

/// 校验请求 URL：HTTPS 强制 + 同源校验
fn validate_request_url(
    url: &str,
    base_url: Option<&str>,
    allow_http: bool,
) -> Result<(), ProviderError> {
    let parsed = url::Url::parse(url).map_err(|e| {
        ProviderError::ParseError(format!("无效的 URL '{}': {}", url, e))
    })?;

    // HTTPS 强制
    if parsed.scheme() == "http" && !allow_http {
        return Err(ProviderError::RequestError(
            "默认禁止 HTTP 请求，请在自定义供应商配置中开启'允许 HTTP'".into(),
        ));
    }

    if parsed.scheme() != "https" && parsed.scheme() != "http" {
        return Err(ProviderError::RequestError(format!(
            "不支持的协议: {}",
            parsed.scheme()
        )));
    }

    // 同源校验：如果有 base_url，请求 URL 的 host:port 必须和 base_url 一致
    if let Some(base) = base_url {
        if let Ok(base_parsed) = url::Url::parse(base) {
            let req_host = parsed.host_str();
            let base_host = base_parsed.host_str();
            let req_port = parsed.port_or_known_default();
            let base_port = base_parsed.port_or_known_default();

            if req_host != base_host || req_port != base_port {
                return Err(ProviderError::RequestError(format!(
                    "请求 URL 的主机 {} 与 base_url 的主机 {} 不一致（同源校验失败）",
                    req_host.unwrap_or("unknown"),
                    base_host.unwrap_or("unknown")
                )));
            }
        }
    }

    Ok(())
}

/// Rust 端发送 HTTP 请求
async fn send_request(
    client: &Client,
    spec: &RequestSpec,
    timeout_ms: u64,
) -> Result<ScriptResponse, ProviderError> {
    let method = match spec.method.to_uppercase().as_str() {
        "GET" => reqwest::Method::GET,
        "POST" => reqwest::Method::POST,
        "PUT" => reqwest::Method::PUT,
        "DELETE" => reqwest::Method::DELETE,
        _ => reqwest::Method::GET,
    };

    let mut builder = client
        .request(method, &spec.url)
        .timeout(std::time::Duration::from_millis(timeout_ms));

    for (key, value) in &spec.headers {
        builder = builder.header(key, value);
    }

    if let Some(body) = &spec.body {
        builder = builder.json(body);
    }

    let resp = builder.send().await.map_err(|e| {
        if let Some(status) = e.status() {
            let code = status.as_u16();
            if code == 401 || code == 403 {
                return ProviderError::AuthError(format!("认证失败 (HTTP {})", code));
            }
            if code == 429 {
                return ProviderError::RateLimited("请求过于频繁".to_string());
            }
        }
        ProviderError::RequestError(e.to_string())
    })?;

    let status = resp.status();
    let success = status.is_success();

    // 收集响应 header
    let mut headers = std::collections::HashMap::new();
    for (key, value) in resp.headers() {
        if let Ok(v) = value.to_str() {
            headers.insert(key.as_str().to_string(), v.to_string());
        }
    }

    // bytes-then-parse
    let body_bytes = resp
        .bytes()
        .await
        .map_err(|e| ProviderError::RequestError(format!("读取响应体失败: {}", e)))?;

    let data: Value = serde_json::from_slice(&body_bytes).unwrap_or(Value::Null);

    Ok(ScriptResponse {
        status: status.as_u16(),
        success,
        data,
        headers,
    })
}

/// 在 rquickjs 沙箱中执行 extractor 函数
fn run_extractor(code: &str, response: &ScriptResponse) -> Result<ExtractResult, ProviderError> {
    use rquickjs::{Function, Object, Runtime, Value};

    let runtime = Runtime::new().map_err(|e| {
        ProviderError::ParseError(format!("无法创建 JS 运行时: {}", e))
    })?;

    let response_json = serde_json::to_string(response).map_err(|e| {
        ProviderError::ParseError(format!("序列化响应失败: {}", e))
    })?;

    let result = runtime
        .spawn::<Result<ExtractResult, ProviderError>, _>(move |ctx| {
            // 执行脚本拿到 {request, extractor} 对象
            let value: Value = ctx.eval(code).map_err(|e| {
                ProviderError::ParseError(format!("执行脚本失败: {}", e))
            })?;

            let obj: Object = value
                .into_object()
                .ok_or_else(|| ProviderError::ParseError("脚本未返回对象".into()))?;

            // 取 extractor 函数
            let extractor: Function = obj
                .get("extractor")
                .map_err(|e| ProviderError::ParseError(format!("读取 extractor 失败: {}", e)))?;

            // 把 response JSON 解析成 JS 对象，传给 extractor
            let response_value: Value = ctx
                .eval(format!("({})", response_json))
                .map_err(|e| ProviderError::ParseError(format!("解析响应失败: {}", e)))?;

            // 调用 extractor
            let result_value: Value = extractor
                .call((response_value,))
                .map_err(|e| ProviderError::ParseError(format!("执行 extractor 失败: {}", e)))?;

            // 把结果转成 JSON 再反序列化
            let result_json: String = ctx
                .eval(format!("JSON.stringify((function(){{return arguments[0];}})({}))", {
                    // 把 result_value 重新 stringify
                    // rquickjs 的 stringify 方式
                    let val: Value = result_value;
                    // 用 JSON.stringify
                    ctx.eval(format!("JSON.stringify(rquickjs_tmp)"))
                }))
                .unwrap_or_else(|_| "{}".to_string());

            serde_json::from_str::<ExtractResult>(&result_json).map_err(|e| {
                ProviderError::ParseError(format!("解析 extractor 结果失败: {}", e))
            })
        })
        .join()
        .map_err(|e| ProviderError::ParseError(format!("JS 运行时错误: {}", e)))??;

    Ok(result)
}
```

注意：rquickjs 的 API 在不同版本间有差异。如果上面的 `ctx.eval` / `runtime.spawn` 签名对不上，参考 `rquickjs` 0.7 的文档调整。核心思路是：执行脚本得到 `{request, extractor}` 对象，Rust 发请求后把响应喂给 extractor。

- [ ] **Step 2: 在 Cargo.toml 添加 url crate 依赖**

在 `src-tauri/Cargo.toml` 的 `[dependencies]` 追加：

```toml
url = "2"
```

- [ ] **Step 3: 运行 cargo check 修复 rquickjs API 差异**

Run: `cd src-tauri && cargo check 2>&1 | grep "script_engine.rs"`

根据编译错误修正 rquickjs API 调用。常见调整点：
- `runtime.spawn` 可能需要不同的泛型参数
- `ctx.eval` 返回类型可能需要显式标注
- `Object::get` 的泛型参数可能不同

修正原则：保持沙箱安全边界（只注入 JSON/Math/Date，不注入 fetch/setTimeout/文件 IO），其它 API 细节按编译器提示调整。

- [ ] **Step 4: 暂不提交（继续 Task 7）**

---

## Task 7: 适配旧版三个 provider + 修复编译并提交

**Files:**
- Modify: `src-tauri/src/providers/openai.rs:50-193`
- Modify: `src-tauri/src/providers/anthropic.rs:42-159`
- Modify: `src-tauri/src/providers/openrouter.rs:50-173`
- Modify: `src-tauri/src/providers/subscription.rs`
- Modify: `src-tauri/src/commands/provider_commands.rs`

本任务把三个旧 provider 的 `id()` 返回值从 `ProviderId::OpenAI` 枚举改为 `String`，并修复所有编译错误，达到可编译状态。

- [ ] **Step 1: 修复 openai.rs 的 id() 方法**

在 `src-tauri/src/providers/openai.rs` 中，把 `fn id(&self) -> ProviderId` 的函数体从：

```rust
fn id(&self) -> ProviderId {
    ProviderId::OpenAI
}
```

改为：

```rust
fn id(&self) -> String {
    "openai".to_string()
}
```

- [ ] **Step 2: 修复 anthropic.rs 的 id() 方法**

在 `src-tauri/src/providers/anthropic.rs` 中，把 `fn id(&self) -> ProviderId` 的函数体从：

```rust
fn id(&self) -> ProviderId {
    ProviderId::Anthropic
}
```

改为：

```rust
fn id(&self) -> String {
    "anthropic".to_string()
}
```

- [ ] **Step 3: 修复 openrouter.rs 的 id() 方法**

在 `src-tauri/src/providers/openrouter.rs` 中，把 `fn id(&self) -> ProviderId` 的函数体从：

```rust
fn id(&self) -> ProviderId {
    ProviderId::OpenRouter
}
```

改为：

```rust
fn id(&self) -> String {
    "openrouter".to_string()
}
```

- [ ] **Step 4: 改造 subscription.rs，把 fetch_anthropic/fetch_openai 重命名为统一 fetch**

在 `src-tauri/src/providers/subscription.rs` 中，把 `fetch_anthropic` 和 `fetch_openai` 方法改名为统一的 `fetch`，按 provider 字符串分发：

把现有的：

```rust
pub async fn fetch_anthropic(&self, oauth_token: &str) -> SubscriptionUsage { ... }
pub async fn fetch_openai(&self, oauth_token: &str) -> SubscriptionUsage { ... }
```

改为：

```rust
/// 统一订阅查询入口，按 provider 字符串分发
pub async fn fetch(&self, provider: &str, oauth_token: &str) -> SubscriptionUsage {
    match provider {
        "anthropic_oauth" => self.fetch_anthropic_oauth(oauth_token).await,
        "openai_wham" => self.fetch_openai_wham(oauth_token).await,
        _ => SubscriptionUsage {
            plan_name: None,
            windows: vec![],
            extra_usage: None,
            status: ProviderStatus::Error,
            error_message: Some(format!("不支持的订阅供应商: {}", provider)),
        },
    }
}

/// Anthropic OAuth 订阅查询（原 fetch_anthropic）
async fn fetch_anthropic_oauth(&self, oauth_token: &str) -> SubscriptionUsage {
    // ... 原有 fetch_anthropic 的实现不变，只是改名 ...
}

/// OpenAI Wham 订阅查询（原 fetch_openai）
async fn fetch_openai_wham(&self, oauth_token: &str) -> SubscriptionUsage {
    // ... 原有 fetch_openai 的实现不变，只是改名 ...
}
```

保留原有两个方法的完整实现体，只改方法名和可见性（改为私有，由公开的 `fetch` 分发）。

- [ ] **Step 5: 运行 cargo check，逐个修复剩余编译错误**

Run: `cd src-tauri && cargo check 2>&1 | grep "error\[" | head -30`

此时主要错误来源是 `provider_commands.rs` 中：
- `parse_provider_id` 函数（返回 `ProviderId` 枚举，已删除）
- `save_provider_order` 的 `matches!` 过滤
- `build_usage_summary` 调用 `fetch_api_usage` / `fetch_subscription_usage` 的签名变化（现在需要传 custom_config）
- `ProviderConfig.provider_id` 类型从 `ProviderId` 改 `String`

逐个修复（具体代码在 Task 8）。

- [ ] **Step 6: 暂不提交（Task 8 修复 provider_commands 后一起提交）**

---

## Task 8: 改造 provider_commands.rs

**Files:**
- Modify: `src-tauri/src/commands/provider_commands.rs`

**Interfaces:**
- Consumes: 改造后的 `ProviderManager`（Task 3）、`ProviderTemplate`（Task 2）
- Produces: 修复后的命令函数 + 新增 `get_provider_templates` / `test_custom_provider_script` / `get_newapi_script_template`

由于 `provider_commands.rs` 改动量大（500+ 行），本任务给出关键改动的代码片段，完整文件在执行时按这些片段逐一替换。

- [ ] **Step 1: 删除 parse_provider_id 函数**

在 `src-tauri/src/commands/provider_commands.rs` 中，删除 `parse_provider_id` 函数（原第 728-735 行）及其所有调用点。provider_id 现在直接是 `String`，不需要解析。

- [ ] **Step 2: 修改 save_provider_order 的过滤逻辑**

把 `save_provider_order` 函数中的过滤：

```rust
let valid_ids: Vec<String> = provider_ids
    .into_iter()
    .filter(|id| matches!(id.as_str(), "openai" | "anthropic" | "openrouter"))
    .collect();
```

改为：

```rust
let valid_ids: Vec<String> = provider_ids
    .into_iter()
    .filter(|id| {
        // 内置供应商在 registry 里 OR 自定义供应商（custom_ 前缀）
        crate::providers::registry::get(id).is_some() || id.starts_with("custom_")
    })
    .collect();
```

- [ ] **Step 3: 修改 build_usage_summary，适配新的 fetch_api_usage 签名**

`build_usage_summary` 中调用 `provider_manager.fetch_api_usage` 的地方，需要传入 `custom_config`。先从 `ProviderEntry` 取出 `custom_config`：

```rust
// 在 build_usage_summary 中
let custom_config_ref = entry.custom_config.as_ref();

// 调用改为
match provider_manager
    .fetch_api_usage(&entry.provider_id, &api_key.value, custom_config_ref)
    .await
{
    Ok((usage, rate_limit)) => { ... }
    Err(e) => { ... }
}
```

同理 `fetch_subscription_usage`：

```rust
match provider_manager
    .fetch_subscription_usage(&entry.provider_id, &oauth_token, custom_config_ref)
    .await
{
    // ...
}
```

- [ ] **Step 4: 修改 ProviderConfig 结构，provider_id 改 String，加 customConfig 字段**

`ProviderConfig`（在 types.rs，但被 provider_commands 使用）已经在 Task 2 改过 `ProviderId = String`。这里需要确保 `ProviderConfig` 能接收前端的 `customConfig`。

在 `src-tauri/src/providers/types.rs` 的 `ProviderConfig` 结构中追加：

```rust
/// 供应商配置（从前端传入）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConfig {
    pub provider_id: String,   // 已是 String（Task 1）
    pub enabled: bool,
    #[serde(default)]
    pub api_keys: Vec<ProviderApiKeyInput>,
    #[serde(default)]
    pub subscriptions: Vec<ProviderSubscriptionInput>,
    // 新增
    #[serde(default)]
    pub provider_template_id: Option<String>,
    #[serde(default)]
    pub custom_config: Option<CustomProviderConfig>,
}
```

- [ ] **Step 5: 修改 ProviderConfigItem 结构，加新字段**

在 `src-tauri/src/providers/types.rs` 的 `ProviderConfigItem` 追加：

```rust
pub struct ProviderConfigItem {
    pub provider_id: String,   // 已是 String
    pub display_name: String,
    pub enabled: bool,
    #[serde(default)]
    pub api_keys: Vec<ProviderApiKeyItem>,
    #[serde(default)]
    pub subscriptions: Vec<ProviderSubscriptionItem>,
    pub capabilities: ProviderCapabilities,
    #[serde(default)]
    pub environment_variable_name: String,
    #[serde(default)]
    pub active_api_key_id: Option<String>,
    // 新增
    #[serde(default)]
    pub provider_template_id: Option<String>,
    #[serde(default)]
    pub custom_config: Option<CustomProviderConfig>,
}
```

（`ProviderManager::get_provider_config_items` 在 Task 3 已经填了这些字段）

- [ ] **Step 6: 修改 get_provider_configs，填充新字段**

在 `provider_commands.rs::get_provider_configs` 中，构造 `ProviderConfigItem` 时填入 `provider_template_id` 和 `custom_config`：

```rust
// 在 get_provider_configs 中构造 ProviderConfigItem 时
provider_template_id: Some(entry.provider_id.clone()),  // 内置供应商
custom_config: entry.custom_config.clone(),              // 自定义供应商
```

对自定义供应商，`provider_template_id` 为 `None`，`custom_config` 为实际配置。

- [ ] **Step 7: 新增三个命令函数**

在 `provider_commands.rs` 末尾追加：

```rust
/// 获取所有可选供应商模板（含内置，用于设置页"新增供应商"下拉）
#[tauri::command]
pub async fn get_provider_templates(
    provider_manager: tauri::State<'_, std::sync::Arc<crate::providers::ProviderManager>>,
) -> Result<Vec<ProviderTemplate>, String> {
    Ok(provider_manager.get_provider_templates())
}

/// 获取 NewAPI 预置脚本模板
#[tauri::command]
pub async fn get_newapi_script_template() -> Result<String, String> {
    Ok(crate::providers::registry::newapi_script_template().to_string())
}

/// 测试自定义供应商脚本（保存前预演）
#[tauri::command]
pub async fn test_custom_provider_script(
    provider_manager: tauri::State<'_, std::sync::Arc<crate::providers::ProviderManager>>,
    code: String,
    api_key: String,
    base_url: Option<String>,
    allow_http: bool,
) -> Result<String, String> {
    // 执行脚本，返回成功/失败信息
    let result = crate::providers::script_engine::run(
        &provider_manager.http_client_ref(),
        &code,
        &api_key,
        base_url.as_deref(),
        allow_http,
        15000,
    )
    .await
    .map_err(|e| e.to_string())?;

    Ok(format!(
        "查询成功：已用 {} / 总额 {:?} / 剩余 {:?} ({})",
        result.total_used, result.total_budget, result.remaining, result.currency
    ))
}
```

注意：`ProviderManager` 需要暴露 `http_client_ref()` 方法返回 `&reqwest::Client`。在 `mod.rs` 的 `impl ProviderManager` 中追加：

```rust
/// 暴露 HTTP 客户端引用（供命令层调用 script_engine）
pub fn http_client_ref(&self) -> &reqwest::Client {
    &self.http_client
}
```

- [ ] **Step 8: 在 lib.rs 注册新命令**

在 `src-tauri/src/lib.rs` 的 `tauri::generate_handler!` 宏中追加三个命令：

```rust
// 在现有的 generate_handler! 列表中追加
get_provider_templates,
get_newapi_script_template,
test_custom_provider_script,
```

- [ ] **Step 9: 运行 cargo check，确认全量编译通过**

Run: `cd src-tauri && cargo check 2>&1 | tail -20`
Expected: 0 errors（可能有 warnings，先不管）

如果有错误，逐个修复。常见问题：
- `ProviderConfig.provider_id` 类型从枚举改 String 后，`serde` 反序列化前端传的 `"openai"` 字符串现在直接成功
- `build_usage_summary` 里 `ProviderId` 相关的 match 分支需要删掉
- `mask_value` 等辅助函数如果用了 `ProviderId` 也要改

- [ ] **Step 10: 运行 cargo fmt**

Run: `cd src-tauri && cargo fmt --all`

- [ ] **Step 11: 提交后端架构改造**

```bash
cd D:/Project/PeekaUsage
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/providers/ src-tauri/src/commands/provider_commands.rs src-tauri/src/config/ src-tauri/src/lib.rs
git commit -m "refactor(provider): 架构改造为配置驱动 + registry + JS 脚本引擎

- ProviderId 从枚举改为 String
- 新增 registry.rs 内置 5 家供应商模板
- 新增 balance.rs 通用余额查询（jsonpath 提取）
- 新增 script_engine.rs rquickjs 沙箱
- ProviderManager 改为查 registry 路由
- 迁移现有 3 家到新架构
- 新增 DeepSeek / NewAPI 供应商
- 新增 get_provider_templates / test_custom_provider_script / get_newapi_script_template 命令
- ProviderError 加 is_transient 方法"
```

---

## Task 9: 改造 app_config.rs 和 system_env.rs

**Files:**
- Modify: `src-tauri/src/config/app_config.rs`
- Modify: `src-tauri/src/config/system_env.rs`

- [ ] **Step 1: 在 ProviderEntry 加新字段**

在 `src-tauri/src/config/app_config.rs` 的 `ProviderEntry` 结构中追加：

```rust
pub struct ProviderEntry {
    pub provider_id: String,
    pub enabled: bool,
    #[serde(default)]
    pub api_keys: Vec<ProviderApiKeyEntry>,
    #[serde(default)]
    pub subscriptions: Vec<ProviderSubscriptionEntry>,
    #[serde(default)]
    pub active_api_key_id: Option<String>,
    #[serde(default)]
    pub manage_api_key_environment: bool,
    // 新增
    #[serde(default)]
    pub provider_template_id: Option<String>,
    #[serde(default)]
    pub custom_config: Option<crate::providers::types::CustomProviderConfig>,
}
```

- [ ] **Step 2: 修改 is_supported_provider_id**

```rust
fn is_supported_provider_id(provider_id: &str, entry: Option<&ProviderEntry>) -> bool {
    // 内置供应商在 registry 里
    if crate::providers::registry::get(provider_id).is_some() {
        return true;
    }
    // 自定义供应商
    if provider_id.starts_with("custom_") {
        return true;
    }
    // 有 custom_config 的也算
    if let Some(entry) = entry {
        return entry.custom_config.is_some();
    }
    false
}
```

注意：调用 `is_supported_provider_id` 的地方也要同步传入 `entry`。

- [ ] **Step 3: 修改 provider_rank**

```rust
fn provider_rank(provider_id: &str) -> usize {
    // 内置供应商按 registry 顺序排序，自定义供应商排最后
    let templates = crate::providers::registry::all();
    for (i, template) in templates.iter().enumerate() {
        if template.id == provider_id {
            return i;
        }
    }
    usize::MAX
}
```

- [ ] **Step 4: 修改 default_provider_card_expanded**

```rust
fn default_provider_card_expanded() -> std::collections::HashMap<String, bool> {
    let mut map = std::collections::HashMap::new();
    for template in crate::providers::registry::all() {
        map.insert(template.id.clone(), true);
    }
    map
}
```

- [ ] **Step 5: 修改 system_env.rs 的 supported_provider_ids 和 parse_provider_id**

在 `src-tauri/src/config/system_env.rs` 中，把：

```rust
fn supported_provider_ids() -> [&'static str; 3] {
    ["openai", "anthropic", "openrouter"]
}

fn parse_provider_id(id: &str) -> Option<ProviderId> { ... }
```

改为：

```rust
/// 同步环境变量时，遍历 config 里的所有 provider（内置 + 自定义），不硬编码列表
fn get_provider_ids_from_config(config: &crate::config::app_config::AppConfig) -> Vec<String> {
    config.get_provider_entries().keys().cloned().collect()
}
```

`sync_active_api_key_envs` 中调用环境变量名的逻辑，改为：
- 内置供应商：`registry::get(id).env_key_name`
- 自定义供应商：`entry.custom_config.env_key_name`（如果有）

- [ ] **Step 6: 运行 cargo check + cargo fmt**

Run: `cd src-tauri && cargo check 2>&1 | tail -5 && cargo fmt --all`

- [ ] **Step 7: 提交**

```bash
cd D:/Project/PeekaUsage
git add src-tauri/src/config/
git commit -m "refactor(config): app_config 和 system_env 适配配置驱动架构

- ProviderEntry 加 provider_template_id / custom_config 字段
- is_supported_provider_id / provider_rank 改为查 registry
- system_env 环境变量同步支持自定义供应商 envKeyName"
```

---

## Task 10: 前端类型同步

**Files:**
- Modify: `src/types/provider.ts`

- [ ] **Step 1: 把 ProviderId 改为 string，新增类型**

在 `src/types/provider.ts` 中，把：

```typescript
export type ProviderId = "openai" | "anthropic" | "openrouter";
```

改为：

```typescript
export type ProviderId = string;
```

并在文件末尾追加：

```typescript
/** 查询类型 */
export type QueryType =
  | { kind: "balance"; url: string; auth: AuthScheme; fieldMap: BalanceFieldMap }
  | { kind: "coding_plan"; provider: string }
  | { kind: "subscription"; provider: string }
  | { kind: "script"; defaultTemplate: string | null };

export type AuthScheme = "bearer" | "x_api_key" | "raw_key" | [string, string][];

export interface BalanceFieldMap {
  total: string;
  used?: string | null;
  remaining?: string | null;
  currency: string;
}

export interface QuerySpec {
  queryType: QueryType;
  baseUrl?: string | null;
}

export interface ProviderTemplate {
  id: string;
  displayName: string;
  envKeyName: string;
  envOauthTokenName?: string | null;
  queries: QuerySpec[];
  capabilities: ProviderCapabilities;
  icon: string;
  docsUrl?: string | null;
  oauthDetect?: OAuthDetectConfig | null;
}

export interface OAuthDetectConfig {
  filePath: string;
  tokenPath: string;
  keychainService?: string | null;
}

export type AuthSchemeConfig = "bearer" | "x_api_key" | "raw_key";
export type QueryTypeConfig = "balance" | "script";

export interface ScriptConfig {
  code: string;
  language: string;
  timeoutMs: number;
}

export interface CustomProviderConfig {
  displayName: string;
  baseUrl: string;
  authScheme: AuthSchemeConfig;
  envKeyName?: string | null;
  icon?: string | null;
  queryType: QueryTypeConfig;
  script?: ScriptConfig;
  allowHttp: boolean;
}
```

- [ ] **Step 2: 修改 ProviderConfigItem 和 ProviderConfig**

```typescript
export interface ProviderConfigItem {
  providerId: string;
  displayName: string;
  enabled: boolean;
  apiKeys: ProviderApiKeyItem[];
  subscriptions: ProviderSubscriptionItem[];
  capabilities: ProviderCapabilities;
  environmentVariableName: string;
  activeApiKeyId: string | null;
  // 新增
  providerTemplateId: string | null;
  customConfig: CustomProviderConfig | null;
}

// ProviderConfig（前端传给后端）
export interface ProviderConfig {
  providerId: string;
  enabled: boolean;
  apiKeys: ProviderApiKeyInput[];
  subscriptions: ProviderSubscriptionInput[];
  providerTemplateId?: string | null;
  customConfig?: CustomProviderConfig | null;
}
```

- [ ] **Step 3: 运行 tsc，修复所有类型错误**

Run: `cd D:/Project/PeekaUsage && npx tsc --noEmit 2>&1 | head -40`

逐个修复 TS 类型错误。常见问题：
- 用到 `ProviderId` 联合类型做 `switch` / 比较 的地方，现在 `ProviderId` 是 `string`，switch 仍能工作
- `as ProviderId` 断言现在不需要了，可删

- [ ] **Step 4: 提交**

```bash
cd D:/Project/PeekaUsage
git add src/types/provider.ts
git commit -m "refactor(types): ProviderId 改为 string，新增配置驱动类型定义"
```

---

## Task 11: 新增 IPC 接口

**Files:**
- Modify: `src/utils/ipc.ts`

- [ ] **Step 1: 添加三个新 IPC 函数**

在 `src/utils/ipc.ts` 中追加：

```typescript
import { invoke } from "@tauri-apps/api/core";
import type { ProviderTemplate, CustomProviderConfig } from "../types/provider";

/** 获取所有可选供应商模板（含内置，用于设置页"新增供应商"下拉） */
export async function getProviderTemplates(): Promise<ProviderTemplate[]> {
  return invoke<ProviderTemplate[]>("get_provider_templates");
}

/** 获取 NewAPI 预置脚本模板 */
export async function getNewApiScriptTemplate(): Promise<string> {
  return invoke<string>("get_newapi_script_template");
}

/** 测试自定义供应商脚本（保存前预演） */
export async function testCustomProviderScript(
  code: string,
  apiKey: string,
  baseUrl: string | null,
  allowHttp: boolean
): Promise<string> {
  return invoke<string>("test_custom_provider_script", {
    code,
    apiKey,
    baseUrl,
    allowHttp,
  });
}
```

- [ ] **Step 2: 运行 tsc**

Run: `cd D:/Project/PeekaUsage && npx tsc --noEmit 2>&1 | tail -5`

- [ ] **Step 3: 暂不提交（与 Task 12 一起）**

---

## Task 12: AppSelect 加分组模式 + ProviderIcon 支持新图标

**Files:**
- Modify: `src/components/common/AppSelect.tsx`
- Modify: `src/components/common/ProviderIcon.tsx`
- Create: `src/assets/provider-icons/deepseek.svg`
- Create: `src/assets/provider-icons/newapi.svg`
- Create: `src/assets/provider-icons/custom.svg`

- [ ] **Step 1: 给 AppSelect 加 grouped 模式**

在 `src/components/common/AppSelect.tsx` 中，扩展 props 支持 `options` 带分组：

```typescript
export interface AppSelectOption {
  value: string;
  label: string;
  icon?: string;        // 供应商图标名
  description?: string; // 简短说明
  badge?: string;       // "订阅" / "余额" / "网关"
}

export interface AppSelectGroup {
  label: string;        // 分组标题，如"官方订阅"
  options: AppSelectOption[];
}

interface AppSelectProps {
  value: string;
  onChange: (value: string) => void;
  options?: AppSelectOption[];        // 扁平模式
  groups?: AppSelectGroup[];          // 分组模式（与 options 互斥）
  placeholder?: string;
  // ... 其它现有 props
}
```

渲染分组时，在 `AppSelect` 的浮层里按 `groups` 遍历，每组前加一个分组标题分隔符（不可点击的 `<div class="app-select-group-title">`）。

- [ ] **Step 2: 扩展 ProviderIcon 支持新图标**

在 `src/components/common/ProviderIcon.tsx` 中，确保支持 `deepseek` / `newapi` / `custom` 图标名，找不到时 fallback 到 `custom.svg`。

- [ ] **Step 3: 创建三个 SVG 图标**

创建 `src/assets/provider-icons/deepseek.svg`、`newapi.svg`、`custom.svg`。

图标内容：简单的供应商 logo（可从官方品牌资源获取，或用占位 SVG）。

`custom.svg` 示例（通用自定义图标）：

```svg
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
  <circle cx="12" cy="12" r="3"/>
  <path d="M12 1v6m0 10v6m4.22-13.22l4.24-4.24M5.54 18.46l4.24-4.24M1 12h6m10 0h6M18.46 5.54l-4.24 4.24M5.54 5.54l4.24 4.24"/>
</svg>
```

DeepSeek 和 NewAPI 用各自品牌色和 logo。

- [ ] **Step 4: 运行 tsc + 暂不提交**

Run: `cd D:/Project/PeekaUsage && npx tsc --noEmit 2>&1 | tail -5`

---

## Task 13: 自定义供应商向导组件

**Files:**
- Create: `src/components/settings/ProviderWizardDialog.tsx`

- [ ] **Step 1: 创建 ProviderWizardDialog 组件**

创建 `src/components/settings/ProviderWizardDialog.tsx`，3 步向导，用 React portal 挂 body，遵循 ConfirmDialog 的样式约定：

```typescript
import { useState } from "react";
import { createPortal } from "react-dom";
import type { CustomProviderConfig, AuthSchemeConfig, QueryTypeConfig } from "../../types/provider";
import { AppSelect } from "../common/AppSelect";
import { getNewApiScriptTemplate, testCustomProviderScript } from "../../utils/ipc";

interface ProviderWizardDialogProps {
  open: boolean;
  onClose: () => void;
  onConfirm: (config: CustomProviderConfig) => void;
}

export function ProviderWizardDialog({ open, onClose, onConfirm }: ProviderWizardDialogProps) {
  const [step, setStep] = useState(1);
  const [displayName, setDisplayName] = useState("");
  const [authScheme, setAuthScheme] = useState<AuthSchemeConfig>("bearer");
  const [baseUrl, setBaseUrl] = useState("");
  const [queryType, setQueryType] = useState<QueryTypeConfig>("script");
  const [scriptCode, setScriptCode] = useState("");
  const [allowHttp, setAllowHttp] = useState(false);
  const [envKeyName, setEnvKeyName] = useState("");
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<string | null>(null);

  if (!open) return null;

  // 预填 NewAPI 脚本
  const fillNewApiTemplate = async () => {
    const code = await getNewApiScriptTemplate();
    setScriptCode(code);
    setBaseUrl(""); // 用户填自己的网关地址
    setQueryType("script");
  };

  const handleTest = async () => {
    setTesting(true);
    setTestResult(null);
    try {
      const result = await testCustomProviderScript(scriptCode, "test-key", baseUrl, allowHttp);
      setTestResult(`✓ ${result}`);
    } catch (e) {
      setTestResult(`✗ ${e}`);
    } finally {
      setTesting(false);
    }
  };

  const handleConfirm = () => {
    const config: CustomProviderConfig = {
      displayName,
      baseUrl,
      authScheme,
      envKeyName: envKeyName || null,
      icon: null,
      queryType,
      script: queryType === "script" ? { code: scriptCode, language: "javascript", timeoutMs: 15000 } : undefined,
      allowHttp,
    };
    onConfirm(config);
    // 重置
    setStep(1);
    setDisplayName("");
    setBaseUrl("");
    setScriptCode("");
  };

  // ... 渲染 3 步 UI，每步有"上一步"/"下一步"按钮，第 3 步有"测试"和"确认"按钮
  // 用 createPortal(document.body) 挂载，样式参考 ConfirmDialog
}
```

完整组件实现包含 3 步 UI 渲染、表单校验、测试按钮反馈。样式用 `src/assets/styles/settings.css` 的类名。

- [ ] **Step 2: 运行 tsc**

Run: `cd D:/Project/PeekaUsage && npx tsc --noEmit 2>&1 | tail -5`

- [ ] **Step 3: 暂不提交（与 Task 14-15 一起）**

---

## Task 14: 改造 ProviderConfig 和 SettingsPanel

**Files:**
- Modify: `src/components/settings/ProviderConfig.tsx`
- Modify: `src/components/settings/SettingsPanel.tsx`

- [ ] **Step 1: SettingsPanel 新增供应商下拉接 getProviderTemplates，分组渲染**

在 `SettingsPanel.tsx` 中，"新增供应商"下拉的数据源从硬编码改为调用 `getProviderTemplates()`，并构造分组：

```typescript
const [templates, setTemplates] = useState<ProviderTemplate[]>([]);

useEffect(() => {
  getProviderTemplates().then(setTemplates);
}, []);

// 构造分组
const groups: AppSelectGroup[] = [
  {
    label: "官方订阅",
    options: templates
      .filter(t => t.capabilities.hasSubscription)
      .map(t => ({ value: t.id, label: t.displayName, icon: t.icon, badge: "订阅" })),
  },
  {
    label: "余额查询",
    options: templates
      .filter(t => !t.capabilities.hasSubscription && t.capabilities.hasBalance)
      .map(t => ({ value: t.id, label: t.displayName, icon: t.icon, badge: "余额" })),
  },
  {
    label: "中转网关",
    options: templates
      .filter(t => t.queries.some(q => q.queryType.kind === "script"))
      .map(t => ({ value: t.id, label: t.displayName, icon: t.icon, badge: "网关" })),
  },
  {
    label: "自定义",
    options: [{ value: "__custom__", label: "自定义供应商", icon: "custom", badge: "自定义" }],
  },
];
```

选 `__custom__` 时打开 `ProviderWizardDialog`。

- [ ] **Step 2: ProviderConfig 卡片字段按 queryType 动态显隐**

在 `ProviderConfig.tsx` 中，根据 `configItem.customConfig` 或 `configItem.providerTemplateId` 判断 queryType，动态显示不同字段：

- Balance 类（deepseek/openrouter）：显示 API Key 输入框 + 切换环境
- Script 类（newapi/自定义）：显示 Base URL + Access Token + User ID + 脚本编辑器
- Subscription 类（anthropic/openai）：显示 OAuth Token 区（含自动检测 + 获取方式）

- [ ] **Step 3: 运行 tsc + 暂不提交**

Run: `cd D:/Project/PeekaUsage && npx tsc --noEmit 2>&1 | tail -5`

---

## Task 15: 改造 ProviderCard 和 WidgetContainer，加 i18n

**Files:**
- Modify: `src/components/widget/ProviderCard.tsx`
- Modify: `src/components/widget/WidgetContainer.tsx`
- Modify: `src/i18n/messages.ts`

- [ ] **Step 1: 在 messages.ts 加 windowLabels 映射和新供应商名称**

在 `src/i18n/messages.ts` 中追加：

```typescript
// 订阅窗口标签 i18n 映射
windowLabels: {
  "five_hour": { "zh-Hans": "5 小时", "zh-Hant": "5 小時", "en": "5h" },
  "seven_day": { "zh-Hans": "7 天", "zh-Hant": "7 天", "en": "7d" },
  "seven_day_sonnet": { "zh-Hans": "7 天 Sonnet", "zh-Hant": "7 天 Sonnet", "en": "7d Sonnet" },
  "seven_day_opus": { "zh-Hans": "7 天 Opus", "zh-Hant": "7 天 Opus", "en": "7d Opus" },
  "weekly_limit": { "zh-Hans": "周限额", "zh-Hant": "週限額", "en": "Weekly" },
  "monthly": { "zh-Hans": "月度", "zh-Hant": "月度", "en": "Monthly" },
},

// 新供应商名称
providerNames: {
  deepseek: { "zh-Hans": "DeepSeek", "zh-Hant": "DeepSeek", "en": "DeepSeek" },
  newapi: { "zh-Hans": "NewAPI", "zh-Hant": "NewAPI", "en": "NewAPI" },
  custom: { "zh-Hans": "自定义供应商", "zh-Hant": "自訂供應商", "en": "Custom Provider" },
},

// 自定义供应商向导文案
wizard: {
  step1Title: { "zh-Hans": "基本信息", "zh-Hant": "基本資訊", "en": "Basic Info" },
  step2Title: { "zh-Hans": "查询配置", "zh-Hant": "查詢設定", "en": "Query Config" },
  step3Title: { "zh-Hans": "高级", "zh-Hant": "進階", "en": "Advanced" },
  displayName: { "zh-Hans": "显示名称", "zh-Hant": "顯示名稱", "en": "Display Name" },
  baseUrl: { "zh-Hans": "Base URL", "zh-Hant": "Base URL", "en": "Base URL" },
  authScheme: { "zh-Hans": "认证方式", "zh-Hant": "認證方式", "en": "Auth Scheme" },
  queryType: { "zh-Hans": "查询类型", "zh-Hant": "查詢類型", "en": "Query Type" },
  script: { "zh-Hans": "脚本代码", "zh-Hant": "腳本代碼", "en": "Script Code" },
  allowHttp: { "zh-Hans": "允许 HTTP", "zh-Hant": "允許 HTTP", "en": "Allow HTTP" },
  envKeyName: { "zh-Hans": "环境变量名（可选）", "zh-Hant": "環境變數名（可選）", "en": "Env Var Name (optional)" },
  test: { "zh-Hans": "测试", "zh-Hant": "測試", "en": "Test" },
  confirm: { "zh-Hans": "确认创建", "zh-Hant": "確認建立", "en": "Confirm" },
  next: { "zh-Hans": "下一步", "zh-Hant": "下一步", "en": "Next" },
  prev: { "zh-Hans": "上一步", "zh-Hant": "上一步", "en": "Previous" },
},
```

- [ ] **Step 2: ProviderCard 适配 window label i18n 映射**

在 `ProviderCard.tsx` 中，渲染 `SubscriptionWindow.label` 时，先查 i18n 映射表，找不到时原样显示：

```typescript
const getWindowLabel = (label: string): string => {
  const messages = windowLabels[label];
  if (messages && messages[currentLanguage]) {
    return messages[currentLanguage];
  }
  return label; // 未知窗口原样显示
};
```

- [ ] **Step 3: WidgetContainer 适配 string ProviderId**

`WidgetContainer.tsx` 中用到 `ProviderId` 类型的地方，现在都是 `string`，无需改逻辑。拖拽排序的 `saveProviderOrder` 调用不变。

- [ ] **Step 4: 运行 tsc + cargo check 全量验证**

Run:
```bash
cd D:/Project/PeekaUsage && npx tsc --noEmit 2>&1 | tail -5
cd src-tauri && cargo fmt --all --check 2>&1 | tail -5
cargo check 2>&1 | tail -5
```

Expected: 全部通过，0 错误。

- [ ] **Step 5: 提交前端改造**

```bash
cd D:/Project/PeekaUsage
git add src/ src/assets/provider-icons/
git commit -m "feat(frontend): 适配配置驱动架构 + 自定义供应商向导 + i18n

- ProviderId 改为 string
- AppSelect 加分组模式
- ProviderIcon 支持 deepseek/newapi/custom
- 新增 ProviderWizardDialog 3 步向导
- ProviderConfig 字段按 queryType 动态显隐
- SettingsPanel 下拉接 getProviderTemplates 分组渲染
- ProviderCard 适配 Balance/Script 展示，window label i18n
- messages.ts 加 windowLabels / providerNames / wizard 文案"
```

---

## Task 16: 更新 AGENTS.md 和 CLAUDE.md 文档

**Files:**
- Modify: `AGENTS.md`
- Modify: `CLAUDE.md`

- [ ] **Step 1: 在 AGENTS.md 追加第 23 章**

在 `AGENTS.md` 的"## 23." 位置（如果有；否则追加到文档末尾合适章节后）加：

```markdown
### 23. 供应商架构已改为配置驱动 + 自定义供应商

文件：

- `src-tauri/src/providers/registry.rs`
- `src-tauri/src/providers/balance.rs`
- `src-tauri/src/providers/script_engine.rs`
- `src-tauri/src/providers/types.rs`
- `src-tauri/src/providers/mod.rs`
- `src-tauri/src/commands/provider_commands.rs`
- `src-tauri/src/config/app_config.rs`
- `src-tauri/src/config/system_env.rs`
- `src/types/provider.ts`
- `src/components/settings/ProviderWizardDialog.tsx`
- `src/components/settings/ProviderConfig.tsx`
- `src/components/settings/SettingsPanel.tsx`
- `src/components/widget/ProviderCard.tsx`
- `src/components/common/AppSelect.tsx`
- `src/i18n/messages.ts`

当前要求：

- `ProviderId` 已从枚举改为 `String`，新增供应商不再需要改枚举和 7 处 match
- 内置供应商模板统一在 `registry.rs` 注册，新增供应商只需追加一条 `ProviderTemplate`
- `ProviderManager` 通过 `registry::get()` 路由查询，不再硬编码 `match provider_id`
- 查询分四类：Balance（余额）/ CodingPlan（百分比，阶段 2）/ Subscription（OAuth）/ Script（JS 脚本）
- JS 脚本引擎使用 rquickjs 沙箱，安全边界：HTTPS 强制（可配置允许 HTTP）、同源校验、无网络/文件 API、超时上限 60s
- 自定义供应商通过 `ProviderWizardDialog` 3 步向导创建，配置存 `customConfig` 字段
- NewAPI 用 accessToken + userId（存 KeyStore），不是普通 API Key
- 订阅窗口 label 改为机器常量（`five_hour` / `seven_day` 等），前端通过 i18n 映射
- 错误处理区分瞬时（RequestError/RateLimited，保留旧值重试）和确定性（AuthError/ParseError，立即透出）
- HTTP 读体统一改为 `bytes().await` + `serde_json::from_slice`（bytes-then-parse 模式）
- 缓存策略：失败快照不写入，保留上次成功值
- 旧配置缺少 `providerTemplateId` / `customConfig` 字段时按 `providerId` 在 registry 兜底
- 阶段 1 已实现：OpenAI / Anthropic / OpenRouter 迁移 + DeepSeek + NewAPI + 自定义供应商
- 阶段 2 待实现：Kimi / GLM / MiniMax / ZenMux（CodingPlan）+ OAuth 自动检测 + Claude seven_day_opus
- 阶段 3 待实现：SiliconFlow / StepFun / Novita + 火山方舟 AK/SK + Gemini OAuth
```

- [ ] **Step 2: 同步 CLAUDE.md**

在 `CLAUDE.md` 中追加相同章节内容。

- [ ] **Step 3: 提交文档**

```bash
cd D:/Project/PeekaUsage
git add AGENTS.md CLAUDE.md
git commit -m "docs: 更新 AGENTS.md 和 CLAUDE.md，加第 23 章供应商配置驱动架构"
```

---

## Task 17: 最终验证与端到端测试

- [ ] **Step 1: 全量类型检查和编译**

Run:
```bash
cd D:/Project/PeekaUsage
npx tsc --noEmit
cd src-tauri
cargo fmt --all --check
cargo check
```

Expected: 全部通过，0 错误。

- [ ] **Step 2: 启动开发环境，手动验证向后兼容**

Run: `cd D:/Project/PeekaUsage && npm run tauri dev`

验证清单：
- [ ] 应用正常启动，无 panic
- [ ] 现有 OpenAI / Anthropic / OpenRouter 卡片显示正常
- [ ] 手动刷新三家供应商，数据正确返回
- [ ] 拖拽排序正常，顺序持久化
- [ ] 透明度滑杆、开机自启、刷新间隔设置不受影响

- [ ] **Step 3: 验证新增供应商**

- [ ] 设置页"新增供应商"下拉显示 5 项 + 自定义，分组显示
- [ ] 选 DeepSeek，填 API Key，主界面卡片显示余额
- [ ] 选 NewAPI，填 Base URL + Access Token + User ID，主界面卡片显示配额
- [ ] 选"自定义供应商"，向导 3 步走通
- [ ] 自定义供应商脚本能执行（用 NewAPI 预置脚本测试）
- [ ] HTTPS 强制生效（填 HTTP 地址默认拒绝）
- [ ] 自定义供应商能拖拽排序

- [ ] **Step 4: 验证错误处理**

- [ ] 填错误的 API Key（401），立即透出鉴权错误
- [ ] 断网刷新，保留上次成功值（不显示错误覆盖）
- [ ] 脚本返回 `isValid: false`，立即透出错误信息

- [ ] **Step 5: 验证迁移一致性**

- [ ] OpenAI 复合查询（credit_grants -> costs -> subscription）依次尝试，返回值和改造前一致
- [ ] Anthropic OAuth 订阅查询正常，窗口显示正确
- [ ] OpenRouter credits / key 回退链路正常

- [ ] **Step 6: 推送全部改动**

```bash
cd D:/Project/PeekaUsage
git push
```

- [ ] **Step 7: 确认 CI 构建**

如果 GitHub Actions 配置了 ci.yml，确认 Linux / macOS 构建仍然通过（rquickjs 跨平台预编译）。

---

## 完成标准

阶段 1 完成的标志：
1. 所有类型检查和编译通过（`tsc --noEmit` + `cargo fmt --all --check` + `cargo check`）
2. 向后兼容：旧 config.json 启动不丢失配置
3. 新增 5 家供应商全部可查（OpenAI/Anthropic/OpenRouter 迁移 + DeepSeek/NewAPI 新增）
4. 自定义供应商向导走通，JS 脚本能执行
5. 错误二分生效（瞬时保留旧值，确定性立即透出）
6. 文档更新（AGENTS.md + CLAUDE.md 第 23 章）
7. 全部提交并推送
