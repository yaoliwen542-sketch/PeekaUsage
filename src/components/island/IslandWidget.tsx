import { useEffect, useRef, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  currentMonitor,
  getCurrentWindow,
  LogicalPosition,
  LogicalSize,
} from "@tauri-apps/api/window";
import ProviderIcon from "../common/ProviderIcon";
import { usageFillClass } from "../widget/SubscriptionBadge";
import type { UsageSummary } from "../../types/provider";
import {
  MAX_POLLING_INTERVAL,
  MIN_POLLING_INTERVAL,
  normalizePollingInterval,
  type PollingMode,
  type PollingUnit,
} from "../../types/settings";
import { useProviderStore } from "../../stores/providerStore";
import {
  SETTINGS_CHANGED_EVENT,
  useSettingsStore,
  type SettingsChangedPayload,
} from "../../stores/settingsStore";
import { useWindowControls } from "../../composables/useWindowControls";
import { useI18n } from "../../i18n";
import { getWindowLabel } from "../../i18n/windowLabels";
import { applyTheme, observeSystemTheme } from "../../utils/theme";
import { toLogicalWindowPosition } from "../../utils/windowBounds";
import { cn } from "@/lib/utils";

const ISLAND_POSITION_KEY = "peekausage.island.position";

/** 窗口尺寸：与 tauri.conf.json 中 island 窗口的 200x40 保持一致 */
const COLLAPSED_WIDTH = 200;
const COLLAPSED_HEIGHT = 40;
/** 展开态窗口尺寸：宽度略大于面板内容，高度覆盖快捷设置 + 供应商列表 */
const EXPANDED_WIDTH = 300;
const EXPANDED_HEIGHT = 400;

/** 拖动判定阈值：mousedown 后移动超过该距离则视为拖动，松手后不触发展开 */
const DRAG_CLICK_SUPPRESS_PX = 5;
/** 拖动结束后 click 抑制的兜底时间窗（覆盖 OS 拖动吞掉 mousemove 的情况） */
const DRAG_CLICK_SUPPRESS_MS = 250;
/** 拖动结束后的位置持久化防抖 */
const POSITION_SAVE_DEBOUNCE_MS = 300;
/** 恢复位置失效时的回退位置：屏幕工作区顶部居中，距顶 12px */
const FALLBACK_TOP_MARGIN = 12;
/** 收起态岛条内平铺供应商数上限：超过后切换为横向轮播 */
const COLLAPSED_TILE_LIMIT = 2;

type SavedIslandPosition = { x: number; y: number };

