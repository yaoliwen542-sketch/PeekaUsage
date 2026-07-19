import { useEffect, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import EdgeDockHandle from "./components/common/EdgeDockHandle";
import TitleBar from "./components/common/TitleBar";
import WidgetContainer from "./components/widget/WidgetContainer";
import SettingsPanel from "./components/settings/SettingsPanel";
import { useEdgeDock } from "./composables/useEdgeDock";
import { useWindowControls } from "./composables/useWindowControls";
import { useProviderStore } from "./stores/providerStore";
import {
  SETTINGS_CHANGED_EVENT,
  useSettingsStore,
  type SettingsChangedPayload,
} from "./stores/settingsStore";
import { useUpdateStore } from "./stores/updateStore";
import { applyTheme, observeSystemTheme } from "./utils/theme";

export default function App() {
  const [currentView, setCurrentView] = useState<"widget" | "settings">("widget");
  const settings = useSettingsStore((state) => state.settings);
  const loadSettings = useSettingsStore((state) => state.loadSettings);
  const { applyOpacity } = useWindowControls();
  // 窗口生命周期（启动恢复边界、移动/缩放监听与持久化、边缘吸附收起状态机）
  // 已抽成 useEdgeDock composable，这里只消费视觉状态与拖拽/悬停入口
  const {
    dockVisualState,
    initializeWindowLifecycle,
    registerTitlebarDragIntent,
    handleAppMouseEnter,
    handleAppMouseLeave,
  } = useEdgeDock();

  useEffect(() => {
    let active = true;
    let unlistenRefresh: UnlistenFn | null = null;
    let unlistenSettings: UnlistenFn | null = null;
    let stopObservingSystemTheme: (() => void) | null = null;
    let cleanupWindowLifecycle: (() => void) | null = null;
    const currentWindow = getCurrentWindow();

    async function syncAlwaysOnTop(alwaysOnTop: boolean) {
      try {
        await currentWindow.setAlwaysOnTop(alwaysOnTop);
      } catch {
        // 忽略置顶同步失败，避免影响界面初始化
      }
    }

    void (async () => {
      await loadSettings();
      const currentSettings = useSettingsStore.getState().settings;

      applyTheme(currentSettings.theme);
      await applyOpacity(currentSettings.windowOpacity);
      await syncAlwaysOnTop(currentSettings.alwaysOnTop);
      // 恢复窗口边界并注册移动/缩放监听（useEdgeDock）
      const cleanup = await initializeWindowLifecycle();

      if (!active) {
        cleanup();
        return;
      }
      cleanupWindowLifecycle = cleanup;

      unlistenRefresh = await listen("tray-refresh", () => {
        void useProviderStore.getState().refreshAll();
      });

      unlistenSettings = await listen("tray-open-settings", () => {
        setCurrentView("settings");
      });

      stopObservingSystemTheme = observeSystemTheme(() => {
        if (useSettingsStore.getState().settings.theme === "system") {
          applyTheme("system");
        }
      });
    })();

    return () => {
      active = false;
      unlistenRefresh?.();
      unlistenSettings?.();
      stopObservingSystemTheme?.();
      cleanupWindowLifecycle?.();
    };
  }, [applyOpacity, loadSettings, initializeWindowLifecycle]);

  // 跨窗口设置同步：收到其他窗口（如灵动岛）保存的设置后更新本地 store，
  // 忽略自己发出的事件避免回环；theme/opacity/alwaysOnTop 由下方既有 effect 响应
  useEffect(() => {
    let active = true;
    let unlistenSettingsChanged: UnlistenFn | null = null;
    const windowLabel = getCurrentWindow().label;

    void listen<SettingsChangedPayload>(SETTINGS_CHANGED_EVENT, (event) => {
      if (event.payload.source === windowLabel) {
        return;
      }
      useSettingsStore.getState().applySyncedSettings(event.payload.settings);
    }).then((fn) => {
      if (active) {
        unlistenSettingsChanged = fn;
      } else {
        fn();
      }
    });

    return () => {
      active = false;
      unlistenSettingsChanged?.();
    };
  }, []);

  useEffect(() => {
    applyTheme(settings.theme);
  }, [settings.theme]);

  useEffect(() => {
    void getCurrentWindow().setAlwaysOnTop(settings.alwaysOnTop).catch(() => {
      // 忽略置顶同步失败，避免影响界面更新
    });
  }, [settings.alwaysOnTop]);

  useEffect(() => {
    void applyOpacity(settings.windowOpacity);
  }, [applyOpacity, settings.windowOpacity]);

  // 应用内更新检查：必须等设置加载完成后再评估，
  // 否则读到的是默认值（updateCheckOnLaunch=true），用户关掉启动检查也会被触发
  const settingsLoaded = useSettingsStore((state) => state.loaded);

  // 灵动岛显隐：跟随 islandVisible 设置（启动恢复 + 设置页/托盘切换同步）。
  // 岛窗口由 tauri.conf.json 常驻创建；getByLabel 在窗口未就绪时可能失败，静默忽略即可，
  // 后端 lib.rs 启动恢复与托盘切换路径也会直接操作岛窗口显隐作为兜底。
  useEffect(() => {
    if (!settingsLoaded) {
      return;
    }

    const visible = settings.islandVisible;
    void (async () => {
      try {
        const island = await WebviewWindow.getByLabel("island");
        if (!island) {
          return;
        }
        if (visible) {
          await island.show();
        } else {
          await island.hide();
        }
      } catch {
        // 灵动岛窗口尚未创建或已销毁时忽略，不阻塞主界面
      }
    })();
  }, [settingsLoaded, settings.islandVisible]);

  useEffect(() => {
    if (!settingsLoaded) {
      return;
    }

    const { loadCurrentVersion, checkUpdate, lastCheckAt } = useUpdateStore.getState();
    void loadCurrentVersion();

    const currentSettings = useSettingsStore.getState().settings;
    if (currentSettings.updateAutoCheckEnabled && currentSettings.updateCheckOnLaunch) {
      const TEN_MINUTES = 10 * 60 * 1000;
      if (lastCheckAt === null || Date.now() - lastCheckAt > TEN_MINUTES) {
        void checkUpdate();
      }
    }
  }, [settingsLoaded]);

  useEffect(() => {
    if (!settings.updateAutoCheckEnabled) return;

    const intervalMs = settings.updateCheckIntervalHours * 60 * 60 * 1000;
    const timer = setInterval(() => {
      void useUpdateStore.getState().checkUpdate();
    }, intervalMs);

    return () => clearInterval(timer);
  }, [settings.updateAutoCheckEnabled, settings.updateCheckIntervalHours]);

  async function handleBackFromSettings() {
    setCurrentView("widget");

    if (useSettingsStore.getState().settings.refreshOnSettingsClose) {
      await useProviderStore.getState().refreshAll();
    }
  }

  return (
    <div
      className={`app-shell${dockVisualState ? " is-edge-docked" : ""}${dockVisualState?.phase === "collapsed" ? " is-edge-docked-collapsed" : ""}${dockVisualState?.phase === "preview" ? " is-edge-docked-preview" : ""}${dockVisualState ? ` edge-${dockVisualState.edge}` : ""}`}
      onMouseEnter={handleAppMouseEnter}
      onMouseLeave={handleAppMouseLeave}
    >
      <TitleBar onDragIntentStart={registerTitlebarDragIntent} />
      {currentView === "widget" ? (
        <WidgetContainer
          onOpenSettings={() => setCurrentView("settings")}
          onDragIntentStart={registerTitlebarDragIntent}
          suppressWindowAutoFit={dockVisualState?.phase === "collapsed"}
        />
      ) : (
        <SettingsPanel onBack={() => void handleBackFromSettings()} />
      )}
      {dockVisualState?.phase === "collapsed" ? <EdgeDockHandle edge={dockVisualState.edge} /> : null}
    </div>
  );
}
