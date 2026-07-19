import { useEffect, useMemo, useRef, useState } from "react";
import type { ProviderId } from "../types/provider";
import {
  getEffectivePollingSettings,
  getPollingIntervalMs,
} from "../types/settings";
import { useProviderStore } from "../stores/providerStore";
import { useSettingsStore } from "../stores/settingsStore";

export function usePolling() {
  const [isActive, setIsActive] = useState(false);
  const shouldRunRef = useRef(false);
  const timersRef = useRef(new Map<ProviderId, ReturnType<typeof setInterval>>());
  const providers = useProviderStore((state) => state.providers);
  const settings = useSettingsStore((state) => state.settings);

  // 定时器调度指纹：只包含"启用中的供应商 id + 各自生效的轮询策略"，
  // providers 数据刷新（引用变化但策略没变）不会触发定时器重建，
  // 避免短间隔供应商每刷一次就把长间隔供应商的定时器清零重计。
  const pollingScheduleFingerprint = useMemo(() => {
    const entries = providers
      .filter((provider) => provider.enabled)
      .map((provider) => {
        const pollingSettings = getEffectivePollingSettings(settings, provider.providerId);
        return `${provider.providerId}:${pollingSettings.pollingMode}/${pollingSettings.pollingInterval}/${pollingSettings.pollingUnit}`;
      })
      .sort();
    return entries.join("|");
  }, [providers, settings]);

  function clearTimer(providerId: ProviderId) {
    const timer = timersRef.current.get(providerId);
    if (timer) {
      clearInterval(timer);
      timersRef.current.delete(providerId);
    }
  }

  function clearAllTimers() {
    for (const providerId of timersRef.current.keys()) {
      clearTimer(providerId);
    }
  }

  function syncTimers() {
    clearAllTimers();

    if (!shouldRunRef.current) {
      setIsActive(false);
      return;
    }

    const enabledProviders = useProviderStore.getState().providers.filter((provider) => provider.enabled);
    const currentSettings = useSettingsStore.getState().settings;

    for (const provider of enabledProviders) {
      const pollingSettings = getEffectivePollingSettings(
        currentSettings,
        provider.providerId,
      );
      const intervalMs = getPollingIntervalMs(pollingSettings);

      if (intervalMs === null) {
        continue;
      }

      const providerId = provider.providerId;
      timersRef.current.set(
        providerId,
        setInterval(() => {
          void useProviderStore.getState().refreshProvider(providerId);
        }, intervalMs),
      );
    }

    setIsActive(timersRef.current.size > 0);
  }

  function start() {
    shouldRunRef.current = true;
    syncTimers();
  }

  function stop() {
    shouldRunRef.current = false;
    clearAllTimers();
    setIsActive(false);
  }

  // 只在调度指纹（供应商集合 + 生效策略）变化时重建定时器；
  // 不再依赖 providers 数组引用或单个设置字段
  useEffect(() => {
    if (shouldRunRef.current) {
      syncTimers();
    }
  }, [pollingScheduleFingerprint]);

  // 组件卸载时停止调度并清理全部定时器
  useEffect(() => {
    return () => {
      shouldRunRef.current = false;
      clearAllTimers();
    };
  }, []);

  return { isActive, start, stop };
}
