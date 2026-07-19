use tauri::{AppHandle, Manager};

/// 设置是否隐藏 Windows 任务栏图标
#[tauri::command]
pub async fn set_window_skip_taskbar(skip: bool, app: AppHandle) -> Result<(), String> {
    let window = app.get_webview_window("main").ok_or("找不到主窗口")?;

    #[cfg(windows)]
    {
        window
            .set_skip_taskbar(skip)
            .map_err(|error| format!("设置任务栏图标状态失败: {}", error))?;
    }

    #[cfg(not(windows))]
    {
        let _ = skip;
    }

    Ok(())
}
