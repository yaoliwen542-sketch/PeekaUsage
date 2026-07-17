import { useEffect, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow, LogicalPosition } from "@tauri-apps/api/window";
import ProviderIcon from "../common/ProviderIcon";
import type { UsageSummary } from "../../types/provider";
import { useProviderStore } from "../../stores/providerStore";
import { useSettingsStore } from "../../stores/settingsStore";
import { useI18n } from "../../i18n";

const ISLAND_POSITION_KEY = "peekausage.island.position";

/** 读取持久化的灵动岛位置 */
function loadSavedPosition(): { x: number; y: number } | null {
  try {
    const raw = localStorage.getItem(ISLAND_POSITION_KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw);
    if (typeof parsed.x === "number" && typeof parsed.y === "number") {
      return parsed;
    }
  } catch {
    // 忽略解析失败
  }
  return null;
}

/** 持久化灵动岛位置 */
function savePosition(x: number, y: number) {
  try {
    localStorage.setItem(ISLAND_POSITION_KEY, JSON.stringify({ x, y }));
  } catch {
    // 忽略存储失败
  }
}

/** 计算供应商最高利用率 */
function getProviderUtil(s: UsageSummary): number {
  let util = 0;
  if (s.usage && s.usage.totalBudget && s.usage.totalBudget > 0) {
    util = Math.max(util, (s.usage.totalUsed / s.usage.totalBudget) * 100);
  }
  for (const sub of s.subscriptions) {
    for (const w of sub.usage.windows) {
      util = Math.max(util, w.utilization);
    }
  }
  return util;
}

/** 获取供应商剩余额度文本 */
function getRemainingText(s: UsageSummary): string {
  if (s.usage) {
    const remaining = s.usage.remaining;
    const currency = s.usage.currency;
    if (remaining !== null && remaining !== undefined) {
      return `${remaining.toFixed(2)} ${currency}`;
    }
  }
  const util = getProviderUtil(s);
  return `${Math.round(util)}%`;
}

/**
 * 灵动岛组件
 *
 * 交互：
 * - 收起态：小胶囊，显示最高用量供应商图标 + 百分比
 * - 点击展开：显示所有供应商摘要 + 刷新按钮 + 供应商详情 + 快捷设置
 * - 可拖动（data-tauri-drag-region）
 * - 位置持久化（localStorage）
 */
