mod commands;
mod config;
mod net;
mod polling;
mod providers;
mod stats;
mod tray;

use config::app_config::AppConfig;
use config::encryption::KeyStore;
use config::migration::migrate_legacy_app_data;
use net::http_client::{HttpClientFactory, ProxyConfig};
use providers::ProviderManager;
use stats::UsageStatsStore;
use std::sync::Arc;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // 单实例插件必须尽量最早注册（官方要求），避免其它插件的初始化在第二实例里白跑。
        // 第二实例启动时回调聚焦已有主窗口，随后插件自动终止第二实例，
        // 防止双开并发写 config.json / keys.dat / usage_stats.json 互相覆盖。
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
            // 真实装配：从已保存配置读取代理四要素构造工厂；密码只从 KeyStore 读，
            // 绝不写进 config.json。保存设置后经 factory.reload() 热重建 client。
            let initial_settings = tauri::async_runtime::block_on(app_config.get_settings());
            let proxy_password =
                tauri::async_runtime::block_on(key_store.get_stored_key("proxy_password"));
            let factory = Arc::new(HttpClientFactory::new(&ProxyConfig::from_settings(
                initial_settings.proxy_mode,
                initial_settings.proxy_url.clone(),
                initial_settings.proxy_username.clone(),
                proxy_password,
            )));
            let provider_manager = ProviderManager::new(factory.clone());
            let usage_stats_store = UsageStatsStore::new(app_data_dir);
            // 统计加载已移出启动关键路径：这里只建空历史并登记后台任务，
            // usage_stats.json 的读+解析在后台完成后换入（移植自玻璃壳分支，
            // 实测同步解析 35MB 文件占启动耗时 ~97%）
            usage_stats_store.start_background_load();

            app.manage(app_config);
            app.manage(key_store);
            app.manage(factory);
            app.manage(provider_manager);
            app.manage(usage_stats_store);

            // 初始化系统托盘。
            let handle = app.handle().clone();
            commands::data_commands::start_webdav_auto_sync(handle.clone());
            tray::setup_tray(&handle, &initial_settings)?;

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
            commands::settings_commands::test_proxy_connection,
            commands::stats_commands::get_usage_stats_snapshot,
            commands::taskbar_commands::set_window_skip_taskbar,
            commands::window_commands::detect_oauth_tokens,
            commands::data_commands::export_app_data,
            commands::data_commands::export_app_data_to_downloads,
            commands::data_commands::import_app_data,
            commands::data_commands::upload_app_data_to_webdav,
            commands::data_commands::upload_app_data_to_webdav_with_root,
            commands::data_commands::download_app_data_from_webdav,
            commands::data_commands::get_webdav_sync_password,
            commands::data_commands::save_webdav_sync_password,
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
