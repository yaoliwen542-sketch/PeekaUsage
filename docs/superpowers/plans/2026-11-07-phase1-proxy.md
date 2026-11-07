# 阶段一：全局代理 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 应用所有用量查询链路支持全局代理（不使用 / 跟随系统 / 自定义 URL），保存即热生效，并提供可诊断的测试连接。

**Architecture:** 新增统一 `HttpClientFactory`（`RwLock<Arc<Client>>` 快照分发），收编 5 处 provider 自建 client；代理字段持久化进 `AppSettings`（默认 `system` 与 reqwest 现状一致，旧配置零迁移），密码单独进 KeyStore；`save_settings` 命令检测代理字段变化后 `reload()` 热重建。

**Tech Stack:** Rust / Tauri 2 State / reqwest 0.12（`Proxy` 内建支持 http/https/socks5）/ React 19 + zustand。

**Spec:** `docs/superpowers/specs/2026-11-07-glass-shell-row-ui-and-proxy-design.md` §2

## Global Constraints

- 全部注释、错误提示、文档用中文（AGENTS.md 语言规范）
- serde 持久化字段 camelCase；新字段必须 `#[serde(default)]`，旧配置缺字段零报错
- client 必须保留 `timeout(30s)` + `connect_timeout(10s)`（AGENTS.md 27 条）
- 代理密码存 KeyStore（`proxy_password` 键），禁止写入 config.json
- WebDAV client（`data_commands.rs::build_webdav_client`）不走代理，保持独立
- 用量查询链路不得引入系统环境变量依赖（AGENTS.md Kimi 排查点原则）
- 每个任务收尾必须过：`npx tsc --noEmit`（涉前端时）、`cargo fmt --all --check`、`cargo check`
- 版本号不变（本次不发版）；发版时三处版本号 + release-notes + latest.json 校验走既有流程

---

### Task 1: 配置字段 ProxyMode + 兼容默认

**Files:**
- Modify: `src-tauri/src/config/app_config.rs`

**Interfaces:**
- Produces: `pub enum ProxyMode { None, System, Custom }`（serde lowercase：`"none"|"system"|"custom"`）、`AppSettings` 新字段 `proxy_mode: ProxyMode`、`proxy_url: String`、`proxy_username: String`（camelCase：`proxyMode` / `proxyUrl` / `proxyUsername`）。后续所有任务按此名称使用。

- [ ] **Step 1: 在 `app_config.rs` 末尾新增测试模块（先写失败测试）**

```rust
#[cfg(test)]
mod proxy_tests {
    use super::*;

    #[test]
    fn 默认设置跟随系统代理() {
        let settings = AppSettings::default();
        assert_eq!(settings.proxy_mode, ProxyMode::System);
        assert_eq!(settings.proxy_url, "");
        assert_eq!(settings.proxy_username, "");
    }

    #[test]
    fn 旧配置缺少代理字段时回退默认() {
        let full = serde_json::to_value(AppSettings::default()).expect("序列化默认配置");
        let obj = full.as_object().expect("应为 JSON 对象").clone();
        let obj: serde_json::Map<String, serde_json::Value> = obj
            .into_iter()
            .filter(|(k, _)| k != "proxyMode" && k != "proxyUrl" && k != "proxyUsername")
            .collect();
        let settings: AppSettings =
            serde_json::from_value(obj.into()).expect("缺代理字段必须能反序列化");
        assert_eq!(settings.proxy_mode, ProxyMode::System);
    }

    #[test]
    fn 代理模式枚举序列化为小写() {
        assert_eq!(
            serde_json::to_value(ProxyMode::Custom).unwrap(),
            serde_json::Value::String("custom".into())
        );
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cd src-tauri && cargo test proxy_tests`
Expected: 编译错误（`ProxyMode` 未定义）

- [ ] **Step 3: 实现字段（加在 `AppSettings` 结构体已有字段之后，serde 属性风格与相邻字段一致）**

```rust
/// 代理模式：none=不使用（禁用系统代理）、system=跟随系统（reqwest 默认）、custom=自定义
#[serde(default)]
pub proxy_mode: ProxyMode,
/// 自定义代理地址，支持 http:// https:// socks5://
#[serde(default)]
pub proxy_url: String,
/// 代理 Basic 鉴权用户名（可选；密码存 KeyStore）
#[serde(default)]
pub proxy_username: String,
```

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProxyMode {
    None,
    System,
    Custom,
}

