# 多供应商支持与配置驱动架构改造设计

- 日期：2026-07-17
- 作者：brainstorming 会话
- 状态：待实现（阶段 1）
- 范围：本设计覆盖三个阶段，实现计划（writing-plans）先只做阶段 1

## 背景与动机

PeekaUsage 当前后端 provider 架构基于 `ProviderId` 枚举（OpenAI / Anthropic / OpenRouter）+ 7 处分散的硬编码 `match`：

- `src-tauri/src/providers/types.rs` 的 `ProviderId` enum 及 `as_str` / `env_key_name` / `env_oauth_token_name` 方法
- `src-tauri/src/providers/mod.rs::fetch_subscription_usage` 的 `match provider_id`
- `src-tauri/src/commands/provider_commands.rs::save_provider_order` 的 `matches!` 过滤、`parse_provider_id`
- `src-tauri/src/config/app_config.rs::is_supported_provider_id` / `provider_rank` / `default_provider_card_expanded`
- `src-tauri/src/config/system_env.rs::supported_provider_ids` / `parse_provider_id`
- 前端 `src/types/provider.ts` 的 `ProviderId` 联合类型

这种架构每新增一家供应商要改 7+ 处代码，与"支持大量供应商 + 自定义"的需求本质冲突。参考 cc-switch（farion1231/cc-switch）的成熟实现，决定改造为"配置驱动 provider registry + 查询模板 + JS 脚本兜底"的开放架构。

## 目标供应商清单（全量 14 家 + 自定义）

| 阶段 | 供应商 | 查询类型 | 接口 |
|---|---|---|---|
| 1 | OpenAI（迁移） | Subscription + Balance | `chatgpt.com/backend-api/wham/usage` + `api.openai.com/v1/dashboard/billing/*` |
| 1 | Anthropic（迁移） | Subscription + Balance | `api.anthropic.com/api/oauth/usage` + `api.anthropic.com/v1/organizations/cost_report` |
| 1 | OpenRouter（迁移） | Balance | `openrouter.ai/api/v1/credits` + `openrouter.ai/api/v1/key` |
| 1 | DeepSeek | Balance | `api.deepseek.com/user/balance` |
| 1 | NewAPI | Script | `{baseUrl}/api/user/self`（预置 JS 脚本） |
| 2 | Kimi | CodingPlan | `api.kimi.com/coding/v1/usages` |
| 2 | GLM / GLM Team | CodingPlan | `open.bigmodel.cn/api/monitor/usage/quota/limit` |
| 2 | MiniMax | CodingPlan | `api.minimaxi.com/v1/api/openplatform/coding_plan/remains` |
| 2 | ZenMux | CodingPlan | 用户填 base_url |
| 3 | SiliconFlow | Balance | `api.siliconflow.cn/v1/user/info` |
| 3 | StepFun | Balance | `api.stepfun.com/v1/accounts` |
| 3 | Novita | Balance | `api.novita.ai/v3/user/balance` |
| 3 | 火山方舟 | CodingPlan（AK/SK 签名） | `open.volcengineapi.com` |
| 3 | Gemini | Subscription | `cloudcode-pa.googleapis.com` |
| - | 自定义供应商 | Balance / Script | 用户配置 |

## 设计决策（已与用户确认）

| 维度 | 决策 |
|---|---|
| 自定义深度 | JS 脚本引擎兜底（rquickjs + `{request, extractor}` 契约） |
| 内置供应商范围 | 全量 14 家 |
| 架构改造 | 重构为配置驱动（ProviderId 从枚举改字符串，去掉硬编码 match） |
| 设置页呈现 | 预置 14 家 + 自定义按钮 |
| OAuth 凭据 | 手动填 + 自动检测本地文件并存 |
| 落地策略 | 方案 B：渐进式，分三阶段 |

---

## 第 1 节：总体架构与分阶段

### 核心改造：ProviderId 从枚举改为字符串

