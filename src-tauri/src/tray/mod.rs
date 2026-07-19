use serde::Serialize;
use tauri::{
    menu::{CheckMenuItem, Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager,
};
use tokio::time::{sleep, Duration};

use crate::config::app_config::{AppConfig, AppLanguage, AppSettings};

/// 托盘图标 ID：保存设置后重建菜单时按此 ID 找回托盘实例
const TRAY_ID: &str = "main-tray";

/// 前端跨窗口设置同步事件名，与 src/stores/settingsStore.ts 保持一致
const SETTINGS_CHANGED_EVENT: &str = "settings-changed";

/// settings-changed 事件负载（与前端 SettingsChangedPayload 对齐）
///
/// source 标记变更来源；托盘发起的变更固定为 "tray"，
/// 前端收到后只更新内存状态，不再回写或再广播，避免回环。
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SettingsChangedEventPayload<'a> {
    source: &'a str,
    settings: &'a AppSettings,
}

/// 托盘菜单文案表（按界面语言三选一，仅覆盖托盘自身，不引入前端 i18n 体系）
struct TrayTexts {
    show_toggle: &'static str,
    island_visible: &'static str,
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
            island_visible: "显示灵动岛",
            recenter: "重置到屏幕中央并置顶",
            refresh: "刷新所有数据",
            settings: "设置...",
            quit: "退出",
        },
        AppLanguage::ZhHant => TrayTexts {
            show_toggle: "顯示/隱藏視窗",
            island_visible: "顯示靈動島",
            recenter: "重置到螢幕中央並置頂",
            refresh: "刷新所有資料",
            settings: "設定...",
            quit: "結束",
        },
        AppLanguage::En => TrayTexts {
            show_toggle: "Show/Hide Window",
            island_visible: "Show Island",
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
        let was_always_on_top = window.is_always_on_top().unwrap_or(false);

        if window.is_minimized().unwrap_or(false) {
            let _ = window.unminimize();
        }

        let _ = window.show();
        let _ = window.set_always_on_top(true);
        let _ = window.center();
        let _ = window.set_focus();

        if !was_always_on_top {
            let window = window.clone();
            tauri::async_runtime::spawn(async move {
                sleep(Duration::from_millis(1200)).await;
                let _ = window.set_always_on_top(false);
            });
        }
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

/// 按当前设置（语言 + 灵动岛可见性）构建托盘菜单
fn build_tray_menu(
    app: &AppHandle,
    settings: &AppSettings,
) -> Result<Menu<tauri::Wry>, Box<dyn std::error::Error>> {
    let texts = tray_texts(&settings.language);

    let show = MenuItem::with_id(app, "show", texts.show_toggle, true, None::<&str>)?;
    // 灵动岛显隐用勾选态菜单项，勾选状态直接反映 island_visible
    let island = CheckMenuItem::with_id(
        app,
        "island-visibility",
        texts.island_visible,
        true,
        settings.island_visible,
        None::<&str>,
    )?;
    let recenter = MenuItem::with_id(app, "recenter", texts.recenter, true, None::<&str>)?;
    let refresh = MenuItem::with_id(app, "refresh", texts.refresh, true, None::<&str>)?;
    let settings_item = MenuItem::with_id(app, "settings", texts.settings, true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", texts.quit, true, None::<&str>)?;

    let menu = Menu::with_items(
        app,
        &[&show, &island, &recenter, &refresh, &settings_item, &quit],
    )?;

    Ok(menu)
}

/// 按最新设置重建托盘菜单（语言切换或灵动岛勾选态变化后调用）。
/// 只替换菜单、不重建托盘图标，保持事件回调不变。
pub fn refresh_tray_menu(app: &AppHandle, settings: &AppSettings) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };

    if let Ok(menu) = build_tray_menu(app, settings) {
        let _ = tray.set_menu(Some(menu));
    }
}

/// 托盘点击切换灵动岛显隐：
/// 走正常配置保存链路，同步岛窗口显隐，并广播 settings-changed 让前端各窗口同步。
fn toggle_island_visibility(app: &AppHandle) {
    let app_handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let app_config = app_handle.state::<AppConfig>();
        let mut settings = app_config.get_settings().await;
        settings.island_visible = !settings.island_visible;

        if let Err(err) = app_config.save_settings(settings.clone()).await {
            eprintln!("保存灵动岛可见性失败: {}", err);
            return;
        }

        // 直接同步岛窗口显隐（前端 effect 会幂等地重复一次，无副作用）
        if let Some(island) = app_handle.get_webview_window("island") {
            if settings.island_visible {
                let _ = island.show();
            } else {
                let _ = island.hide();
            }
        }

        // 刷新托盘菜单勾选态
        refresh_tray_menu(&app_handle, &settings);

        // 通知前端各窗口同步设置；前端 applySyncedSettings 只更新内存状态，不回写，避免回环
        let _ = app_handle.emit(
            SETTINGS_CHANGED_EVENT,
            SettingsChangedEventPayload {
                source: "tray",
                settings: &settings,
            },
        );
    });
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
                "island-visibility" => {
                    toggle_island_visibility(app);
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