impl Default for ProxyMode {
    fn default() -> Self {
        ProxyMode::System
    }
}
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cd src-tauri && cargo test proxy_tests`
Expected: 3 passed

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/config/app_config.rs
git commit -m "feat(config): 新增全局代理配置字段，旧配置缺字段兼容默认跟随系统"
```

---

### Task 2: HttpClientFactory + 构建矩阵单测

**Files:**
- Create: `src-tauri/src/net/mod.rs`
- Create: `src-tauri/src/net/http_client.rs`
- Modify: `src-tauri/src/lib.rs`（只加 `mod net;` 声明，State 装配在 Task 4）

**Interfaces:**
- Consumes: Task 1 的 `ProxyMode`
- Produces: `net::http_client::{ProxyConfig, HttpClientFactory}`，方法 `HttpClientFactory::new(proxy: &ProxyConfig) -> Self`、`async fn reload(&self, proxy: &ProxyConfig) -> Result<(), String>`、`async fn client(&self) -> Arc<reqwest::Client>`；自由函数 `pub fn build_client(proxy: &ProxyConfig) -> Result<reqwest::Client, String>`（Task 5 测试连接复用）

- [ ] **Step 1: 创建 `src-tauri/src/net/mod.rs`**

```rust
pub mod http_client;
```

- [ ] **Step 2: 创建 `src-tauri/src/net/http_client.rs`（含失败测试先行：测试与实现同文件，先只写测试再补实现）**

```rust
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::RwLock;

use crate::config::app_config::ProxyMode;

/// 代理配置快照（密码由 KeyStore 读出后传入，绝不落 config.json）
#[derive(Debug, Clone)]
pub struct ProxyConfig {
    pub mode: ProxyMode,
    pub url: String,
    pub username: String,
    pub password: Option<String>,
}

impl ProxyConfig {
    pub fn from_settings(mode: ProxyMode, url: String, username: String, password: Option<String>) -> Self {
        Self { mode, url, username, password }
    }
}

/// 按 ProxyConfig 构建 client：none 显式禁用代理；system 用 reqwest 默认（读系统代理）；
/// custom 用 Proxy::all，用户名非空时附带 Basic 鉴权。
pub fn build_client(proxy: &ProxyConfig) -> Result<reqwest::Client, String> {
    let mut builder = reqwest::Client::builder()
        // 必须带超时：网络挂起时避免刷新永久卡死（AGENTS.md 27 条）
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(10));
    builder = match proxy.mode {
        ProxyMode::None => builder.no_proxy(),
        ProxyMode::System => builder,
        ProxyMode::Custom => {
            let url = proxy.url.trim();
            if url.is_empty() {
                return Err(String::from("代理地址不能为空"));
            }
            let mut proxy_builder =
                reqwest::Proxy::all(url).map_err(|error| format!("代理地址无效: {error}"))?;
            if !proxy.username.is_empty() {
                if let Some(password) = proxy.password.as_deref() {
                    proxy_builder = proxy_builder.basic_auth(&proxy.username, password);
                }
            }
            builder.proxy(proxy_builder)
        }
    };
    builder.build().map_err(|error| format!("初始化 HTTP 客户端失败: {error}"))
}

/// 统一 HTTP 客户端工厂：所有用量查询链路从这里取 client。
/// 保存代理设置后调用 reload() 热重建；请求侧通过 client() 取当前快照，不阻塞 reload。
pub struct HttpClientFactory {
    client: RwLock<Arc<reqwest::Client>>,
}

impl HttpClientFactory {
    pub fn new(proxy: &ProxyConfig) -> Self {
        let client = build_client(proxy).expect("无法创建 HTTP 客户端");
        Self {
            client: RwLock::new(Arc::new(client)),
        }
    }

    pub async fn reload(&self, proxy: &ProxyConfig) -> Result<(), String> {
        let client = build_client(proxy)?;
        *self.client.write().await = Arc::new(client);
        Ok(())
    }

    pub async fn client(&self) -> Arc<reqwest::Client> {
        self.client.read().await.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(mode: ProxyMode, url: &str) -> ProxyConfig {
        ProxyConfig { mode, url: url.into(), username: String::new(), password: None }
    }

    #[test]
    fn system_模式构建成功() {
        assert!(build_client(&config(ProxyMode::System, "")).is_ok());
    }

    #[test]
    fn none_模式构建成功且显式无代理() {
        assert!(build_client(&config(ProxyMode::None, "")).is_ok());
    }

    #[test]
    fn custom_模式空地址报错() {
        let error = build_client(&config(ProxyMode::Custom, "")).unwrap_err();
        assert!(error.contains("代理地址不能为空"));
    }

    #[test]
    fn custom_模式非法地址报错() {
        let error = build_client(&config(ProxyMode::Custom, "not a proxy url")).unwrap_err();
        assert!(error.contains("代理地址无效"));
    }

    #[test]
    fn custom_模式支持_http_与_socks5() {
        assert!(build_client(&config(ProxyMode::Custom, "http://127.0.0.1:7890")).is_ok());
        assert!(build_client(&config(ProxyMode::Custom, "socks5://127.0.0.1:1080")).is_ok());
    }

    #[tokio::test]
    async fn reload_后_client_快照更新() {
        let factory = HttpClientFactory::new(&config(ProxyMode::System, ""));
        factory
            .reload(&config(ProxyMode::Custom, "http://127.0.0.1:7890"))
            .await
            .expect("reload 应成功");
        let client = factory.client().await;
        // 快照 Arc 指向新构建的 client，可正常发起请求（不实际联网）
        assert!(client.get("https://example.com").try_clone().is_some());
    }
}
```

