import { create } from "zustand";
import { emit } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { AppSettings } from "../types/settings";
import {
  DEFAULT_SETTINGS,
  normalizeAppSettings,
} from "../types/settings";
import { getSettings, saveSettings as ipcSaveSettings } from "../utils/ipc";

/** 跨窗口设置同步事件名（主窗口与灵动岛窗口共用） */
export const SETTINGS_CHANGED_EVENT = "settings-changed";

/**
 * settings-changed 事件负载。
 * source 是发出变更的窗口 label，接收方据此忽略自己发出的事件，避免回环覆盖。
 */
export type SettingsChangedPayload = {
  source: string;
  settings: AppSettings;
};

type SettingsStoreState = {
  settings: AppSettings;
  loaded: boolean;
  loadSettings: (force?: boolean) => Promise<void>;
  saveSettings: (newSettings: Partial<AppSettings>) => Promise<void>;
  applySyncedSettings: (remoteSettings: AppSettings) => void;
};

/** 把远端/持久化的设置合并到默认值上并归一化（加载与跨窗口同步共用） */
function mergeWithDefaults(remoteSettings: Partial<AppSettings>): AppSettings {
  return normalizeAppSettings({
    ...DEFAULT_SETTINGS,
    ...remoteSettings,
    providerCardExpanded: {
      ...DEFAULT_SETTINGS.providerCardExpanded,
      ...remoteSettings.providerCardExpanded,
    },
    providerPollingOverrides: {
      ...DEFAULT_SETTINGS.providerPollingOverrides,
      ...remoteSettings.providerPollingOverrides,
    },
  });
}

let loadingPromise: Promise<void> | null = null;

export const useSettingsStore = create<SettingsStoreState>((set, get) => ({
  settings: { ...DEFAULT_SETTINGS },
  loaded: false,

  async loadSettings(force = false) {
    if (get().loaded && !force) {
      return;
    }

    if (loadingPromise && !force) {
      return loadingPromise;
    }

    loadingPromise = (async () => {
      try {
        const remoteSettings = await getSettings();
        set({
          settings: mergeWithDefaults(remoteSettings),
          loaded: true,
        });
      } catch {
        set({
          settings: { ...DEFAULT_SETTINGS },
          loaded: true,
        });
      }
    })();

    try {
      await loadingPromise;
    } finally {
      loadingPromise = null;
    }
  },

  async saveSettings(newSettings: Partial<AppSettings>) {
    const currentSettings = get().settings;
    const nextSettings = normalizeAppSettings({
      ...currentSettings,
      ...newSettings,
      providerPollingOverrides: {
        ...currentSettings.providerPollingOverrides,
        ...newSettings.providerPollingOverrides,
      },
    });

    set({
      settings: nextSettings,
    });

    await ipcSaveSettings(nextSettings);

    // 持久化成功后广播给其他窗口（如灵动岛），保持多窗口设置一致
    try {
      const payload: SettingsChangedPayload = {
        source: getCurrentWindow().label,
        settings: nextSettings,
      };
      await emit(SETTINGS_CHANGED_EVENT, payload);
    } catch {
      // 广播失败不影响本地保存结果
    }
  },

  applySyncedSettings(remoteSettings: AppSettings) {
    // 仅更新内存状态，不再持久化或广播，避免窗口间回环
    set({
      settings: mergeWithDefaults(remoteSettings),
    });
  },
}));