```rust
// 旧（types.rs:4-13）
pub enum ProviderId { OpenAI, Anthropic, OpenRouter }

// 新
pub type ProviderId = String;  // "openai", "anthropic", "deepseek", "custom_xxx"
```

`ProviderId` 在 Rust 端变成 `String`，前端 TS 也从联合类型 `"openai" | "anthropic" | "openrouter"` 改为 `string`。所有 `match provider_id` 分支被一张可扩展的 Provider Registry 取代。

### Provider Registry：配置驱动的供应商注册表

新增 `src-tauri/src/providers/registry.rs`：

```rust
pub struct ProviderTemplate {
    pub id: &'static str,                    // "openai", "kimi", ...
    pub display_name: &'static str,          // "OpenAI"
    pub env_key_name: &'static str,          // "OPENAI_API_KEY"
    pub env_oauth_token_name: Option<&'static str>,
    pub queries: Vec<QuerySpec>,             // 一个 provider 可有多条查询路径
    pub capabilities: ProviderCapabilities,
    pub icon: &'static str,                  // 对应前端 provider-icons 文件名
    pub docs_url: Option<&'static str>,      // "获取方式"按钮跳转
    pub oauth_detect: Option<OAuthDetectConfig>,  // 自动检测配置（阶段 2 填充）
}

pub struct QuerySpec {
    pub query_type: QueryType,
    pub base_url: Option<&'static str>,   // 覆盖默认 base url
}

pub enum QueryType {
    Balance { url: &'static str, auth: AuthScheme, field_map: BalanceFieldMap },
    CodingPlan { provider: CodingPlanProvider },        // 阶段 2
    Subscription { provider: SubscriptionProvider },
    Script { default_template: Option<&'static str> },  // NewAPI/custom 预置脚本
}

pub enum AuthScheme {
    Bearer,
    XApiKey,           // Anthropic
    RawKey,            // GLM（无 Bearer 前缀）
    Custom(Vec<(&'static str, &'static str)>),
}

pub struct BalanceFieldMap {
    pub total: JsonPath,
    pub used: Option<JsonPath>,
    pub remaining: Option<JsonPath>,
    pub currency: &'static str,
}

pub type JsonPath = &'static str;
```

新增供应商 = 在 registry 加一行 `ProviderTemplate`，不再改任何 `match`。

### 分阶段策略

| 阶段 | 内容 | 可发版 |
|---|---|---|
| 阶段 1 | 架构改造 + 迁移现有 3 家 + NewAPI/DeepSeek + JS 引擎基础 + 自定义供应商 UI | ✅ |
| 阶段 2 | Kimi/GLM/MiniMax/ZenMux（CodingPlan）+ OAuth 自动检测 + Claude `seven_day_opus` + ChatGPT-Account-Id | ✅ |
| 阶段 3 | SiliconFlow/StepFun/Novita + 火山方舟 AK/SK + Gemini OAuth + refresh | ✅ |

本设计文档覆盖三阶段完整设计，实现计划先只做阶段 1。

### 兼容性

- 旧 `config.json` 的 `providers` HashMap key 仍是字符串，无需迁移
- 现有 OpenAI/Anthropic/OpenRouter 的密钥、订阅、顺序配置完全保留
- 旧 `ProviderId` 枚举的方法搬到 registry，外部 API 不变
- 前端 `ProviderId` 改 `string` 后编译期全量报错，逐个修，无运行时风险

---

## 第 2 节：数据模型与配置结构

### 2.1 后端 Rust 类型（`src-tauri/src/providers/types.rs`）

**ProviderId 改为字符串：**

```rust
pub type ProviderId = String;
```

`as_str()` / `env_key_name()` / `env_oauth_token_name()` 方法删除，改由 registry 查询。

**UsageData 扩展（兼容货币型和百分比型）：**

