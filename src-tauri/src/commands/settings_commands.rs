use std::sync::Arc;

use tauri::{AppHandle, State};

use crate::config::app_config::{AppConfig, AppSettings};
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
