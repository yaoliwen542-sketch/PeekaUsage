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

/// 供应商能力
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCapabilities {
    pub has_balance: bool,
    pub has_usage: bool,
    pub has_rate_limit: bool,
    pub has_subscription: bool,
}

/// 用量数据（按量 API）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageData {
    pub total_used: f64,
    pub total_budget: Option<f64>,
    pub remaining: Option<f64>,
    pub currency: String,
    pub period_start: Option<String>,
    pub period_end: Option<String>,
}

/// 单个 API Key 的用量摘要
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeyUsageSummary {
    pub key_id: String,
    pub key_name: String,
    pub color: String,
    pub status: ProviderStatus,
    pub usage: Option<UsageData>,
    pub rate_limit: Option<RateLimitData>,
    pub error_message: Option<String>,
}

/// 订阅用量窗口
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionWindow {
    /// 窗口名称（如 "5小时"、"7天"、"主窗口"、"次窗口"）
    pub label: String,
    /// 利用率百分比 (0-100)
    pub utilization: f64,
    /// 重置时间 (ISO 8601)
    pub resets_at: Option<String>,
}

/// 额外用量（Extra Usage）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtraUsage {
    /// 是否已启用
    pub is_enabled: bool,
    /// 月度上限（美元），None 表示无限额
    pub monthly_limit_usd: Option<f64>,
    /// 本月已用（美元）
    pub used_usd: Option<f64>,
    /// 利用率百分比 (0-100)
    pub utilization: Option<f64>,
    /// 重置时间（月初，ISO 8601）
    pub resets_at: Option<String>,
}

/// 订阅用量数据
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionUsage {
    /// 订阅计划名称（如 "Pro", "Max", "Plus"）
    pub plan_name: Option<String>,
    /// 各个限制窗口
    pub windows: Vec<SubscriptionWindow>,
    /// 额外用量（仅 Anthropic）
    pub extra_usage: Option<ExtraUsage>,
    /// 状态
    pub status: ProviderStatus,
    /// 错误信息
    pub error_message: Option<String>,
}

/// 单个命名订阅的用量摘要
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionUsageSummary {
    pub subscription_id: String,
    pub subscription_name: String,
    pub color: String,
    pub source: Option<String>,
    pub usage: SubscriptionUsage,
}

/// 速率限制数据
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RateLimitData {
    pub requests_per_minute: Option<u64>,
    pub requests_per_minute_limit: Option<u64>,
    pub tokens_per_minute: Option<u64>,
    pub tokens_per_minute_limit: Option<u64>,
}

/// 供应商状态
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderStatus {
    #[serde(rename = "idle")]
    Idle,
    #[serde(rename = "loading")]
    Loading,
    #[serde(rename = "success")]
    Success,
    #[serde(rename = "error")]
    Error,
}

/// 供应商用量摘要（返回给前端）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSummary {
    pub provider_id: ProviderId,
    pub display_name: String,
    pub enabled: bool,
    pub status: ProviderStatus,
    #[serde(default)]
    pub api_key_usages: Vec<ApiKeyUsageSummary>,
    pub usage: Option<UsageData>,
    #[serde(default)]
    pub subscriptions: Vec<SubscriptionUsageSummary>,
    pub rate_limit: Option<RateLimitData>,
    pub last_updated: Option<String>,
    pub error_message: Option<String>,
}

/// 供应商的命名 API Key（前端传入）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderApiKeyInput {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub color: String,
    pub value: String,
}

/// 供应商配置（从前端传入）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConfig {
    pub provider_id: ProviderId,
    pub enabled: bool,
    #[serde(default)]
    pub api_keys: Vec<ProviderApiKeyInput>,
    #[serde(default)]
    pub subscriptions: Vec<ProviderSubscriptionInput>,
    // 新增：内置供应商模板 ID（自定义供应商为 None）
    #[serde(default)]
    pub provider_template_id: Option<String>,
    // 新增：自定义供应商配置（内置供应商为 None）
    #[serde(default)]
    pub custom_config: Option<CustomProviderConfig>,
}

/// 返回前端的 API Key 配置项
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderApiKeyItem {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub color: String,
    pub value: String,
    #[serde(default)]
    pub is_active_in_environment: bool,
}

/// 命名订阅配置项
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSubscriptionInput {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub color: String,
    pub oauth_token: String,
    #[serde(default)]
    pub source: Option<String>,
}

/// 返回前端的订阅配置项
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSubscriptionItem {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub color: String,
    pub oauth_token: String,
    #[serde(default)]
    pub source: Option<String>,
}

/// 供应商配置项（返回给前端，不含完整 Key）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConfigItem {
    pub provider_id: ProviderId,
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
    // 新增：内置供应商模板 ID（自定义供应商为 None）
    #[serde(default)]
    pub provider_template_id: Option<String>,
    // 新增：自定义供应商配置（内置供应商为 None）
    #[serde(default)]
    pub custom_config: Option<CustomProviderConfig>,
}

// ====================================================================
// 以下为配置驱动架构新增的类型（Task 2）
// ====================================================================

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
    CodingPlan { provider: String },
    /// OAuth 订阅查询（如 Claude、Codex、Gemini）
    Subscription { provider: String },
    /// JS 脚本查询（NewAPI、自定义）
    Script { default_template: Option<String> },
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
    /// NewAPI 等 Script 模板需要的访问令牌（阶段 1 临时方案：随 customConfig 明文存储，
    /// 阶段 2 迁移到 KeyStore 加密存储）。脚本模板通过 {{accessToken}} 占位符引用。
    #[serde(default)]
    pub access_token: Option<String>,
    /// NewAPI 等 Script 模板需要的用户 ID（阶段 1 临时方案：随 customConfig 明文存储，
    /// 阶段 2 迁移到 KeyStore 加密存储）。脚本模板通过 {{userId}} 占位符引用。
    #[serde(default)]
    pub user_id: Option<String>,
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