```rust
pub struct UsageData {
    pub total_used: f64,              // 已用（货币=USD，百分比=utilization）
    pub total_budget: Option<f64>,    // 总额（货币=余额上限，百分比=100）
    pub remaining: Option<f64>,       // 剩余
    pub currency: String,             // "USD" / "CNY" / "%" / "credits"
    pub period_start: Option<String>,
    pub period_end: Option<String>,
}
```

- Balance 类：`currency="USD"`，`total_budget` = 账户总额，`remaining` = 余额
- CodingPlan/Subscription 类：`currency="%"`，`total_budget=100`，`total_used` = utilization

**SubscriptionWindow label 改为机器可枚举常量：**

```rust
pub const WIDGET_FIVE_HOUR: &str = "five_hour";
pub const WIDGET_SEVEN_DAY: &str = "seven_day";
pub const WIDGET_SEVEN_DAY_SONNET: &str = "seven_day_sonnet";
pub const WIDGET_SEVEN_DAY_OPUS: &str = "seven_day_opus";   // 新增
pub const WIDGET_WEEKLY_LIMIT: &str = "weekly_limit";
pub const WIDGET_MONTHLY: &str = "monthly";
```

`SubscriptionWindow.label` 存常量，前端通过 i18n 表翻译。同时遍历未知窗口，新窗口名原样透出。

### 2.2 阶段 1 registry 内容（5 家）

| id | query_type | 说明 |
|---|---|---|
| `openai` | Balance × 3 + Subscription × 1 | 迁移现有，复合型 |
| `anthropic` | Balance × 1 + Subscription × 1 | 迁移现有 |
| `openrouter` | Balance × 2 | 迁移现有，回退链路 |
| `deepseek` | Balance × 1 | 新增 |
| `newapi` | Script × 1 | 新增，预置脚本 |

OpenAI 复合型通过 `queries: Vec<QuerySpec>` 多条路径依次尝试解决。

### 2.3 配置文件结构（`config.json`）

旧字段全部保留，新增 `provider_template_id` 和 `custom_config` 两个字段：

```jsonc
{
  "providers": {
    "openai": {
      "providerId": "openai",
      "enabled": true,
      "apiKeys": [...],
      "subscriptions": [...],
      "activeApiKeyId": "key-1",
      "manageApiKeyEnvironment": false,
      "providerTemplateId": "openai",       // 新增
      "customConfig": null                   // 新增
    },
    "custom_abc123": {
      "providerId": "custom_abc123",
      "enabled": true,
      "apiKeys": [...],
      "providerTemplateId": null,
      "customConfig": {
        "displayName": "我的中转站",
        "baseUrl": "https://my-gateway.com",
        "authScheme": "bearer",
        "envKeyName": "MY_GATEWAY_KEY",
        "icon": "custom",
        "queryType": "script",
        "script": {
          "code": "({request:{...}, extractor:...})",
          "language": "javascript",
          "timeoutMs": 15000
        }
        // 注意：NewAPI 的 accessToken / userId 不存 config.json
        // 存 KeyStore（{provider_id}::access_token / {provider_id}::user_id）
      }
    }
  },
  "providerOrder": ["openai", "custom_abc123", "anthropic"]
}
```

规则：
- `providerTemplateId` 非空 = 内置模板，配置走 registry
- `customConfig` 非空 = 自定义供应商，配置完全自包含
- 二者互斥
- 旧配置缺这两个字段时，按 `providerId` 在 registry 查找兜底

### 2.4 ProviderEntry 改造

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
    pub custom_config: Option<CustomProviderConfig>,
}

pub struct CustomProviderConfig {
    pub display_name: String,
    pub base_url: String,
    #[serde(default)]
    pub auth_scheme: AuthSchemeConfig,
    #[serde(default)]
    pub env_key_name: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    pub query_type: QueryTypeConfig,        // balance / script
    #[serde(default)]
    pub script: Option<ScriptConfig>,
    #[serde(default)]
    pub allow_http: bool,                   // 默认 false
}

