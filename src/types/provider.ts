/** 供应商 ID（配置驱动，不再是固定枚举） */
export type ProviderId = string;

/** 精简模式标记色 */
export const PROVIDER_MARKER_COLORS = [
  "#3b82f6",
  "#10b981",
  "#f59e0b",
  "#ef4444",
  "#8b5cf6",
  "#06b6d4",
  "#ec4899",
  "#84cc16",
] as const;

export function normalizeProviderMarkerColor(color: string | null | undefined, index: number): string {
  if (color && PROVIDER_MARKER_COLORS.includes(color as (typeof PROVIDER_MARKER_COLORS)[number])) {
    return color;
  }

  return PROVIDER_MARKER_COLORS[index % PROVIDER_MARKER_COLORS.length];
}

/** 供应商能力 */
export interface ProviderCapabilities {
  hasBalance: boolean;
  hasUsage: boolean;
  hasRateLimit: boolean;
  hasSubscription: boolean;
}

/** 用量数据（按量 API） */
export interface UsageData {
  totalUsed: number;
  totalBudget: number | null;
  remaining: number | null;
  currency: string;
  periodStart: string | null;
  periodEnd: string | null;
  /** 分窗口利用率（Coding Plan 类供应商的 5 小时 / 周限额等），空数组表示无 */
  windows?: SubscriptionWindow[];
  /** 套餐标注（如火山方舟 "Agent Plan · Medium"），无套餐概念时为 null/缺省 */
  planName?: string | null;
}

/** 订阅用量窗口 */
export interface SubscriptionWindow {
  label: string;
  utilization: number;
  resetsAt: string | null;
}

/** 额外用量（Extra Usage，仅 Anthropic） */
export interface ExtraUsage {
  isEnabled: boolean;
  monthlyLimitUsd: number | null;
  usedUsd: number | null;
  /** 利用率百分比 0-100 */
  utilization: number | null;
  resetsAt: string | null;
}

/** 订阅用量数据 */
export interface SubscriptionUsage {
  planName: string | null;
  windows: SubscriptionWindow[];
  extraUsage: ExtraUsage | null;
  status: ProviderStatus;
  errorMessage: string | null;
}

export interface SubscriptionUsageSummary {
  subscriptionId: string;
  subscriptionName: string;
  color: string;
  source: string | null;
  usage: SubscriptionUsage;
}

/** 速率限制数据 */
export interface RateLimitData {
  requestsPerMinute: number | null;
  requestsPerMinuteLimit: number | null;
  tokensPerMinute: number | null;
  tokensPerMinuteLimit: number | null;
}

/** 供应商状态 */
export type ProviderStatus = "idle" | "loading" | "success" | "error";

/** 单个 API Key 的用量摘要 */
export interface ApiKeyUsageSummary {
  keyId: string;
  keyName: string;
  color: string;
  status: ProviderStatus;
  usage: UsageData | null;
  rateLimit: RateLimitData | null;
  errorMessage: string | null;
}

/** 供应商用量摘要（从后端返回） */
export interface UsageSummary {
  providerId: ProviderId;
  displayName: string;
  enabled: boolean;
  status: ProviderStatus;
  apiKeyUsages: ApiKeyUsageSummary[];
  usage: UsageData | null;
  subscriptions: SubscriptionUsageSummary[];
  rateLimit: RateLimitData | null;
  lastUpdated: string | null;
  errorMessage: string | null;
}

/** 命名 API Key 配置 */
export interface ProviderApiKeyItem {
  id: string;
  name: string;
  color: string;
  value: string;
  isActiveInEnvironment: boolean;
}

export interface ProviderSubscriptionItem {
  id: string;
  name: string;
  color: string;
  oauthToken: string;
  source: string | null;
}

/** 供应商配置（前端用） */
export interface ProviderConfigItem {
  providerId: ProviderId;
  displayName: string;
  enabled: boolean;
  apiKeys: ProviderApiKeyItem[];
  subscriptions: ProviderSubscriptionItem[];
  capabilities: ProviderCapabilities;
  environmentVariableName: string;
  activeApiKeyId: string | null;
  // 新增：内置供应商模板 ID（自定义供应商为 null）
  providerTemplateId: string | null;
  // 新增：自定义供应商配置（内置供应商为 null）
  customConfig: CustomProviderConfig | null;
}

/** 前端传给后端的 API Key 输入 */
export interface ProviderApiKeyInput {
  id: string;
  name: string;
  color: string;
  value: string;
}

/** 前端传给后端的订阅输入 */
export interface ProviderSubscriptionInput {
  id: string;
  name: string;
  color: string;
  oauthToken: string;
  source: string | null;
}

