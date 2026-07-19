mod commands;
mod config;
mod polling;
mod providers;
mod stats;
mod tray;

use config::app_config::AppConfig;
use config::encryption::KeyStore;
use config::migration::migrate_legacy_app_data;
use providers::ProviderManager;
use stats::UsageStatsStore;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // 单实例插件必须尽量最早注册（官方要求），避免其它插件的初始化在第二实例里白跑。
        // 第二实例启动时回调聚焦已有主窗口，随后插件自动终止第二实例，
        // 防止双开并发写 config.json / keys.dat / usage_stats.json 互相覆盖。
        // 灵动岛窗口随主窗口同属一个进程，无需单独处理。
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                if window.is_minimized().unwrap_or(false) {
                    let _ = window.unminimize();
                }
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .setup(|app| {
            // 获取应用数据目录
            let app_data_dir = app.path().app_data_dir().expect("无法获取应用数据目录");

            // 品牌改名后保留旧数据，避免 identifier 变化导致用户配置和凭据丢失。
            migrate_legacy_app_data(&app_data_dir).map_err(std::io::Error::other)?;

            // 初始化状态
            let app_config = AppConfig::new(app_data_dir.clone());
            let key_store = KeyStore::new(app_data_dir.clone());
            let provider_manager = ProviderManager::new();
            let usage_stats_store = UsageStatsStore::new(app_data_dir);
            let initial_settings = tauri::async_runtime::block_on(app_config.get_settings());

            app.manage(app_config);
            app.manage(key_store);
            app.manage(provider_manager);
            app.manage(usage_stats_store);

            // 初始化系统托盘（菜单文案与灵动岛勾选态按初始设置生成）
            let handle = app.handle().clone();
            tray::setup_tray(&handle, &initial_settings)?;

            // 启动时按配置恢复灵动岛显隐（默认显示；用户关闭后重启保持隐藏，避免先闪一下再隐藏）
            if !initial_settings.island_visible {
                if let Some(island) = app.get_webview_window("island") {
                    let _ = island.hide();
                }
            }

            // 窗口关闭事件：隐藏到托盘而非退出
            let window = app.get_webview_window("main").unwrap();
            #[cfg(windows)]
            if initial_settings.hide_taskbar_icon {
                let _ = window.set_skip_taskbar(true);
            }
            window.on_window_event(move |event| {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    if let Some(w) = handle.get_webview_window("main") {
                        let _ = w.hide();
                    }
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::provider_commands::fetch_all_usage,
            commands::provider_commands::fetch_provider_usage,
            commands::provider_commands::get_provider_configs,
            commands::provider_commands::get_supported_providers,
            commands::provider_commands::save_provider_config,
            commands::provider_commands::remove_provider_config,
            commands::provider_commands::save_provider_order,
            commands::provider_commands::validate_api_key,
            commands::provider_commands::activate_provider_api_key,
            commands::provider_commands::get_provider_templates,
            commands::provider_commands::get_newapi_script_template,
            commands::provider_commands::test_custom_provider_script,
            commands::settings_commands::get_settings,
            commands::settings_commands::save_settings,
            commands::stats_commands::get_usage_stats_snapshot,
            commands::taskbar_commands::set_window_skip_taskbar,
            commands::window_commands::detect_oauth_tokens,
            commands::update_commands::check_app_update,
            commands::update_commands::install_app_update,
            commands::update_commands::get_current_version,
        ])
        .build(tauri::generate_context!())
        .expect("启动应用失败")
        .run(|app_handle, event| {
            // 修复 L12：统计写入做了 30s 节流，退出前把内存中未落盘的修改兜底写盘，
            // 避免正常退出丢失最近一次刷新产生的样本。
            if let tauri::RunEvent::ExitRequested { .. } = event {
                let store = app_handle.state::<UsageStatsStore>();
                if let Err(error) = tauri::async_runtime::block_on(store.flush_if_dirty()) {
                    eprintln!("退出前落盘统计历史失败: {}", error);
                }
            }
        });
}