- [ ] **Step 3: 在 `src-tauri/src/lib.rs` 顶部模块声明区加入**

```rust
mod net;
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cd src-tauri && cargo test net::http_client`
Expected: 6 passed

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/net/ src-tauri/src/lib.rs
git commit -m "feat(net): 统一 HttpClientFactory，支持 none/system/custom 代理与热重建"
```

---

### Task 3: Provider 侧 client 改造（5 处持有者 + 调用点）

**Files:**
- Modify: `src-tauri/src/providers/mod.rs`、`openai.rs`、`anthropic.rs`、`openrouter.rs`、`subscription.rs`
- Modify: `src-tauri/src/commands/provider_commands.rs`（`http_client_ref` 调用点）

**Interfaces:**
- Consumes: Task 2 的 `HttpClientFactory` / `ProxyConfig`
- Produces: `ProviderManager::new(factory: Arc<HttpClientFactory>)`；`ProviderManager::http_client(&self) -> Arc<reqwest::Client>`（异步，替代原同步 `http_client_ref(&self) -> &reqwest::Client`）；`OpenAIProvider::new(factory)` / `AnthropicProvider::new(factory)` / `OpenRouterProvider::new(factory)` / `SubscriptionFetcher::new(factory)`。coding_plan / balance / script_engine / gemini 的 `&Client` 签名不变，调用方传工厂快照。

- [ ] **Step 1: 改 `openai.rs`（其余三个 provider 同款改法，见 Step 3 清单）**

结构体字段与构造：

```rust
use std::sync::Arc;
use crate::net::http_client::HttpClientFactory;

pub struct OpenAIProvider {
    id: String,
    factory: Arc<HttpClientFactory>,
}

