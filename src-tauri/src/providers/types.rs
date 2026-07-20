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
    /// 火山方舟 AFP 每日窗口
    pub const DAILY: &str = "daily";
    /// OpenAI 主窗口（wham primary_window）在无法按时长归类时的兜底标签
    pub const PRIMARY: &str = "primary";
    /// OpenAI 次窗口（wham secondary_window）在无法按时长归类时的兜底标签
    pub const SECONDARY: &str = "secondary";
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
    /// 分窗口利用率（Coding Plan 类供应商的 5 小时 / 周限额等窗口）
    /// 空数组表示无分窗口数据，前端回退显示单条总进度条
    #[serde(default)]
    pub windows: Vec<SubscriptionWindow>,
    /// 套餐标注（如火山方舟 "Agent Plan · Medium"），无套餐概念时为 None
    #[serde(default)]
    pub plan_name: Option<String>,
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
    /// 窗口标签：优先使用 window_labels 模块的机器常量（如 five_hour / seven_day /
    /// primary），前端经 windowLabels 映射成各语言文案；
    /// 无法归类的来源（如 Gemini 未知 modelId）可能直接是原始标识字符串
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
///
/// serde 说明：variant 名保持 snake_case（"balance" / "coding_plan" / ...），
/// variant 内容字段用 camelCase（fieldMap / defaultTemplate），与
/// src/types/provider.ts 的 QueryType 联合类型对齐（修复 L11 的类型不一致）。
/// 该类型只在内存与 IPC 传输中使用（模板由 builtin_templates 代码构造，
/// 不落盘），改 serde 名不影响存量配置数据。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
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
    /// 提取值乘以此系数（如 Novita 的 0.0001，将万分之一美元换算为美元）
    ///
    /// 应用范围：total / used / remaining 三个字段均会乘以 scale。
    /// 默认 None 表示不换算（DeepSeek/OpenRouter 等模板不受影响）。
    #[serde(default)]
    pub scale: Option<f64>,
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
    /// NewAPI 等 Script 模板需要的访问令牌。脚本模板通过 {{accessToken}} 占位符引用。
    ///
    /// 阶段 2 起不再明文落盘：保存时由后端写入 KeyStore、本字段持久化为 None；
    /// 读取配置时后端回填掩码值供前端回显（查询时从 KeyStore 取真实值）。
    /// 旧配置里残留的明文会在下次读取配置时自动迁移到 KeyStore。
    #[serde(default)]
    pub access_token: Option<String>,
    /// NewAPI 等 Script 模板需要的用户 ID。脚本模板通过 {{userId}} 占位符引用。
    /// 存储/迁移语义与 access_token 相同（阶段 2 起存 KeyStore）。
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 修复 L11：QueryType 的 serde 名必须与 src/types/provider.ts 的
    /// QueryType 联合类型对齐——variant 名 snake_case、内容字段 camelCase。
    #[test]
    fn test_query_type_serde_names_align_with_ts() {
        let balance = QueryType::Balance {
            url: "https://example.com".to_string(),
            auth: AuthScheme::Bearer,
            field_map: BalanceFieldMap {
                total: "$.total".to_string(),
                used: None,
                remaining: None,
                currency: "USD".to_string(),
                scale: None,
            },
        };
        let json = serde_json::to_value(&balance).unwrap();
        assert_eq!(json["kind"], "balance");
        assert!(json.get("fieldMap").is_some());
        assert!(json.get("field_map").is_none());

        let coding_plan = QueryType::CodingPlan {
            provider: "kimi".to_string(),
        };
        let json = serde_json::to_value(&coding_plan).unwrap();
        assert_eq!(json["kind"], "coding_plan");

        let script = QueryType::Script {
            default_template: Some("code".to_string()),
        };
        let json = serde_json::to_value(&script).unwrap();
        assert_eq!(json["kind"], "script");
        assert_eq!(json["defaultTemplate"], "code");
        assert!(json.get("default_template").is_none());
    }
}