pub struct ScriptConfig {
    pub code: String,
    #[serde(default = "default_language")]
    pub language: String,                  // 当前只支持 "javascript"
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
}
```

`access_token` / `user_id`（NewAPI 用）不存 config.json，存 KeyStore（`{provider_id}::access_token`、`{provider_id}::user_id`）。

### 2.5 硬编码移除

`is_supported_provider_id` / `provider_rank` / `save_provider_order` 过滤 / `supported_provider_ids` / `parse_provider_id` 全部改为查 registry 或基于"是否在 registry/customConfig 里"判定。

### 2.6 前端 TS 类型

```typescript
export type ProviderId = string;

export interface ProviderConfigItem {
  providerId: string;
  displayName: string;
  enabled: boolean;
  apiKeys: ProviderApiKeyItem[];
  subscriptions: ProviderSubscriptionItem[];
  capabilities: ProviderCapabilities;
  environmentVariableName: string;
  activeApiKeyId: string | null;
  providerTemplateId: string | null;        // 新增
  customConfig: CustomProviderConfig | null;  // 新增
  isCustom: boolean;                          // 便捷字段
}

export interface CustomProviderConfig {
  displayName: string;
  baseUrl: string;
  authScheme: "bearer" | "x_api_key" | "raw_key";
  envKeyName?: string;
  icon?: string;
  queryType: "balance" | "script";
  script?: ScriptConfig;
  allowHttp: boolean;
}