/** 读取持久化的灵动岛位置（逻辑像素） */
function loadSavedPosition(): SavedIslandPosition | null {
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

/** 持久化灵动岛位置（逻辑像素） */
function savePosition(x: number, y: number) {
  try {
    localStorage.setItem(ISLAND_POSITION_KEY, JSON.stringify({ x, y }));
  } catch {
    // 忽略存储失败
  }
}

function clamp(value: number, min: number, max: number) {
  return Math.min(Math.max(value, min), Math.max(min, max));
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

/** 数字状态色：阈值与主界面 usageFillClass 一致（<60 正常 / 60-85 警告 / ≥85 危险） */
function usageTextClass(percent: number): string {
  if (percent < 60) return "text-success";
  if (percent < 85) return "text-warning";
  return "text-danger";
}

/**
 * 启动时恢复灵动岛位置。
 * 参考主窗口 windowBounds 的 normalize 思路：基于当前显示器工作区校验，
 * 保存的位置可见面积不足一半（例如副屏已拔掉）时回退到工作区顶部居中；
 * 位置有效但贴边越界时夹取回工作区内。
 */
async function restoreIslandPosition() {
  const saved = loadSavedPosition();
  if (!saved || !Number.isFinite(saved.x) || !Number.isFinite(saved.y)) {
    return;
  }

  const win = getCurrentWindow();
  try {
    const monitor = await currentMonitor();
    if (!monitor) {
      return;
    }
    const scale = monitor.scaleFactor;
    const areaPos = monitor.workArea.position.toLogical(scale);
    const areaSize = monitor.workArea.size.toLogical(scale);
    const area = {
      x: Math.round(areaPos.x),
      y: Math.round(areaPos.y),
      width: Math.round(areaSize.width),
      height: Math.round(areaSize.height),
    };

    const overlapX = Math.min(saved.x + COLLAPSED_WIDTH, area.x + area.width) - Math.max(saved.x, area.x);
    const overlapY = Math.min(saved.y + COLLAPSED_HEIGHT, area.y + area.height) - Math.max(saved.y, area.y);
    const hasEnoughVisible = overlapX >= COLLAPSED_WIDTH / 2 && overlapY >= COLLAPSED_HEIGHT / 2;

    if (!hasEnoughVisible) {
      const fallbackX = area.x + Math.round((area.width - COLLAPSED_WIDTH) / 2);
      await win.setPosition(new LogicalPosition(fallbackX, area.y + FALLBACK_TOP_MARGIN));
      return;
    }

    const x = clamp(Math.round(saved.x), area.x, area.x + area.width - COLLAPSED_WIDTH);
    const y = clamp(Math.round(saved.y), area.y, area.y + area.height - COLLAPSED_HEIGHT);
    await win.setPosition(new LogicalPosition(x, y));
  } catch {
    // 恢复失败时保持系统默认位置
  }
}

/**
 * 灵动岛组件
 *
 * 交互：
 * - 收起态：圆角胶囊，平铺 / 轮播各供应商摘要（图标 + 状态色利用率 + 迷你进度条）
 * - 点击展开：窗口扩为 300x400，显示所有供应商摘要 + 刷新 + 供应商详情 + 快捷设置
 * - 收起态拖动走 Tauri startDragging（OS 级拖动，避免物理/逻辑像素混用与 IPC 风暴）
 * - 位置持久化（localStorage，逻辑像素，启动时做离屏校验）
 */
export default function IslandWidget() {
  const [summaries, setSummaries] = useState<UsageSummary[]>([]);
  const [expanded, setExpanded] = useState(false);
  const [expandedProvider, setExpandedProvider] = useState<string | null>(null);
  const [showQuickSettings, setShowQuickSettings] = useState(false);
  const { t, language } = useI18n();
  const refreshAll = useProviderStore((s) => s.refreshAll);
  const isRefreshing = useProviderStore((s) => s.isRefreshing);
  const settingsLoaded = useSettingsStore((s) => s.loaded);
  const theme = useSettingsStore((s) => s.settings.theme);
  const windowOpacity = useSettingsStore((s) => s.settings.windowOpacity);
  const loadSettings = useSettingsStore((s) => s.loadSettings);
  const { applyOpacity } = useWindowControls();

  // mousedown 起点与"已发生拖动"标记，用于抑制拖动后的误触 click
  const dragStartClientRef = useRef<{ x: number; y: number } | null>(null);
  const dragMovedRef = useRef(false);
  const lastWindowMoveAtRef = useRef(0);
  const positionSaveTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // 启动：加载用户设置 + 恢复位置 + 注册各类监听
  useEffect(() => {
    let active = true;
    let unlistenUsage: UnlistenFn | null = null;
    let unlistenSettingsChanged: UnlistenFn | null = null;
    let unlistenMoved: UnlistenFn | null = null;
    let stopObservingSystemTheme: (() => void) | null = null;
    const win = getCurrentWindow();
    const windowLabel = win.label;

    void (async () => {
      // 先加载用户设置，加载完成前快捷设置区展示加载态，
      // 避免岛内基于 DEFAULT_SETTINGS 的误保存把用户配置重置成默认值
      await loadSettings();
      if (!active) {
        return;
      }

      await restoreIslandPosition();

      unlistenUsage = await listen<UsageSummary[]>("island-usage-update", (event) => {
        setSummaries(event.payload);
      });

      // 跨窗口设置同步：忽略自己发出的事件，避免回环
      unlistenSettingsChanged = await listen<SettingsChangedPayload>(SETTINGS_CHANGED_EVENT, (event) => {
        if (event.payload.source === windowLabel) {
          return;
        }
        useSettingsStore.getState().applySyncedSettings(event.payload.settings);
      });

      // 窗口移动：标记拖动（抑制随后的 click）并防抖持久化逻辑像素位置
      unlistenMoved = await win.onMoved(({ payload }) => {
        dragMovedRef.current = true;
        lastWindowMoveAtRef.current = Date.now();
        if (positionSaveTimerRef.current) {
          clearTimeout(positionSaveTimerRef.current);
        }
        positionSaveTimerRef.current = setTimeout(() => {
          positionSaveTimerRef.current = null;
          void (async () => {
            try {
              const scale = await win.scaleFactor();
              const logical = toLogicalWindowPosition(payload, scale);
              // toLogicalWindowPosition 已过滤隐藏/最小化时的离屏哨兵坐标
              if (logical) {
                savePosition(logical.x, logical.y);
              }
            } catch {
              // 忽略持久化失败
            }
          })();
        }, POSITION_SAVE_DEBOUNCE_MS);
      });

      stopObservingSystemTheme = observeSystemTheme(() => {
        if (useSettingsStore.getState().settings.theme === "system") {
          applyTheme("system");
        }
      });
    })();

    return () => {
      active = false;
      unlistenUsage?.();
      unlistenSettingsChanged?.();
      unlistenMoved?.();
      stopObservingSystemTheme?.();
      if (positionSaveTimerRef.current) {
        clearTimeout(positionSaveTimerRef.current);
        positionSaveTimerRef.current = null;
      }
    };
  }, [loadSettings]);

  // 主题即时生效（含其他窗口同步过来的变更）；加载完成前不动，避免闪默认主题
  useEffect(() => {
    if (!settingsLoaded) {
      return;
    }
    applyTheme(theme);
  }, [settingsLoaded, theme]);

  // 透明度即时生效（含其他窗口同步过来的变更）
  useEffect(() => {
    if (!settingsLoaded) {
      return;
    }
    void applyOpacity(windowOpacity);
  }, [settingsLoaded, windowOpacity, applyOpacity]);

  /** 展开 / 收起时同步调整窗口尺寸（setSize 不受 resizable:false 限制） */
  async function applyWindowSize(nextExpanded: boolean) {
    const win = getCurrentWindow();
    try {
      if (nextExpanded) {
        await win.setSize(new LogicalSize(EXPANDED_WIDTH, EXPANDED_HEIGHT));
      } else {
        await win.setSize(new LogicalSize(COLLAPSED_WIDTH, COLLAPSED_HEIGHT));
      }
    } catch {
      // 窗口权限未就绪等场景忽略，面板仍按收起态展示
    }
  }

  function setExpandedWithSize(nextExpanded: boolean) {
    setExpanded(nextExpanded);
    setExpandedProvider(null);
    setShowQuickSettings(false);
    void applyWindowSize(nextExpanded);
  }

  // 收起态岛条：mousedown 时交给 Tauri/OS 拖动窗口
  function handleBarMouseDown(e: React.MouseEvent) {
    if (e.button !== 0 || expanded) {
      return;
    }
    dragStartClientRef.current = { x: e.clientX, y: e.clientY };
    dragMovedRef.current = false;
    void getCurrentWindow().startDragging().catch(() => {
      // 拖动权限未就绪时忽略，点击展开仍然可用
    });
  }

  // 部分平台 OS 拖动前仍会派发 mousemove，超过阈值则标记为拖动
  function handleBarMouseMove(e: React.MouseEvent) {
    const start = dragStartClientRef.current;
    if (!start || dragMovedRef.current) {
      return;
    }
    if (
      Math.abs(e.clientX - start.x) > DRAG_CLICK_SUPPRESS_PX
      || Math.abs(e.clientY - start.y) > DRAG_CLICK_SUPPRESS_PX
    ) {
      dragMovedRef.current = true;
    }
  }

  function handleBarMouseUp() {
    dragStartClientRef.current = null;
  }

  function handleBarClick() {
    // 发生过拖动（或刚刚发生过窗口移动）时不展开，避免拖完松手误触
    if (dragMovedRef.current || Date.now() - lastWindowMoveAtRef.current < DRAG_CLICK_SUPPRESS_MS) {
      dragMovedRef.current = false;
      return;
    }
    setExpandedWithSize(!expanded);
  }

  const enabledSummaries = summaries.filter((s) => s.enabled);

  // 展开态
  if (expanded) {
    return (
      <div className="island-panel flex h-full w-full flex-col overflow-hidden rounded-xl border border-white/6 bg-card shadow-xl backdrop-blur-md">
        {/* 顶部栏：标题 + 刷新 + 设置 + 收起（展开面板区域不挂拖动） */}
        <div className="flex h-10 shrink-0 items-center justify-between px-3">
          <span className="text-[13px] font-semibold text-foreground">{t("island.title")}</span>
          <div className="flex items-center gap-1">
            <button
              type="button"
              className={cn(
                "flex h-7 w-7 items-center justify-center rounded-md text-text-secondary transition-colors duration-150",
                "hover:bg-white/8 hover:text-text",
                "focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent/60",
                "disabled:cursor-not-allowed disabled:opacity-50",
              )}
              onClick={() => void refreshAll()}
              disabled={isRefreshing}
              title={t("island.refresh")}
              aria-label={t("island.refresh")}
            >
              <svg width="12" height="12" viewBox="0 0 24 24" fill="none" className={isRefreshing ? "animate-spin" : ""} aria-hidden="true">
                <path d="M20 12a8 8 0 1 1-2.34-5.66" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" />
                <path d="M20 5.5v5h-5" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" />
              </svg>
            </button>
            <button
              type="button"
              className={cn(
                "flex h-7 w-7 items-center justify-center rounded-md transition-colors duration-150",
                "hover:bg-white/8 hover:text-text",
                "focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent/60",
                showQuickSettings ? "bg-white/10 text-foreground" : "text-text-secondary",
              )}
              onClick={() => { setShowQuickSettings(!showQuickSettings); setExpandedProvider(null); }}
              title={t("island.quickSettings")}
              aria-label={t("island.quickSettings")}
              aria-pressed={showQuickSettings}
            >
              <svg width="12" height="12" viewBox="0 0 12 12" fill="none" aria-hidden="true">
                <circle cx="6" cy="6" r="1.5" stroke="currentColor" strokeWidth="1.2" />
                <path d="M6 1v1.5M6 9.5V11M11 6H9.5M2.5 6H1M9.5 9.5l-1-1M3.5 3.5l1 1M9.5 2.5l-1 1M3.5 8.5l1-1" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" />
              </svg>
            </button>
            <button
              type="button"
              className={cn(
                "flex h-7 w-7 items-center justify-center rounded-md text-text-secondary transition-colors duration-150",
                "hover:bg-white/8 hover:text-text",
                "focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent/60",
              )}
              onClick={() => setExpandedWithSize(false)}
              title={t("island.collapse")}
              aria-label={t("island.collapse")}
            >
              <svg width="10" height="10" viewBox="0 0 10 10" fill="none" aria-hidden="true">
                <path d="M2.5 3.5L5 6l2.5-2.5" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
              </svg>
            </button>
          </div>
        </div>

        {/* 供应商列表：紧凑版主界面卡片（图标 + 名称 + 状态色数字 + 4px 进度条） */}
        <div className="island-scroll min-h-0 flex-1 divide-y divide-white/5 overflow-y-auto">
          {enabledSummaries.length === 0 && (
            <div className="flex h-full items-center justify-center text-[11px] text-text-tertiary">
              {t("island.noData")}
            </div>
          )}
          {enabledSummaries.map((s) => {
            const util = clamp(getProviderUtil(s), 0, 100);
            const isExpanded = expandedProvider === s.providerId;
            return (
              <div key={s.providerId}>
                <button
                  type="button"
                  className={cn(
                    "flex w-full flex-col gap-1.5 px-3 py-2.5 text-left transition-colors duration-150",
                    "hover:bg-white/4 focus-visible:outline-none focus-visible:bg-white/6",
                  )}
                  onClick={() => setExpandedProvider(isExpanded ? null : s.providerId)}
                  aria-expanded={isExpanded}
                >
                  <div className="flex items-center gap-2">
                    <ProviderIcon providerId={s.providerId} size={16} />
                    <span className="min-w-0 flex-1 truncate text-[12px] font-medium text-foreground" title={s.displayName}>
                      {s.displayName}
                    </span>
                    <span className={cn("shrink-0 text-[13px] font-semibold tabular-nums", usageTextClass(util))}>
                      {getRemainingText(s)}
                    </span>
                  </div>
                  <div className="h-1 w-full overflow-hidden rounded-full bg-progress-track">
                    <div
                      className={cn("h-full rounded-full transition-[width] duration-300", usageFillClass(util))}
                      style={{ width: `${util}%` }}
                    />
                  </div>
                </button>
                {/* 供应商详情展开 */}
                {isExpanded && (
                  <div className="mx-3 mb-2 flex flex-col gap-1 border-l border-white/10 pl-3">
                    {s.usage && (
                      <div className="flex justify-between text-[10px] text-text-muted">
                        <span>{t("island.usageUsed", { used: s.usage.totalUsed.toFixed(2), currency: s.usage.currency })}</span>
                        {s.usage.totalBudget !== null && (
                          <span>{t("island.usageTotal", { total: s.usage.totalBudget.toFixed(2), currency: s.usage.currency })}</span>
                        )}
                      </div>
                    )}
                    {s.subscriptions.map((sub) => (
                      <div key={sub.subscriptionId} className="flex flex-col gap-0.5">
                        <span className="text-[10px] text-text-muted">{sub.subscriptionName}</span>
                        {sub.usage.windows.map((w) => (
                          <div key={w.label} className="flex justify-between text-[10px] text-text-secondary">
                            <span>{getWindowLabel(w.label, language)}</span>
                            <span className="tabular-nums">{Math.round(w.utilization)}%</span>
                          </div>
                        ))}
                      </div>
                    ))}
                    {s.rateLimit && (
                      <div className="text-[10px] text-text-muted">
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

        {/* 快捷设置（底部固定）：设置未加载完成时显示加载态，避免基于默认值误保存 */}
        {showQuickSettings && (
          <div className="shrink-0 border-t border-white/6 px-3 py-2">
            {settingsLoaded ? (
              <IslandQuickSettings />
            ) : (
              <div className="flex items-center justify-center py-3 text-[11px] text-text-tertiary">
                {t("island.loading")}
              </div>
            )}
          </div>
        )}
      </div>
    );
  }

  // 收起态：没有数据时显示占位胶囊
  if (summaries.length === 0) {
    return (
      <div
        className="flex h-full w-full cursor-pointer select-none items-center gap-2 overflow-hidden rounded-full border border-white/8 bg-card/90 px-3 backdrop-blur-md transition-colors duration-150 hover:border-white/16"
        onMouseDown={handleBarMouseDown}
        onMouseMove={handleBarMouseMove}
        onMouseUp={handleBarMouseUp}
        onClick={handleBarClick}
      >
        <div className="h-2 w-2 shrink-0 animate-pulse rounded-full bg-primary" />
        <span className="truncate text-[12px] font-medium text-text-secondary">PeekaUsage</span>
      </div>
    );
  }

  // 收起态：平铺或横向轮播各供应商摘要
  const useMarquee = enabledSummaries.length > COLLAPSED_TILE_LIMIT;
  // 轮播时复制一份实现无缝循环；第二份对辅助技术隐藏
  const marqueeItems = useMarquee ? [...enabledSummaries, ...enabledSummaries] : enabledSummaries;

  return (
    <div
      className="flex h-full w-full cursor-pointer select-none items-center overflow-hidden rounded-full border border-white/8 bg-card/90 px-3 backdrop-blur-md transition-colors duration-150 hover:border-white/16"
      onMouseDown={handleBarMouseDown}
      onMouseMove={handleBarMouseMove}
      onMouseUp={handleBarMouseUp}
      onClick={handleBarClick}
    >
      {enabledSummaries.length === 0 ? (
        <span className="truncate text-[12px] text-text-secondary">{t("island.noData")}</span>
      ) : (
        <div
          className={cn("flex w-max items-center", useMarquee && "island-marquee-track")}
          style={useMarquee ? { animationDuration: `${enabledSummaries.length * 4}s` } : undefined}
        >
          {marqueeItems.map((s, index) => (
            <CollapsedProviderItem
              key={`${s.providerId}-${index}`}
              summary={s}
              ariaHidden={useMarquee && index >= enabledSummaries.length}
            />
          ))}
        </div>
      )}
    </div>
  );
}

/** 收起态单个供应商摘要：14px 图标 + 状态色利用率 + 4px 迷你进度条 */
function CollapsedProviderItem({ summary, ariaHidden }: { summary: UsageSummary; ariaHidden?: boolean }) {
  const util = clamp(getProviderUtil(summary), 0, 100);
  return (
    // pr-4 放在条目上而不是容器 gap 上，保证轮播两份拷贝宽度严格一致、循环无跳动
    <div className="flex shrink-0 items-center gap-1.5 pr-4" aria-hidden={ariaHidden || undefined}>
      <ProviderIcon providerId={summary.providerId} size={14} />
      <span className={cn("text-[12px] font-semibold leading-none tabular-nums", usageTextClass(util))}>
        {Math.round(util)}%
      </span>
      <div className="h-1 w-8 overflow-hidden rounded-full bg-progress-track">
        <div
          className={cn("h-full rounded-full transition-[width] duration-300", usageFillClass(util))}
          style={{ width: `${util}%` }}
        />
      </div>
    </div>
  );
}

/** 岛内向分段控件：与设置页子导航一致的 segmented pill 风格（小尺寸版） */
function IslandSegmented<T extends string>(props: {
  options: Array<{ value: T; label: string }>;
  value: T;
  ariaLabel: string;
  onChange: (value: T) => void;
}) {
  const { options, value, ariaLabel, onChange } = props;
  return (
    <div className="inline-flex shrink-0 gap-0.5 rounded-full bg-white/5 p-0.5" role="group" aria-label={ariaLabel}>
      {options.map((option) => {
        const isActive = option.value === value;
        return (
          <button
            key={option.value}
            type="button"
            aria-pressed={isActive}
            className={cn(
              "flex h-6 items-center justify-center whitespace-nowrap rounded-full px-2.5 text-[11px] transition-colors duration-150",
              "focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-white/30",
              isActive ? "bg-white/10 font-medium text-foreground" : "text-text-secondary hover:text-text",
            )}
            onClick={() => onChange(option.value)}
          >
            {option.label}
          </button>
        );
      })}
    </div>
  );
}

/**
 * 灵动岛快捷设置面板。
 * 独立组件以便每次打开时从 store 初始化草稿态：
 * - 透明度滑杆拖动中只本地预览（应用 CSS），松手后才 saveSettings 写盘
 * - 刷新间隔输入框防抖 500ms 或失焦时保存
 */
function IslandQuickSettings() {
  const { t } = useI18n();
  const settings = useSettingsStore((s) => s.settings);
  const saveSettings = useSettingsStore((s) => s.saveSettings);
  const { applyOpacity } = useWindowControls();

  // 透明度草稿：非 null 表示正在拖动预览，松手后回写 null 并保存
  const [opacityDraft, setOpacityDraft] = useState<number | null>(null);
  // 刷新间隔草稿：受控输入，防抖保存
  const [intervalDraft, setIntervalDraft] = useState(() => String(settings.pollingInterval));
  const intervalSaveTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const intervalDraftRef = useRef(intervalDraft);

  const opacityValue = opacityDraft ?? settings.windowOpacity;

  const pollingModeOptions: Array<{ value: PollingMode; label: string }> = [
    { value: "auto", label: t("settings.polling.auto") },
    { value: "manual", label: t("settings.polling.manual") },
  ];
  const pollingUnitOptions: Array<{ value: PollingUnit; label: string }> = [
    { value: "seconds", label: t("common.secondsShort") },
    { value: "minutes", label: t("common.minutesShort") },
  ];

  function clearIntervalSaveTimer() {
    if (intervalSaveTimerRef.current) {
      clearTimeout(intervalSaveTimerRef.current);
      intervalSaveTimerRef.current = null;
    }
  }

  /** 拖动中：只更新草稿并应用 CSS 预览，不写盘 */
  function handleOpacityInput(e: React.ChangeEvent<HTMLInputElement>) {
    const value = Number(e.target.value);
    setOpacityDraft(value);
    void applyOpacity(value);
  }

  /** 松手 / 失焦：把预览值正式保存 */
  function finalizeOpacity() {
    if (opacityDraft === null) {
      return;
    }
    const value = opacityDraft;
    setOpacityDraft(null);
    if (value !== useSettingsStore.getState().settings.windowOpacity) {
      void saveSettings({ windowOpacity: value });
    }
  }

  function persistIntervalDraft(raw: string) {
    const trimmed = raw.trim();
    if (trimmed === "") {
      // 清空输入不保存，回显当前已保存值
      const current = useSettingsStore.getState().settings.pollingInterval;
      intervalDraftRef.current = String(current);
      setIntervalDraft(String(current));
      return;
    }
    const value = Number(trimmed);
    if (!Number.isFinite(value)) {
      return;
    }
    const normalized = normalizePollingInterval(value);
    if (normalized !== useSettingsStore.getState().settings.pollingInterval) {
      void saveSettings({ pollingInterval: normalized });
    }
    // 回显归一化后的值（例如超出 1..999 会被夹取）
    intervalDraftRef.current = String(normalized);
    setIntervalDraft(String(normalized));
  }

  function handleIntervalChange(e: React.ChangeEvent<HTMLInputElement>) {
    const raw = e.target.value;
    intervalDraftRef.current = raw;
    setIntervalDraft(raw);
    clearIntervalSaveTimer();
    intervalSaveTimerRef.current = setTimeout(() => {
      intervalSaveTimerRef.current = null;
      persistIntervalDraft(intervalDraftRef.current);
    }, 500);
  }

  function handleIntervalBlur() {
    clearIntervalSaveTimer();
    persistIntervalDraft(intervalDraftRef.current);
  }

  // 面板关闭时：若有防抖中的输入未落盘，立即保存
  useEffect(() => () => {
    if (intervalSaveTimerRef.current) {
      clearTimeout(intervalSaveTimerRef.current);
      intervalSaveTimerRef.current = null;
      persistIntervalDraft(intervalDraftRef.current);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return (
    <div className="flex flex-col gap-2">
      {/* 分区标题 */}
      <div className="text-[11px] font-semibold uppercase tracking-wide text-text-tertiary">
        {t("island.quickSettings")}
      </div>

      {/* 透明度 */}
      <div className="flex items-center gap-2">
        <span className="w-10 shrink-0 text-[11px] text-text-secondary">{t("island.opacity")}</span>
        <input
          type="range"
          min="10"
          max="100"
          value={opacityValue}
          onChange={handleOpacityInput}
          onPointerUp={finalizeOpacity}
          onKeyUp={finalizeOpacity}
          onBlur={finalizeOpacity}
          className="opacity-slider h-1.5 min-w-0 flex-1 cursor-pointer appearance-none rounded-full bg-border"
          aria-label={t("island.opacity")}
        />
        <span className="w-9 shrink-0 text-right text-[11px] tabular-nums text-text-secondary">{opacityValue}%</span>
      </div>

      {/* 刷新：模式分段 + 数值输入 + 单位分段（数值输入保留原有防抖 / 失焦保存） */}
      <div className="flex flex-wrap items-center gap-x-2 gap-y-1.5">
        <span className="w-10 shrink-0 text-[11px] text-text-secondary">{t("island.refreshInterval")}</span>
        <IslandSegmented
          options={pollingModeOptions}
          value={settings.pollingMode}
          ariaLabel={t("settings.polling.modeAriaLabel")}
          onChange={(value) => void saveSettings({ pollingMode: value })}
        />
        {settings.pollingMode !== "manual" && (
          <>
            <input
              type="number"
              min={MIN_POLLING_INTERVAL}
              max={MAX_POLLING_INTERVAL}
              value={intervalDraft}
              onChange={handleIntervalChange}
              onBlur={handleIntervalBlur}
              aria-label={t("settings.polling.intervalAriaLabel")}
              className={cn(
                "h-6 w-12 rounded-md border border-white/10 bg-white/5 px-1.5 text-[11px] tabular-nums text-foreground",
                "outline-none transition-colors focus:border-white/20",
              )}
            />
            <IslandSegmented
              options={pollingUnitOptions}
              value={settings.pollingUnit}
              ariaLabel={t("settings.polling.unitAriaLabel")}
              onChange={(value) => void saveSettings({ pollingUnit: value })}
            />
          </>
        )}
      </div>

      {/* 主题 */}
      <div className="flex items-center gap-2">
        <span className="w-10 shrink-0 text-[11px] text-text-secondary">{t("island.theme")}</span>
        <IslandSegmented
          options={(["light", "dark", "system"] as const).map((mode) => ({
            value: mode,
            label: mode === "light" ? t("island.themeLight") : mode === "dark" ? t("island.themeDark") : t("island.themeSystem"),
          }))}
          value={settings.theme}
          ariaLabel={t("island.theme")}
          onChange={(mode) => void saveSettings({ theme: mode })}
        />
      </div>
    </div>
  );
}
