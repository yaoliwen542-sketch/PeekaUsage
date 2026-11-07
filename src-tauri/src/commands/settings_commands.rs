use std::sync::Arc;

use tauri::{AppHandle, Manager, State};

use crate::config::app_config::{AppConfig, AppSettings, ProxyMode};
use crate::config::encryption::KeyStore;
use crate::net::http_client::{HttpClientFactory, ProxyConfig};

/// 获取应用设置
#[tauri::command]
pub async fn get_settings(app_config: State<'_, AppConfig>) -> Result<AppSettings, String> {
    Ok(app_config.get_settings().await)
}

/// 保存应用设置
///
/// `proxy_password` 语义：`Some(非空)` 才覆盖 KeyStore 中的代理密码；
/// `None` / 空串表示"不修改"。reload 时始终回读 KeyStore 最新值。
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
    // 用 as_ref() 借用而非消耗 Option，后面 proxy_changed 判定还要读 proxy_password。
    if let Some(password) = proxy_password.as_ref().filter(|value| !value.is_empty()) {
        key_store.set_key("proxy_password", password).await?;
    }

    // 代理四要素（mode/url/username/password）任一变化即热重建 client，无需重启应用。
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

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyTestResult {
    pub ok: bool,
    pub endpoint: Option<String>,
    pub error: Option<String>,
}

const PROXY_TEST_ENDPOINTS: [&str; 2] = [
    "https://www.gstatic.com/generate_204",
    "https://www.baidu.com",
];

/// 用表单当前值临时构建 client 测试代理连通性，不影响已保存配置。
///
/// 注：Tauri v2 要求含引用输入（`State<'_, ...>`）的 async 命令必须返回
/// `Result`，为保留裸结构体返回（前端 invoke 永不 reject），这里改用
/// `AppHandle` owned 参数、命令内经 `state::<KeyStore>()` 取凭据存储。
#[tauri::command]
pub async fn test_proxy_connection(
    mode: ProxyMode,
    url: String,
    username: String,
    password: Option<String>,
    app_handle: AppHandle,
) -> ProxyTestResult {
    let key_store = app_handle.state::<KeyStore>();
    // 前端未填密码时回退已存储值，保证"只改了模式"也能测真实配置。
    let effective_password = match password {
        Some(value) if !value.is_empty() => Some(value),
        _ => key_store.get_stored_key("proxy_password").await,
    };
    let proxy = ProxyConfig::from_settings(mode, url, username, effective_password);
    let client = match crate::net::http_client::build_client(&proxy) {
        Ok(client) => client,
        Err(error) => {
            return ProxyTestResult {
                ok: false,
                endpoint: None,
                error: Some(error),
            };
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
    ProxyTestResult {
        ok: false,
        endpoint: None,
        error: Some(last_error),
    }
}