export interface ScriptConfig {
  code: string;
  language: "javascript";
  timeoutMs: number;
}
```

---

## 第 3 节：查询链路与 JS 引擎

### 3.1 查询链路总览

`ProviderManager` 不再用 `match provider_id` 路由，改为从 registry 取 `ProviderTemplate`，按 `queries` 列表依次执行：

```rust
pub async fn fetch_api_usage(&self, provider_id: &str, api_key: &str)
    -> Result<(UsageData, Option<RateLimitData>), ProviderError>
{
    let template = self.resolve_template(provider_id)?;
    for spec in template.queries.iter().filter(|q| q.is_usage_query()) {
        match self.execute_query(spec, api_key).await {
            Ok(data) => return Ok((data, self.fetch_rate_limits(template, api_key).await?)),
            Err(ProviderError::AuthError(_)) => return Err(...),
            Err(_) => continue,
        }
    }
    Err(ProviderError::RequestError("所有查询路径都失败".into()))
}
```

关键：路由变成"查 registry -> 遍历 queries -> 按 QueryType 分发"，删除所有 `match provider_id`。

### 3.2 QueryType 分发

```rust
async fn execute_query(&self, spec: &QuerySpec, api_key: &str)
    -> Result<UsageData, ProviderError>
{
    match &spec.query_type {
        QueryType::Balance { url, auth, field_map } => {
            self.execute_balance_query(url, auth, field_map, api_key).await
        }
        QueryType::CodingPlan { provider } => coding_plan::fetch(provider, api_key).await,
        QueryType::Subscription { .. } => Err(...),  // 在 fetch_subscription_usage 处理
        QueryType::Script { template_code } => {
            script_engine::run(template_code, api_key, spec.base_url.as_deref()).await
        }
    }
}
```

### 3.3 Balance 查询模板

阶段 1 实现 DeepSeek 验证模板机制：

```rust
ProviderTemplate {
    id: "deepseek",
    display_name: "DeepSeek",
    env_key_name: "DEEPSEEK_API_KEY",
    queries: vec![QuerySpec {
        query_type: QueryType::Balance {
            url: "https://api.deepseek.com/user/balance",
            auth: AuthScheme::Bearer,
            field_map: BalanceFieldMap {
                total: "$.balance_infos[0].total_balance",
                used: None,
                remaining: "$.balance_infos[0].total_balance",
                currency: "USD",
            }
        }
    }],
}
```

`execute_balance_query` 流程：
1. 用 url + auth 构造 reqwest 请求
2. 发请求（15s 超时，**先 `resp.bytes().await` 再 `serde_json::from_slice`**）
3. 用 `jsonpath-rust` 按 `field_map` 提取字段
4. 组装 `UsageData` 返回

依赖：`jsonpath-rust`（纯 Rust，无 unsafe，+50KB 编译产物）。

### 3.4 JS 脚本引擎

依赖：`rquickjs`（纯 Rust 嵌入 QuickJS，+2MB 编译产物，跨平台）。

模块：`src-tauri/src/providers/script_engine.rs`。

**脚本契约：**

```javascript
({
  request: {
    url: "{{baseUrl}}/api/user/self",
    method: "GET",
    headers: { "Authorization": "Bearer {{apiKey}}", ... },
    body: null
  },
  extractor: function(response) {
    // response = { status, success, data, headers }
    if (response.success && response.data) {
      return {
        planName: "Pro",
        total: 100, used: 30, remaining: 70,
        currency: "USD",
        windows: [{ label: "five_hour", utilization: 30, resetsAt: "..." }]
      };
    }
    return { isValid: false, invalidMessage: "查询失败" };
  }
})
```

**`script_engine::run` 流程：**

1. 模板变量替换：`{{apiKey}}` / `{{baseUrl}}` / `{{accessToken}}` / `{{userId}}`
2. 安全沙箱初始化：创建 QuickJS runtime，注入受限全局对象（只有 `JSON` / `Math` / `Date`，没有 `fetch` / `setTimeout` / 文件 IO）
3. 执行 extractor 的 request 部分，拿到 `{url, method, headers, body}`
4. Rust 端发请求（脚本不能自己发，统一由 Rust 走 reqwest）
5. 把响应喂回 extractor：`response = {status, success, data, headers}`，`data` 是解析后的 JSON
6. 执行 extractor 函数，拿到结果，转换成 `UsageData` 或 `SubscriptionUsage`

**安全边界（必须实现）：**

- HTTPS 强制：内置模板允许 HTTP；纯自定义脚本默认强制 HTTPS，用户需勾选"允许 HTTP"才能用内网地址
- 同源校验：请求 URL 的 host:port 必须和 `baseUrl` 一致
- 超时：默认 15s，用户可配 `timeoutMs`（上限 60s）
- 无网络 API：脚本不能直接发请求
- 无文件/进程访问：QuickJS 沙箱不注入这些 API

**NewAPI 预置脚本：**

```javascript
({
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
})
```

NewAPI 用 accessToken + userId（系统登录令牌，不是推理 key），`CustomProviderConfig` 扩展 `access_token` / `user_id` 字段，存入 KeyStore。

### 3.5 现有三家迁移

**OpenAI（复合型）：** `fetch_usage` 三条逻辑（credit_grants -> costs -> subscription）拆成三个 Balance 模板；`fetch_subscription` 搬到 `subscription.rs::fetch_openai_wham`。

**Anthropic：** `cost_report` 拆 Balance 模板（x-api-key 认证）；OAuth 搬到 `subscription.rs::fetch_anthropic_oauth`。

**OpenRouter：** `credits` / `key` 两条拆成 Balance 模板。

### 3.6 错误处理改进

引入瞬时失败 vs 确定性失败二分：

```rust
impl ProviderError {
    pub fn is_transient(&self) -> bool {
        matches!(self, ProviderError::RequestError(_) | ProviderError::RateLimited(_))
    }
}
```

前端 `useProviders.ts` 对接：
- `is_transient() = true`：保留上次成功值，触发 retry
- `is_transient() = false`：立即透出错误状态

所有 HTTP 读体改为 `resp.bytes().await` + `serde_json::from_slice`。

### 3.7 缓存策略

沿用"失败快照不写入"原则：
- `fetch_api_usage` 返回 `Ok` 才写缓存
- 返回 `Err`（瞬时）不写缓存、不 emit 事件，保留旧值
- 返回 `Ok(成功=false)` 写错误状态缓存，立即透出

---

## 第 4 节：前端交互与设置页改造

### 4.1 新增供应商入口

`AppSelect` 下拉项扩展为 14 家内置 + 1 个自定义，分组显示：

```
── 官方订阅 ──
  Claude (Anthropic) / OpenAI / Codex / Gemini