export default function IslandWidget() {
  const [summaries, setSummaries] = useState<UsageSummary[]>([]);
  const [expanded, setExpanded] = useState(false);
  const [expandedProvider, setExpandedProvider] = useState<string | null>(null);
  const [showQuickSettings, setShowQuickSettings] = useState(false);
  const [dragging, setDragging] = useState(false);
  const { t } = useI18n();
  const refreshAll = useProviderStore((s) => s.refreshAll);
  const isRefreshing = useProviderStore((s) => s.isRefreshing);
  const settings = useSettingsStore((s) => s.settings);
  const saveSettings = useSettingsStore((s) => s.saveSettings);

  // 启动时恢复持久化的位置
  useEffect(() => {
    const saved = loadSavedPosition();
    if (saved) {
      const win = getCurrentWindow();
      win.setPosition(new LogicalPosition(saved.x, saved.y)).catch(() => {});
    }
  }, []);

  useEffect(() => {
    let unlisten: UnlistenFn | null = null;
    listen<UsageSummary[]>("island-usage-update", (event) => {
      setSummaries(event.payload);
    }).then((fn) => {
      unlisten = fn;
    });
    return () => {
      unlisten?.();
    };
  }, []);

  const enabledSummaries = summaries.filter((s) => s.enabled);
  const topProvider = enabledSummaries
    .map((s) => ({ summary: s, util: getProviderUtil(s) }))
    .sort((a, b) => b.util - a.util)[0];

  // 拖动逻辑
  async function handleDragStart(e: React.MouseEvent) {
    if (e.button !== 0) return;
    // 展开状态下不拖动（允许内部交互）
    if (expanded) return;
    e.preventDefault();
    setDragging(true);

    const win = getCurrentWindow();
    const startX = e.screenX;
    const startY = e.screenY;
    const startPos = await win.outerPosition();

    const onMove = async (ev: MouseEvent) => {
      const dx = ev.screenX - startX;
      const dy = ev.screenY - startY;
      const newX = startPos.x + dx;
      const newY = startPos.y + dy;
      await win.setPosition(new LogicalPosition(newX, newY));
    };

    const onUp = async () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
      setDragging(false);
      try {
        const finalPos = await win.outerPosition();
        savePosition(finalPos.x, finalPos.y);
      } catch {
        // 忽略
      }
    };

    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
  }

  function handleClick() {
    if (dragging) return;
    setExpanded(!expanded);
    setExpandedProvider(null);
    setShowQuickSettings(false);
  }

  // 收起态：没有数据时显示占位
  if (summaries.length === 0) {
    return (
      <div
        className="flex h-9 items-center gap-2 rounded-full border border-border/50 bg-surface/95 px-3 shadow-md cursor-pointer select-none backdrop-blur-md"
        data-tauri-drag-region
        onMouseDown={handleDragStart}
        onClick={handleClick}
        style={{ WebkitAppRegion: "drag" } as React.CSSProperties}
      >
        <div className="h-2 w-2 rounded-full bg-primary animate-pulse" />
        <span className="text-xs font-medium text-text-secondary">PeekaUsage</span>
      </div>
    );
  }

  // 展开态
  if (expanded) {
    return (
      <div className="flex w-72 flex-col gap-2 rounded-2xl border border-border/50 bg-surface/98 p-3 shadow-xl backdrop-blur-md">
        {/* 顶部栏：标题 + 刷新 + 设置 + 收起 */}
        <div className="flex items-center justify-between" data-tauri-drag-region>
          <span className="text-xs font-semibold text-text">用量监控</span>
          <div className="flex items-center gap-1">
            <button
              className="flex h-6 w-6 items-center justify-center rounded-md text-text-tertiary transition-colors hover:bg-surface-elevated hover:text-text"
              onClick={() => void refreshAll()}
              disabled={isRefreshing}
              title={t("common.refresh")}
            >
              <svg width="12" height="12" viewBox="0 0 12 12" fill="none" className={isRefreshing ? "animate-spin" : ""}>
                <path d="M6 1.5A4.5 4.5 0 1 1 1.5 6" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
                <path d="M6 1.5L8 3.5L6 5.5" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
              </svg>
            </button>
            <button
              className={`flex h-6 w-6 items-center justify-center rounded-md transition-colors hover:bg-surface-elevated hover:text-text ${showQuickSettings ? "text-primary bg-primary/10" : "text-text-tertiary"}`}
              onClick={() => { setShowQuickSettings(!showQuickSettings); setExpandedProvider(null); }}
              title="快捷设置"
            >
              <svg width="12" height="12" viewBox="0 0 12 12" fill="none">
                <circle cx="6" cy="6" r="1.5" stroke="currentColor" strokeWidth="1.2" />
                <path d="M6 1v1.5M6 9.5V11M11 6H9.5M2.5 6H1M9.5 9.5l-1-1M3.5 3.5l1 1M9.5 2.5l-1 1M3.5 8.5l1-1" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" />
              </svg>
            </button>
            <button
              className="flex h-6 w-6 items-center justify-center rounded-md text-text-tertiary transition-colors hover:bg-surface-elevated hover:text-text"
              onClick={() => { setExpanded(false); setExpandedProvider(null); setShowQuickSettings(false); }}
              title="收起"
            >
              <svg width="10" height="10" viewBox="0 0 10 10" fill="none">
                <path d="M2.5 3.5L5 6l2.5-2.5" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
              </svg>
            </button>
          </div>
        </div>

        {/* 快捷设置面板 */}
        {showQuickSettings && (
          <div className="flex flex-col gap-2 rounded-md border border-border bg-surface-elevated p-2">
            {/* 透明度 */}
            <div className="flex items-center gap-2">
              <span className="text-[11px] text-text-secondary w-12">透明度</span>
              <input
                type="range"
                min="10"
                max="100"
                value={settings.windowOpacity}
                onChange={(e) => void saveSettings({ windowOpacity: Number(e.target.value) })}
                className="h-1 flex-1 cursor-pointer appearance-none rounded-full bg-border"
              />
              <span className="text-[11px] text-text-tertiary w-8 text-right">{settings.windowOpacity}%</span>
            </div>
            {/* 刷新间隔 */}
            <div className="flex items-center gap-2">
              <span className="text-[11px] text-text-secondary w-12">刷新</span>
              {settings.pollingMode === "manual" ? (
                <span className="text-[11px] text-text-tertiary flex-1">仅手动</span>
              ) : (
                <>
                  <input
                    type="number"
                    min="1"
                    max="1440"
                    value={settings.pollingInterval}
                    onChange={(e) => void saveSettings({ pollingInterval: Number(e.target.value) })}
                    className="h-6 w-12 rounded border border-border bg-background px-1 text-[11px] text-text"
                  />
                  <span className="text-[11px] text-text-tertiary">{settings.pollingUnit === "minutes" ? "分" : "秒"}</span>
                </>
              )}
            </div>
            {/* 主题 */}
            <div className="flex items-center gap-2">
              <span className="text-[11px] text-text-secondary w-12">主题</span>
              <div className="flex gap-1">
                {(["light", "dark", "system"] as const).map((mode) => (
                  <button
                    key={mode}
                    className={`h-6 px-2 rounded text-[11px] transition-colors ${settings.theme === mode ? "bg-primary text-primary-foreground" : "bg-surface text-text-tertiary hover:text-text"}`}
                    onClick={() => void saveSettings({ theme: mode })}
                  >
                    {mode === "light" ? "浅色" : mode === "dark" ? "深色" : "系统"}
                  </button>
                ))}
              </div>
            </div>
          </div>
        )}

        {/* 供应商列表 */}
        <div className="flex flex-col gap-1">
          {enabledSummaries.map((s) => {
            const util = getProviderUtil(s);
            const isExpanded = expandedProvider === s.providerId;
            return (
              <div key={s.providerId} className="rounded-md">
                <button
                  className="flex w-full items-center gap-2 px-1.5 py-1.5 rounded-md transition-colors hover:bg-surface-elevated text-left"
                  onClick={() => setExpandedProvider(isExpanded ? null : s.providerId)}
                >
                  <ProviderIcon providerId={s.providerId} size={16} />
                  <span className="text-xs text-text flex-1 truncate">{s.displayName}</span>
                  <div className="h-1.5 w-14 rounded-full bg-border overflow-hidden">
                    <div
                      className="h-full rounded-full transition-all"
                      style={{
                        width: `${Math.min(util, 100)}%`,
                        backgroundColor: util > 80 ? "var(--color-error)" : util > 50 ? "var(--color-warning)" : "var(--color-success)",
                      }}
                    />
                  </div>
                  <span className="text-[11px] text-text-secondary w-12 text-right">{getRemainingText(s)}</span>
                </button>
                {/* 供应商详情展开 */}
                {isExpanded && (
                  <div className="flex flex-col gap-1 px-2 py-1.5 ml-5 border-l border-border">
                    {s.usage && (
                      <div className="flex justify-between text-[10px] text-text-tertiary">
                        <span>已用 {s.usage.totalUsed.toFixed(2)} {s.usage.currency}</span>
                        {s.usage.totalBudget !== null && (
                          <span>总额 {s.usage.totalBudget.toFixed(2)} {s.usage.currency}</span>
                        )}
                      </div>
                    )}
                    {s.subscriptions.map((sub) => (
                      <div key={sub.subscriptionId} className="flex flex-col gap-0.5">
                        <span className="text-[10px] text-text-tertiary">{sub.subscriptionName}</span>
                        {sub.usage.windows.map((w) => (
                          <div key={w.label} className="flex justify-between text-[10px] text-text-secondary">
                            <span>{w.label}</span>
                            <span>{Math.round(w.utilization)}%</span>
                          </div>
                        ))}
                      </div>
                    ))}
                    {s.rateLimit && (
                      <div className="text-[10px] text-text-tertiary">
                        {s.rateLimit.requestsPerMinute && `${s.rateLimit.requestsPerMinute}/${s.rateLimit.requestsPerMinuteLimit} RPM`}
                      </div>
                    )}
                    {s.errorMessage && (
                      <div className="text-[10px] text-error">{s.errorMessage}</div>
                    )}
                  </div>
                )}
              </div>
            );
          })}
        </div>
      </div>
    );
  }

  // 收起态：显示最高用量供应商
  return (
    <div
      className="flex h-9 items-center gap-2 rounded-full border border-border/50 bg-surface/95 px-3 shadow-md cursor-pointer select-none backdrop-blur-md"
      data-tauri-drag-region
      onMouseDown={handleDragStart}
      onClick={handleClick}
      style={{ WebkitAppRegion: "drag" } as React.CSSProperties}
    >
      {topProvider ? (
        <>
          <ProviderIcon providerId={topProvider.summary.providerId} size={16} />
          <span className="text-xs font-semibold text-text">{Math.round(topProvider.util)}%</span>
          {enabledSummaries.length > 1 && (
            <span className="text-[10px] text-text-tertiary">+{enabledSummaries.length - 1}</span>
          )}
        </>
      ) : (
        <span className="text-xs text-text-secondary">无数据</span>
      )}
    </div>
  );
}