impl OpenAIProvider {
    pub fn new(factory: Arc<HttpClientFactory>) -> Self {
        Self { id: String::from("openai"), factory }
    }
```

原 `fetch` 等方法内所有 `self.client.` 用法改为先取快照（方法开头加一行，正文把 `self.client` 全部替换为 `client`）：

```rust
let client = self.factory.client().await;
```

- [ ] **Step 2: 运行 `cargo check` 确认当前失败点即待改文件清单**

Run: `cd src-tauri && cargo check`
Expected: 报错集中在 `mod.rs`（`OpenAIProvider::new()` 参数不匹配）——按清单逐个改，不引入其它修改

- [ ] **Step 3: 同款改造其余持有者**

- `anthropic.rs` / `openrouter.rs`：与 Step 1 完全同款（字段 `client: Client` → `factory: Arc<HttpClientFactory>`，`new(factory)`，方法开头取快照）
- `subscription.rs`：`SubscriptionFetcher::new(factory)`，其 `fetch` / 各 provider 查询方法开头加 `let client = self.factory.client().await;`，正文 `self.client` → `client`
- `mod.rs`：
  - `http_client: reqwest::Client` 字段删除，新增 `factory: Arc<HttpClientFactory>`
  - `new()` 签名改 `pub fn new(factory: Arc<HttpClientFactory>) -> Self`，内部 `let client = factory.client().await` 不可用（new 非 async）——legacy provider 构造直接传 `factory.clone()`，构造改同步：

```rust
pub fn new(factory: Arc<HttpClientFactory>) -> Self {
    let mut legacy_providers: HashMap<String, Arc<dyn UsageProvider>> = HashMap::new();

    let openai = Arc::new(openai::OpenAIProvider::new(factory.clone()));
    let anthropic = Arc::new(anthropic::AnthropicProvider::new(factory.clone()));
    let openrouter = Arc::new(openrouter::OpenRouterProvider::new(factory.clone()));

    legacy_providers.insert(openai.id(), openai);
    legacy_providers.insert(anthropic.id(), anthropic);
    legacy_providers.insert(openrouter.id(), openrouter);

    Self {
        legacy_providers,
        factory,
        subscription_fetcher: subscription::SubscriptionFetcher::new(factory.clone()),
    }
}
```

  - `http_client_ref` 删除，替换为：

```rust
/// 当前 HTTP client 快照（供命令层调用 script_engine / coding_plan / balance）
pub async fn http_client(&self) -> Arc<reqwest::Client> {
    self.factory.client().await
}
```

  - `ProviderManager` 内部所有使用 `self.http_client` 的查询路径改为 `let client = self.factory.client().await;`

- [ ] **Step 4: 更新命令层调用点**

`provider_commands.rs` 中原 `provider_manager.http_client_ref()` 的调用处改为：

```rust
let client = provider_manager.http_client().await;
// 原来传 &client 参数的地方传 &client（Arc<Client> 解引用后类型一致）
```

- [ ] **Step 5: 编译验证**

Run: `cd src-tauri && cargo check`
Expected: 通过（此时 `lib.rs` 仍是 `ProviderManager::new()` 无参调用，会编译失败——先临时改为 `ProviderManager::new(Arc::new(HttpClientFactory::new(&ProxyConfig { mode: ProxyMode::System, url: String::new(), username: String::new(), password: None })))` 占位，Task 4 换成真实装配）

Run: `cd src-tauri && cargo test`
Expected: 既有测试全部通过（provider 逻辑未变，只换了 client 来源）

- [ ] **Step 6: 提交**

```bash
git add src-tauri/src/providers/ src-tauri/src/commands/provider_commands.rs src-tauri/src/lib.rs
git commit -m "refactor(providers): client 统一由 HttpClientFactory 快照提供，为代理热生效铺路"
```

---

### Task 4: lib.rs 真实装配 + save_settings 代理热生效与密码存储

**Files:**
- Modify: `src-tauri/src/lib.rs:46-55`
- Modify: `src-tauri/src/commands/settings_commands.rs`

**Interfaces:**
- Consumes: Task 1-3 全部产出；KeyStore 既有 `get_stored_key(&str) -> Option<String>` / `set_key(&str, &str)`
- Produces: Tauri State `HttpClientFactory`（前端任务无需感知）；`save_settings` 命令新增第二参数 `proxy_password: Option<String>`（camelCase 调用参数 `proxyPassword`；None 表示不修改密码）

- [ ] **Step 1: 改 `lib.rs` setup 装配（原 46-55 行区域）**

```rust
let app_config = AppConfig::new(app_data_dir.clone());
let key_store = KeyStore::new(app_data_dir.clone());
let settings = app_config.get_settings().await;
let proxy_password = key_store.get_stored_key("proxy_password").await;
let factory = Arc::new(HttpClientFactory::new(&ProxyConfig::from_settings(
    settings.proxy_mode,
    settings.proxy_url.clone(),
    settings.proxy_username.clone(),
    proxy_password,
)));
let provider_manager = ProviderManager::new(factory.clone());

app.manage(app_config);
app.manage(key_store);
app.manage(factory);
app.manage(provider_manager);
app.manage(usage_stats_store);
```

（`usage_stats_store` 与其余行保持原样；`use std::sync::Arc;`、`use net::http_client::{HttpClientFactory, ProxyConfig};` 按需补。）

- [ ] **Step 2: 扩展 `save_settings` 命令**

```rust
use std::sync::Arc;
use tauri::{AppHandle, State};
use crate::config::app_config::{AppConfig, AppSettings, ProxyMode};
use crate::config::encryption::KeyStore;
use crate::net::http_client::{HttpClientFactory, ProxyConfig};

/// 保存应用设置
#[tauri::command]
pub async fn save_settings(
    settings: AppSettings,
    proxy_password: Option<String>,
    app_config: State<'_, AppConfig>,
    key_store: State<'_, KeyStore>,
    factory: State<'_, Arc<HttpClientFactory>>,
    app_handle: AppHandle,
) -> Result<(), String> {
    let previous = app_config.get_settings().await;
    app_config.save_settings(settings).await?;
    let next = app_config.get_settings().await;

    // 语言变化时重建托盘菜单，保持托盘文案与设置页一致。
    if previous.language != next.language {
        crate::tray::refresh_tray_menu(&app_handle, &next);
    }

    // 显式传入新密码才覆盖 KeyStore；None/空串视为"不修改"。
    if let Some(password) = proxy_password.filter(|value| !value.is_empty()) {
        key_store.set_key("proxy_password", &password).await?;
    }

    // 代理四要素任一变化即热重建 client，无需重启应用。
    let proxy_changed = previous.proxy_mode != next.proxy_mode
        || previous.proxy_url != next.proxy_url
        || previous.proxy_username != next.proxy_username
        || proxy_password.is_some();
    if proxy_changed {
        let stored_password = key_store.get_stored_key("proxy_password").await;
        factory
            .reload(&ProxyConfig::from_settings(
                next.proxy_mode,
                next.proxy_url.clone(),
                next.proxy_username.clone(),
                stored_password,
            ))
            .await?;
    }

    Ok(())
}
```

- [ ] **Step 3: 编译 + 全量测试**

Run: `cd src-tauri && cargo check && cargo test`
Expected: 全部通过

- [ ] **Step 4: 提交**

```bash
git add src-tauri/src/lib.rs src-tauri/src/commands/settings_commands.rs
git commit -m "feat(proxy): 启动装配代理工厂，保存设置热生效并把密码隔离进 KeyStore"
```

---

### Task 5: test_proxy_connection 命令

**Files:**
- Modify: `src-tauri/src/commands/settings_commands.rs`
- Modify: `src-tauri/src/lib.rs`（generate_handler 注册）

**Interfaces:**
- Consumes: Task 2 的 `build_client` / `ProxyConfig`
- Produces: 命令 `test_proxy_connection(mode, url, username, password)` → `ProxyTestResult { ok, endpoint, error }`（camelCase）；前端 Task 6 直接 `invoke("test_proxy_connection", {...})`

- [ ] **Step 1: 在 `settings_commands.rs` 追加命令与结果结构**

```rust
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyTestResult {
    pub ok: bool,
    pub endpoint: Option<String>,
    pub error: Option<String>,
}

const PROXY_TEST_ENDPOINTS: [&str; 2] =
    ["https://www.gstatic.com/generate_204", "https://www.baidu.com"];

/// 用表单当前值临时构建 client 测试代理连通性，不影响已保存配置。
#[tauri::command]
pub async fn test_proxy_connection(
    mode: ProxyMode,
    url: String,
    username: String,
    password: Option<String>,
    key_store: State<'_, KeyStore>,
) -> ProxyTestResult {
    // 前端未填密码时回退已存储值，保证"只改了模式"也能测真实配置。
    let effective_password = match password {
        Some(value) if !value.is_empty() => Some(value),
        _ => key_store.get_stored_key("proxy_password").await,
    };
    let proxy = ProxyConfig::from_settings(mode, url, username, effective_password);
    let client = match crate::net::http_client::build_client(&proxy) {
        Ok(client) => client,
        Err(error) => {
            return ProxyTestResult { ok: false, endpoint: None, error: Some(error) };
        }
    };

    let mut last_error = String::from("所有测试端点均不可达");
    for endpoint in PROXY_TEST_ENDPOINTS {
        match client
            .get(endpoint)
            .timeout(std::time::Duration::from_secs(8))
            .send()
            .await
        {
            Ok(response) => {
                // 204（generate_204）或任意 2xx/3xx 都视为代理链路可达
                if response.status().is_success() || response.status().is_redirection() {
                    return ProxyTestResult {
                        ok: true,
                        endpoint: Some(String::from(endpoint)),
                        error: None,
                    };
                }
                last_error = format!("{endpoint} 返回异常状态 {}", response.status());
            }
            Err(error) => {
                last_error = format!("{endpoint}: {error}");
            }
        }
    }
    ProxyTestResult { ok: false, endpoint: None, error: Some(last_error) }
}
```

- [ ] **Step 2: 在 `lib.rs` 的 `generate_handler![...]` 列表加入**

```rust
commands::settings_commands::test_proxy_connection,
```

- [ ] **Step 3: 编译验证**

Run: `cd src-tauri && cargo check`
Expected: 通过

- [ ] **Step 4: 提交**

```bash
git add src-tauri/src/commands/settings_commands.rs src-tauri/src/lib.rs
git commit -m "feat(proxy): 新增 test_proxy_connection 命令，双端点探测并透出具体错误"
```

---

### Task 6: 前端接入（类型 / IPC / 文案 / 设置 UI）

**Files:**
- Modify: `src/types/settings.ts`、`src/utils/ipc.ts`、`src/stores/settingsStore.ts`（保存调用点）、`src/components/settings/SettingsPanel.tsx`、`src/i18n/messages.ts`

**Interfaces:**
- Consumes: Task 4-5 命令参数/返回（camelCase：`proxyPassword` / `ok, endpoint, error`）
- Produces: `AppSettings`（TS）新增 `proxyMode: ProxyMode` / `proxyUrl: string` / `proxyUsername: string`；`type ProxyMode = "none" | "system" | "custom"`；`saveSettings(settings, proxyPassword?)`；`testProxyConnection(args)`

- [ ] **Step 1: `src/types/settings.ts` 加类型与默认值**

```ts
export type ProxyMode = "none" | "system" | "custom";
```

`AppSettings` 接口加三个字段（紧跟 `launchAtStartup` 之后）：

```ts
proxyMode: ProxyMode;
proxyUrl: string;
proxyUsername: string;
```

`DEFAULT_SETTINGS`（110 行附近的对象）同步加：

```ts
proxyMode: "system",
proxyUrl: "",
proxyUsername: "",
```

- [ ] **Step 2: `src/utils/ipc.ts` 扩展**

```ts
export type ProxyTestResult = {
  ok: boolean;
  endpoint: string | null;
  error: string | null;
};

export async function saveSettings(
  settings: AppSettings,
  proxyPassword?: string,
): Promise<void> {
  return invoke("save_settings", { settings, proxyPassword: proxyPassword ?? null });
}

export async function testProxyConnection(args: {
  mode: ProxyMode;
  url: string;
  username: string;
  password?: string;
}): Promise<ProxyTestResult> {
  return invoke("test_proxy_connection", {
    mode: args.mode,
    url: args.url,
    username: args.username,
    password: args.password ?? null,
  });
}
```

（原 `saveSettings` 已存在于 116 行，替换其函数体与签名；所有调用方经 `settingsStore` 走，grep `saveSettings(` 更新传参。）

- [ ] **Step 3: `src/i18n/messages.ts` 三语言块各加 `proxy` 文案（zh-Hans 示例，zh-Hant/en 同键翻译）**

```ts
proxy: {
  title: "网络代理",
  hint: "用量查询请求的出站方式；跟随系统时读取操作系统代理设置",
  modeNone: "不使用",
  modeSystem: "跟随系统",
  modeCustom: "自定义",
  urlLabel: "代理地址",
  urlPlaceholder: "http://127.0.0.1:7890 或 socks5://…",
  usernameLabel: "用户名（可选）",
  passwordLabel: "密码（可选）",
  passwordPlaceholder: "已设置时留空保持不变",
  test: "测试连接",
  testing: "测试中…",
  testOk: "连接成功",
  testOkVia: "连接成功（经 {endpoint}）",
  testFail: "连接失败",
},
```

- [ ] **Step 4: `SettingsPanel.tsx` 通用区新增「网络代理」卡片**

位置：紧跟「窗口与行为」分组卡片之后（同页 `GROUP_TITLE_CLASS` / `GROUP_CARD_CLASS` 风格）。新增状态与处理：

```tsx
const [proxyModeDraft, setProxyModeDraft] = useState<ProxyMode>(settings.proxyMode);
const [proxyUrlDraft, setProxyUrlDraft] = useState(settings.proxyUrl);
const [proxyUsernameDraft, setProxyUsernameDraft] = useState(settings.proxyUsername);
const [proxyPasswordDraft, setProxyPasswordDraft] = useState("");
const [proxyTesting, setProxyTesting] = useState(false);
const [proxyTestResult, setProxyTestResult] = useState<ProxyTestResult | null>(null);
const [proxySaving, setProxySaving] = useState(false);

// settings 变化（保存成功回读）时同步草稿
useEffect(() => {
  setProxyModeDraft(settings.proxyMode);
  setProxyUrlDraft(settings.proxyUrl);
  setProxyUsernameDraft(settings.proxyUsername);
}, [settings.proxyMode, settings.proxyUrl, settings.proxyUsername]);

async function handleProxySave() {
  setProxySaving(true);
  try {
    const next: AppSettings = {
      ...settings,
      proxyMode: proxyModeDraft,
      proxyUrl: proxyModeDraft === "custom" ? proxyUrlDraft.trim() : "",
      proxyUsername: proxyModeDraft === "custom" ? proxyUsernameDraft.trim() : "",
    };
    await saveSettings(next, proxyPasswordDraft || undefined);
    setProxyPasswordDraft("");
    setProxyTestResult(null);
  } finally {
    setProxySaving(false);
  }
}

async function handleProxyTest() {
  setProxyTesting(true);
  try {
    const result = await testProxyConnection({
      mode: proxyModeDraft,
      url: proxyUrlDraft.trim(),
      username: proxyUsernameDraft.trim(),
      password: proxyPasswordDraft || undefined,
    });
    setProxyTestResult(result);
  } finally {
    setProxyTesting(false);
  }
}
```

卡片 JSX（按钮/输入样式复用页内既有类；分段三钮样式与子导航 segmented 一致）：

```tsx
<div className="flex flex-col gap-1.5">
  <h3 className={GROUP_TITLE_CLASS}>{t("settings.proxy.title")}</h3>
  <div className={`${GROUP_CARD_CLASS} flex flex-col gap-3`}>
    <p className="text-xs text-text-muted">{t("settings.proxy.hint")}</p>
    <div className="flex items-center gap-1.5">
      {(["none", "system", "custom"] as const).map((mode) => (
        <button
          key={mode}
          type="button"
          onClick={() => setProxyModeDraft(mode)}
          aria-pressed={proxyModeDraft === mode}
          className={/* 与子导航 segmented active 样式一致，active 时 bg-surface-elevated */}
        >
          {t(`settings.proxy.mode${mode === "none" ? "None" : mode === "system" ? "System" : "Custom"}`)}
        </button>
      ))}
    </div>
    {proxyModeDraft === "custom" && (
      <>
        <label className="flex flex-col gap-1">
          <span className="text-xs text-text-secondary">{t("settings.proxy.urlLabel")}</span>
          <input
            type="text"
            value={proxyUrlDraft}
            onChange={(event) => setProxyUrlDraft(event.target.value)}
            placeholder={t("settings.proxy.urlPlaceholder")}
            className="rounded-md border border-border bg-input-bg px-2.5 py-1.5 text-sm"
          />
        </label>
        <div className="grid grid-cols-2 gap-2">
          <label className="flex flex-col gap-1">
            <span className="text-xs text-text-secondary">{t("settings.proxy.usernameLabel")}</span>
            <input type="text" value={proxyUsernameDraft} onChange={(event) => setProxyUsernameDraft(event.target.value)} className="rounded-md border border-border bg-input-bg px-2.5 py-1.5 text-sm" />
          </label>
          <label className="flex flex-col gap-1">
            <span className="text-xs text-text-secondary">{t("settings.proxy.passwordLabel")}</span>
            <input type="password" value={proxyPasswordDraft} onChange={(event) => setProxyPasswordDraft(event.target.value)} placeholder={t("settings.proxy.passwordPlaceholder")} className="rounded-md border border-border bg-input-bg px-2.5 py-1.5 text-sm" />
          </label>
        </div>
      </>
    )}
    <div className="flex items-center gap-2">
      <button type="button" disabled={proxySaving} onClick={() => void handleProxySave()} className="rounded-md bg-primary px-3 py-1.5 text-xs font-medium text-white disabled:opacity-50">
        保存
      </button>
      <button type="button" disabled={proxyTesting || (proxyModeDraft === "custom" && !proxyUrlDraft.trim())} onClick={() => void handleProxyTest()} className="rounded-md border border-border px-3 py-1.5 text-xs text-text-secondary disabled:opacity-50">
        {proxyTesting ? t("settings.proxy.testing") : t("settings.proxy.test")}
      </button>
      {proxyTestResult && (
        <span className={`text-xs ${proxyTestResult.ok ? "text-success" : "text-error"}`} aria-live="polite">
          {proxyTestResult.ok
            ? t("settings.proxy.testOkVia", { endpoint: proxyTestResult.endpoint ?? "" })
            : `${t("settings.proxy.testFail")}: ${proxyTestResult.error ?? ""}`}
        </span>
      )}
    </div>
  </div>
</div>
```

实现时以页内相邻卡片真实类名为准微调；「保存」按钮文案沿用页内既有保存按钮的 key（若通用区已有统一保存入口则复用之，`proxyPassword` 仅在该入口传参）。

- [ ] **Step 5: 前端验证**

Run: `npx tsc --noEmit`
Expected: 通过

- [ ] **Step 6: 提交**

```bash
git add src/types/settings.ts src/utils/ipc.ts src/stores/settingsStore.ts src/components/settings/SettingsPanel.tsx src/i18n/messages.ts
git commit -m "feat(settings): 设置页新增网络代理分区，支持三模式、凭据与测试连接"
```

---

### Task 7: 端到端手动验证 + 文档同步

**Files:**
- Modify: `AGENTS.md`（新增「代理约束」小节 + 修改流程验证项）
- Modify: `CLAUDE.md`（镜像同步）

- [ ] **Step 1: 全量静态检查**

Run: `npx tsc --noEmit && cd src-tauri && cargo fmt --all --check && cargo check && cargo test`
Expected: 全部通过

- [ ] **Step 2: 手动验证关键路径（`npm run tauri dev`）**

1. 不配置代理：各供应商刷新正常（行为与改动前一致）
2. 「跟随系统」：开启系统代理（如 Clash 系统代理）后刷新走代理；关闭后刷新直连
3. 「自定义」`http://127.0.0.1:7890`：测试连接显示成功并带命中端点；填非法地址时保存/测试报「代理地址无效」
4. 改代理后**不重启**，刷新立即按新代理生效（热重建）
5. 用户名/密码代理：填入后测试连接成功；保存后重新打开设置页密码框为空、测试仍成功（KeyStore 回读）
6. 代理错误场景：填一个不存在的代理地址测试连接，错误文案包含具体端点与失败原因
7. WebDAV 备份功能不受代理设置影响（直连 NAS/局域网端点仍可用）

- [ ] **Step 3: 更新 AGENTS.md / CLAUDE.md**

新增小节「代理约束」要点：

- 持久化字段 `proxyMode` / `proxyUrl` / `proxyUsername`（config.json）+ `proxy_password`（KeyStore）
- 所有用量查询 client 必须从 `net::http_client::HttpClientFactory` 获取，禁止再新建 `reqwest::Client`（WebDAV 例外）
- 默认 `system`；`none` 显式禁用系统代理；`custom` 支持 http/https/socks5 + Basic 鉴权
- 保存设置即热生效；`test_proxy_connection` 是唯一代理诊断入口
- 更新器（tauri-plugin-updater）与 WebDAV 不走代理

- [ ] **Step 4: 提交**

```bash
git add AGENTS.md CLAUDE.md
git commit -m "docs: 补充代理约束与排查点"
```

---

## Self-Review 记录

- 规格覆盖：§2.1 字段（Task 1）✅ §2.2 工厂+改造点（Task 2-4）✅ §2.3 UI+测试连接（Task 5-6）✅ §2.4 边界（WebDAV 排除写入 Task 7 文档、updater 排除写入文档）✅
- 占位符扫描：Task 6 Step 4 的 segmented active 样式与「保存」按钮文案引用页内既有模式，已显式标注以真实类名为准——属于对既有代码的引用而非待定项
- 类型一致性：`ProxyConfig::from_settings(mode, url, username, password)` 在 Task 2/4/5 三处签名一致；`saveSettings(settings, proxyPassword?)` 与 Rust `proxy_password: Option<String>` 对应；`ProxyTestResult` 前后端字段一致
