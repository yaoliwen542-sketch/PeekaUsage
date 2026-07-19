use tauri::{AppHandle, State};

use crate::config::app_config::{AppConfig, AppSettings};

/// 获取应用设置
#[tauri::command]
pub async fn get_settings(app_config: State<'_, AppConfig>) -> Result<AppSettings, String> {
    Ok(app_config.get_settings().await)
}

/// 保存应用设置
#[tauri::command]
pub async fn save_settings(
    settings: AppSettings,
    app_config: State<'_, AppConfig>,
    app_handle: AppHandle,
) -> Result<(), String> {
    let previous = app_config.get_settings().await;
    app_config.save_settings(settings).await?;
    let next = app_config.get_settings().await;

    // 托盘菜单文案和灵动岛勾选态来自配置：
    // 语言或灵动岛可见性变化时重建托盘菜单，保持与设置页一致
    if previous.language != next.language || previous.island_visible != next.island_visible {
        crate::tray::refresh_tray_menu(&app_handle, &next);
    }

    Ok(())
}