/** 供应商配置（前端传给后端） */
export interface ProviderConfig {
  providerId: ProviderId;
  enabled: boolean;
  apiKeys: ProviderApiKeyInput[];
  subscriptions: ProviderSubscriptionInput[];
  providerTemplateId?: string | null;
  customConfig?: CustomProviderConfig | null;
}

// ====================================================================
// 以下为配置驱动架构新增类型（与 src-tauri/src/providers/types.rs 同步）
// ====================================================================

/** 查询类型：决定如何获取供应商用量（与 Rust QueryType 对齐，serde tag=kind） */
export type QueryType =
  | { kind: "balance"; url: string; auth: AuthScheme; fieldMap: BalanceFieldMap }
  | { kind: "coding_plan"; provider: string }
  | { kind: "subscription"; provider: string }
  | { kind: "script"; defaultTemplate: string | null };

/** 认证方案（运行期，可能含自定义 header 集合） */
export type AuthScheme =
  | "bearer"
  | "x_api_key"
  | "raw_key"
  | { custom: Array<[string, string]> };

/** Balance 查询的字段映射（JSONPath） */
export interface BalanceFieldMap {
  total: string;
  used?: string | null;
  remaining?: string | null;
  currency: string;
  /** 提取值乘以此系数（如 Novita 的 0.0001，将万分之一美元换算为美元）
   * 应用范围：total / used / remaining 三个字段均会乘以 scale。
   * 默认 null 表示不换算。 */
  scale?: number | null;
}

/** 单条查询规格 */
export interface QuerySpec {
  queryType: QueryType;
  /** 覆盖默认 base url（自定义供应商用） */
  baseUrl?: string | null;
}

/** 内置供应商模板（注册表条目） */
export interface ProviderTemplate {
  /** 供应商 ID，如 "openai" */
  id: string;
  /** 显示名称，如 "OpenAI" */
  displayName: string;
  /** 按量 API Key 对应的环境变量名，如 "OPENAI_API_KEY" */
  envKeyName: string;
  /** 订阅 OAuth Token 对应的环境变量名（可选） */
  envOauthTokenName?: string | null;
  /** 查询规格列表（一个供应商可有多条查询路径） */
  queries: QuerySpec[];
  /** 供应商能力 */
  capabilities: ProviderCapabilities;
  /** 图标名（对应 src/assets/provider-icons/ 下的文件名，不含扩展名） */
  icon: string;
  /** "获取方式"按钮跳转的官方文档 URL */
  docsUrl?: string | null;
  /** OAuth 凭据自动检测配置（阶段 2 填充，阶段 1 预留） */
  oauthDetect?: OAuthDetectConfig | null;
}

/** OAuth 凭据自动检测配置（阶段 2 实现，阶段 1 仅占位） */
export interface OAuthDetectConfig {
  /** 凭据文件路径（如 "~/.codex/auth.json"） */
  filePath: string;
  /** 文件内 token 的 JSONPath */
  tokenPath: string;
  /** macOS Keychain service 名（可选） */
  keychainService?: string | null;
}

/** 认证方案（配置层，前端可序列化） */
export type AuthSchemeConfig = "bearer" | "x_api_key" | "raw_key";

/** 查询类型（配置层，前端可序列化） */
export type QueryTypeConfig = "balance" | "script";

/** JS 脚本配置 */
export interface ScriptConfig {
  /** 脚本代码（返回 {request, extractor} 的 JS 表达式） */
  code: string;
  language: string;
  timeoutMs: number;
}

/** 自定义供应商配置（存于 config.json 的 customConfig 字段） */
export interface CustomProviderConfig {
  displayName: string;
  baseUrl: string;
  authScheme: AuthSchemeConfig;
  /** 自定义环境变量名（不填则不接管环境变量） */
  envKeyName?: string | null;
  icon?: string | null;
  queryType: QueryTypeConfig;
  script?: ScriptConfig;
  /** 是否允许 HTTP（默认 false，强制 HTTPS） */
  allowHttp: boolean;
  /** NewAPI 等 Script 模板需要的访问令牌（阶段 1 临时方案：随 customConfig 明文存储，
   * 阶段 2 迁移到 KeyStore 加密存储）。脚本模板通过 {{accessToken}} 占位符引用。 */
  accessToken?: string | null;
  /** NewAPI 等 Script 模板需要的用户 ID（阶段 1 临时方案：随 customConfig 明文存储，
   * 阶段 2 迁移到 KeyStore 加密存储）。脚本模板通过 {{userId}} 占位符引用。 */
  userId?: string | null;
}
