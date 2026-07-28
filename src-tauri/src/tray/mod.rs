use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager,
};
use tokio::time::{sleep, Duration};

use crate::config::app_config::{AppConfig, AppLanguage, AppSettings};

/// 托盘图标 ID：保存设置后重建菜单时按此 ID 找回托盘实例
const TRAY_ID: &str = "main-tray";

/// 托盘菜单文案表（按界面语言三选一，仅覆盖托盘自身，不引入前端 i18n 体系）
struct TrayTexts {
    show_toggle: &'static str,
    recenter: &'static str,
    refresh: &'static str,
    settings: &'static str,
    quit: &'static str,
}

/// 按当前语言返回托盘菜单文案
fn tray_texts(language: &AppLanguage) -> TrayTexts {
    match language {
        AppLanguage::ZhHans => TrayTexts {
            show_toggle: "显示/隐藏窗口",
            recenter: "重置到屏幕中央并置顶",
            refresh: "刷新所有数据",
            settings: "设置...",
            quit: "退出",
        },
        AppLanguage::ZhHant => TrayTexts {
            show_toggle: "顯示/隱藏視窗",
            recenter: "重置到螢幕中央並置頂",
            refresh: "刷新所有資料",
            settings: "設定...",
            quit: "結束",
        },
        AppLanguage::En => TrayTexts {
            show_toggle: "Show/Hide Window",
            recenter: "Center & Bring to Front",
            refresh: "Refresh All Data",
            settings: "Settings...",
            quit: "Quit",
        },
    }
}

fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        if window.is_minimized().unwrap_or(false) {
            let _ = window.unminimize();
        }
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn recenter_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        // 修复 L18：记录操作前的置顶状态，用于读不到设置时的兜底
        let was_always_on_top = window.is_always_on_top().unwrap_or(false);

        if window.is_minimized().unwrap_or(false) {
            let _ = window.unminimize();
        }

        let _ = window.show();
        let _ = window.set_always_on_top(true);
        let _ = window.center();
        let _ = window.set_focus();

        let app_handle = app.clone();
        let window = window.clone();
        tauri::async_runtime::spawn(async move {
            sleep(Duration::from_millis(1200)).await;
            // 修复 L18：1.2s 后不能无条件 set_always_on_top(false)——
            // 用户在这 1.2s 内手动开启置顶会被改回。
            // 优先按持久化设置恢复（窗口置顶状态平时就由 settings.always_on_top 驱动，
            // 用户刚做的切换此时已保存）；读不到设置时回退到操作前的窗口状态。
            let target = match app_handle.try_state::<AppConfig>() {
                Some(config) => config.get_settings().await.always_on_top,
                None => was_always_on_top,
            };
            let _ = window.set_always_on_top(target);
        });
    }
}

fn toggle_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        if window.is_minimized().unwrap_or(false) {
            let _ = window.unminimize();
            let _ = window.show();
            let _ = window.set_focus();
        } else if window.is_visible().unwrap_or(false) {
            let _ = window.hide();
        } else {
            let _ = window.show();
            let _ = window.set_focus();
        }
    }
}

/// 按当前设置语言构建托盘菜单
fn build_tray_menu(
    app: &AppHandle,
    settings: &AppSettings,
) -> Result<Menu<tauri::Wry>, Box<dyn std::error::Error>> {
    let texts = tray_texts(&settings.language);

    let show = MenuItem::with_id(app, "show", texts.show_toggle, true, None::<&str>)?;
    let recenter = MenuItem::with_id(app, "recenter", texts.recenter, true, None::<&str>)?;
    let refresh = MenuItem::with_id(app, "refresh", texts.refresh, true, None::<&str>)?;
    let settings_item = MenuItem::with_id(app, "settings", texts.settings, true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", texts.quit, true, None::<&str>)?;

    let menu = Menu::with_items(app, &[&show, &recenter, &refresh, &settings_item, &quit])?;

    Ok(menu)
}

/// 按最新设置重建托盘菜单（语言切换后调用）。
/// 只替换菜单、不重建托盘图标，保持事件回调不变。
pub fn refresh_tray_menu(app: &AppHandle, settings: &AppSettings) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };

    if let Ok(menu) = build_tray_menu(app, settings) {
        let _ = tray.set_menu(Some(menu));
    }
}

/// 初始化系统托盘
pub fn setup_tray(
    app: &AppHandle,
    settings: &AppSettings,
) -> Result<(), Box<dyn std::error::Error>> {
    let menu = build_tray_menu(app, settings)?;

    let mut tray_builder = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .tooltip("PeekaUsage")
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| {
            match event.id.as_ref() {
                "show" => {
                    toggle_main_window(app);
                }
                "recenter" => {
                    recenter_main_window(app);
                }
                "refresh" => {
                    // 通过事件通知前端刷新
                    let _ = app.emit("tray-refresh", ());
                }
                "settings" => {
                    show_main_window(app);
                    let _ = app.emit("tray-open-settings", ());
                }
                "quit" => {
                    app.exit(0);
                }
                _ => {}
            }
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                toggle_main_window(&tray.app_handle());
            }
        });

    if let Some(icon) = app.default_window_icon().cloned() {
        tray_builder = tray_builder.icon(icon);
    }

    let _tray = tray_builder.build(app)?;

    Ok(())
}