── 国内 Coding Plan ──
  Kimi / GLM / MiniMax / ZenMux / 火山方舟
── 余额查询 ──
  DeepSeek / OpenRouter / SiliconFlow / StepFun / Novita
── 中转网关 ──
  NewAPI / 自定义供应商
```

`AppSelect` 加 `grouped` 模式（分组标题分隔符）。

### 4.2 供应商配置卡片字段动态显隐

| 字段 | Balance | Script (NewAPI) | Subscription | Custom |
|---|---|---|---|---|
| API Key | ✅ | ✅ | ❌ | ✅ |
| OAuth Token | ❌ | ❌ | ✅ | ❌ |
| Base URL | ❌ | ✅ | ❌ | ✅ |
| Access Token (NewAPI) | ❌ | ✅ | ❌ | ✅ |
| User ID (NewAPI) | ❌ | ✅ | ❌ | ✅ |
| 查询模板/脚本 | ❌ | ✅ | ❌ | ✅ |
| 切换环境 | ✅ | ❌ | ❌ | ❌ |
| 多 Key | ✅ | ❌ | ✅ | ✅ |

卡片根据 `capabilities` + `queryType` 动态渲染，配置驱动。

### 4.3 自定义供应商向导

新建 `src/components/settings/ProviderWizardDialog.tsx`，3 步：

1. 基本信息：显示名称、图标选择、认证方式
2. 查询配置：查询类型（Balance/Script）+ Base URL + 字段映射 或 脚本代码
3. 高级：自定义环境变量名、允许 HTTP 开关

NewAPI 模板走快捷预设，跳过向导。

### 4.4 主界面卡片展示

- Balance 类：detailed 显示"已用/总额 + 进度条 + 余额"；compact 显示 `[icon] DeepSeek ━━━━░░ 70% $3.50/$5.00`
- CodingPlan/Subscription：多窗口展示；compact 每窗口一行
- Script 类：和 Balance 一致，若返回 windows 则按 Subscription 展示

**window.label i18n 映射：**

```typescript
windowLabels: {
  "five_hour": { "zh-Hans": "5 小时", "zh-Hant": "5 小時", "en": "5h" },
  "seven_day": { "zh-Hans": "7 天", "zh-Hant": "7 天", "en": "7d" },
  "seven_day_sonnet": { "zh-Hans": "7 天 Sonnet", "zh-Hant": "7 天 Sonnet", "en": "7d Sonnet" },
  "seven_day_opus": { "zh-Hans": "7 天 Opus", "zh-Hant": "7 天 Opus", "en": "7d Opus" },
  "weekly_limit": { "zh-Hans": "周限额", "zh-Hant": "週限額", "en": "Weekly" },
  "monthly": { "zh-Hans": "月度", "zh-Hant": "月度", "en": "Monthly" },
}
```

未知 label 原样显示。

### 4.5 供应商图标

`src/assets/provider-icons/` 新增：
- 阶段 1：`deepseek.*` / `newapi.*` / `custom.*`
- 阶段 2：`kimi.*` / `glm.*` / `minimax.*` / `zenmux.*`
- 阶段 3：`siliconflow.*` / `stepfun.*` / `novita.*` / `volcengine.*` / `gemini.*`

`ProviderIcon.tsx` 逻辑不变，找不到时 fallback 到 `custom.*`。

### 4.6 拖拽排序

`saveProviderOrder` 命令过滤改为查 registry，自定义供应商（`custom_xxx`）也能拖拽排序。

### 4.7 IPC 命令新增

```typescript
getProviderTemplates(): Promise<ProviderTemplate[]>
testCustomProviderScript(config: CustomProviderConfig): Promise<TestResult>
getNewApiScriptTemplate(): Promise<string>
```

### 4.8 前端类型同步

`src/types/provider.ts` 和 `src-tauri/src/providers/types.rs` 同步（AGENTS.md 核心约束）。

---

## 第 5 节：阶段 1 详细范围与验证清单

### 5.1 阶段 1 交付物

#### Rust 后端

| 文件 | 改动 |
|---|---|
| `src-tauri/src/providers/types.rs` | 改：ProviderId 改 String，新增 QueryType/AuthScheme/BalanceFieldMap/ScriptConfig/CustomProviderConfig，新增窗口常量，ProviderError 加 is_transient |
| `src-tauri/src/providers/registry.rs` | 新建：ProviderTemplate + 5 家注册表 |
| `src-tauri/src/providers/mod.rs` | 改：删除硬编码，改为查 registry 路由 |
| `src-tauri/src/providers/balance.rs` | 新建：execute_balance_query 通用实现 |
| `src-tauri/src/providers/script_engine.rs` | 新建：rquickjs 沙箱 + run + 安全边界 |
| `src-tauri/src/providers/openai.rs` | 改：三条逻辑拆 Balance 模板 |
| `src-tauri/src/providers/anthropic.rs` | 改：cost_report 拆模板，OAuth 搬走 |
| `src-tauri/src/providers/openrouter.rs` | 改：credits/key 拆模板 |
| `src-tauri/src/providers/subscription.rs` | 改：重命名 fetch 函数，由 execute_subscription 调度 |
| `src-tauri/src/commands/provider_commands.rs` | 改：parse_provider_id 删除，save_provider_order 改，新增 3 个命令，save_provider_config 支持 customConfig |
| `src-tauri/src/config/app_config.rs` | 改：ProviderEntry 加字段，is_supported_provider_id 改 |
| `src-tauri/src/config/system_env.rs` | 改：supported_provider_ids 改，sync 支持 customConfig |
| `src-tauri/src/lib.rs` | 改：注册新命令 |
| `src-tauri/Cargo.toml` | 改：加 rquickjs、jsonpath-rust |

#### 前端

| 文件 | 改动 |
|---|---|
| `src/types/provider.ts` | 改：ProviderId 改 string，新增类型 |
| `src/utils/ipc.ts` | 改：新增 3 个 IPC |
| `src/components/common/AppSelect.tsx` | 改：加 grouped 模式 |
| `src/components/common/ProviderIcon.tsx` | 改：支持新图标 |
| `src/components/settings/ProviderConfig.tsx` | 改：字段动态显隐，NewAPI 字段 |
| `src/components/settings/ProviderWizardDialog.tsx` | 新建：自定义向导 |
| `src/components/settings/SettingsPanel.tsx` | 改：下拉接 getProviderTemplates，分组渲染 |
| `src/components/widget/ProviderCard.tsx` | 改：适配 Balance/Script 展示，window label i18n |
| `src/components/widget/WidgetContainer.tsx` | 改：适配 string ProviderId |
| `src/assets/provider-icons/` | 加：deepseek/newapi/custom |
| `src/i18n/messages.ts` | 改：加 windowLabels + 新供应商名称 + 向导文案 |

#### 文档

| 文件 | 改动 |
|---|---|
| `AGENTS.md` | 加新章节"23. 供应商架构改为配置驱动 + 自定义供应商" |
| `CLAUDE.md` | 同步 |

### 5.2 阶段 1 范围边界

**包含：** 架构改造 + 迁移 3 家 + DeepSeek + NewAPI + JS 引擎 + 自定义向导 + window label i18n + 错误二分 + 缓存策略

**不包含（留阶段 2/3）：** Kimi/GLM/MiniMax/ZenMux、SiliconFlow/StepFun/Novita、火山方舟 AK/SK、Gemini OAuth、OAuth 自动检测、Claude seven_day_opus、ChatGPT-Account-Id

`ProviderTemplate` 结构预留 `oauth_detect` 字段（`Option<OAuthDetectConfig>`），阶段 2 填充。

### 5.3 依赖

| 依赖 | 用途 | 编译产物影响 |
|---|---|---|
| `rquickjs` | JS 脚本引擎 | +2MB |
| `jsonpath-rust` | Balance 字段提取 | +50KB |

跨平台预编译，无平台风险，CI 不改。

### 5.4 验证清单

#### 类型检查
- `npx tsc --noEmit` 通过
- `cargo fmt --all --check` 通过
- `cargo check` 通过

#### 向后兼容
- 旧 config.json 启动配置不丢失
- OpenAI/Anthropic/OpenRouter 卡片显示正常，能刷新
- 拖拽排序保持
- 透明度、开机自启、刷新间隔不受影响

#### 新架构验证
- 新增供应商下拉显示 5 项 + 自定义，分组显示
- DeepSeek 填 key 后显示余额
- NewAPI 填 baseUrl/accessToken/userId 后显示配额
- 自定义供应商向导 3 步走通
- 自定义脚本能执行（用 NewAPI 预置脚本测试）
- HTTPS 强制生效（HTTP 默认拒绝）
- 超时生效
- 自定义供应商能拖拽排序

#### 迁移验证
- OpenAI 复合查询依次尝试，逻辑和旧版一致
- Anthropic OAuth 订阅查询正常
- OpenRouter credits/key 回退链路正常

#### 错误处理
- 鉴权失败立即透出，不重试
- 网络错误保留上次成功值
- 脚本 isValid:false 立即透出

#### 跨平台
- Windows NSIS 打包通过
- Linux deb/AppImage 打包通过（CI）
- macOS app/dmg 打包通过（CI）

### 5.5 阶段 2/3 方向

**阶段 2：**
- CodingPlan 模板（`src-tauri/src/providers/coding_plan.rs`）
- Kimi / GLM（含 team）/ MiniMax / ZenMux 接入 registry
- OAuthDetectConfig 实现：读取 `~/.claude/.credentials.json` / `~/.codex/auth.json` / macOS Keychain
- 补 Claude `seven_day_opus` 窗口 + 遍历未知窗口
- ChatGPT 请求补 `ChatGPT-Account-Id` header

**阶段 3：**
- SiliconFlow / StepFun / Novita（Balance 模板）
- 火山方舟：`sigv4.rs` 实现 AK/SK 签名
- Gemini：`gemini.rs` 实现 OAuth + refresh_token

### 5.6 风险与对策

| 风险 | 对策 |
|---|---|
| rquickjs 跨平台编译 | 阶段 1 优先 Windows 验证；CI 加 Linux/macOS 构建检查 |
| 枚举改字符串后前端类型散落 | TS 编译期全量报错逐个修；不放过 `as ProviderId` 断言 |
| OpenAI 复合查询迁移逻辑漂移 | 用真实 key 对比旧版返回值，三项数据逐一核对 |
| JS 脚本安全漏洞 | 严格按 3.4 安全边界实现；阶段 1 不开放脚本分享；考虑加 CSP |
| 配置迁移破坏旧用户 | 新字段都 `#[serde(default)]`；旧 provider 在 registry 找不到时按 providerId 兜底 |

---

## 参考实现

- cc-switch（farion1231/cc-switch）：provider 抽象、JS 脚本引擎、错误二分、缓存策略、OAuth 凭据读取
- 关键源码：`src-tauri/src/commands/provider.rs`、`src-tauri/src/services/balance.rs`、`src-tauri/src/services/subscription.rs`、`src-tauri/src/usage_script.rs`
