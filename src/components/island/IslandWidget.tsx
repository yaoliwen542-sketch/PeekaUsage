import { useEffect, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import ProviderIcon from "../common/ProviderIcon";
import type { UsageSummary } from "../../types/provider";

/**
 * 灵动岛组件
 *
 * 形态：
 * - 收起态：小胶囊，显示最高用量供应商图标 + 百分比
 * - 悬停展开：显示所有启用供应商的摘要（图标 + 名称 + 百分比 + 进度条）
 * - 可拖动（data-tauri-drag-region）
 * - 通过 Tauri event "island-usage-update" 接收主窗口的用量数据
 */
export default function IslandWidget() {
  const [summaries, setSummaries] = useState<UsageSummary[]>([]);
  const [hovered, setHovered] = useState(false);

  useEffect(() => {
    let unlisten: UnlistenFn | null = null;

    // 监听主窗口发来的用量更新
    listen<UsageSummary[]>("island-usage-update", (event) => {
      setSummaries(event.payload);
    }).then((fn) => {
      unlisten = fn;
    });

    return () => {
      unlisten?.();
    };
  }, []);

  // 找到用量最高的供应商（用于收起态显示）
  const topProvider = summaries
    .filter((s) => s.usage || s.subscriptions.length > 0)
    .map((s) => {
      // 计算最高利用率
      let maxUtil = 0;
      if (s.usage && s.usage.totalBudget) {
        maxUtil = Math.max(maxUtil, (s.usage.totalUsed / s.usage.totalBudget) * 100);
      }
      for (const sub of s.subscriptions) {
        for (const w of sub.usage.windows) {
          maxUtil = Math.max(maxUtil, w.utilization);
        }
      }
      return { summary: s, util: maxUtil };
    })
    .sort((a, b) => b.util - a.util)[0];

  const enabledCount = summaries.filter((s) => s.enabled).length;

  // 拖动逻辑
  async function handleDragStart(e: React.MouseEvent) {
    if (e.button !== 0) return;
    e.preventDefault();

    const win = getCurrentWindow();
    const startX = e.screenX;
    const startY = e.screenY;
    const startPos = await win.outerPosition();

    const onMove = async (ev: MouseEvent) => {
      const dx = ev.screenX - startX;
      const dy = ev.screenY - startY;
      const newX = startPos.x + dx;
      const newY = startPos.y + dy;
      await win.setPosition(new (await import("@tauri-apps/api/window")).LogicalPosition(newX, newY));
    };

    const onUp = () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    };

    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
  }

  // 收起态：没有数据时显示占位
  if (summaries.length === 0) {
    return (
      <div
        className="island-collapsed flex h-10 items-center gap-2 rounded-full border border-border/50 bg-surface/90 px-3 backdrop-blur-md"
        data-tauri-drag-region
        onMouseDown={handleDragStart}
        onMouseEnter={() => setHovered(true)}
        onMouseLeave={() => setHovered(false)}
        style={{ WebkitAppRegion: "drag" } as React.CSSProperties}
      >
        <div className="h-2 w-2 rounded-full bg-primary animate-pulse" />
        <span className="text-xs font-medium text-text-secondary">PeekaUsage</span>
      </div>
    );
  }

  // 展开态
  if (hovered && summaries.length > 0) {
    return (
      <div
        className="island-expanded flex flex-col gap-1 rounded-2xl border border-border/50 bg-surface/95 p-2 backdrop-blur-md shadow-lg"
        data-tauri-drag-region
        onMouseDown={handleDragStart}
        onMouseEnter={() => setHovered(true)}
        onMouseLeave={() => setHovered(false)}
        style={{ WebkitAppRegion: "drag" } as React.CSSProperties}
      >
        {summaries
          .filter((s) => s.enabled)
          .map((s) => {
            let util = 0;
            if (s.usage && s.usage.totalBudget) {
              util = (s.usage.totalUsed / s.usage.totalBudget) * 100;
            }
            for (const sub of s.subscriptions) {
              for (const w of sub.usage.windows) {
                util = Math.max(util, w.utilization);
              }
            }
            return (
              <div key={s.providerId} className="flex items-center gap-2 px-1 py-0.5">
                <ProviderIcon providerId={s.providerId} size={16} />
                <span className="text-xs text-text flex-1 truncate">{s.displayName}</span>
                <div className="h-1.5 w-12 rounded-full bg-border overflow-hidden">
                  <div
                    className="h-full rounded-full transition-all"
                    style={{
                      width: `${Math.min(util, 100)}%`,
                      backgroundColor: util > 80 ? "var(--color-error)" : util > 50 ? "var(--color-warning)" : "var(--color-success)",
                    }}
                  />
                </div>
                <span className="text-xs text-text-secondary w-8 text-right">{Math.round(util)}%</span>
              </div>
            );
          })}
      </div>
    );
  }

  // 收起态：显示最高用量供应商
  return (
    <div
      className="island-collapsed flex h-10 items-center gap-2 rounded-full border border-border/50 bg-surface/90 px-3 backdrop-blur-md"
      data-tauri-drag-region
      onMouseDown={handleDragStart}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
      style={{ WebkitAppRegion: "drag" } as React.CSSProperties}
    >
      {topProvider ? (
        <>
          <ProviderIcon providerId={topProvider.summary.providerId} size={16} />
          <span className="text-xs font-semibold text-text">{Math.round(topProvider.util)}%</span>
          {enabledCount > 1 && (
            <span className="text-xs text-text-tertiary">+{enabledCount - 1}</span>
          )}
        </>
      ) : (
        <span className="text-xs text-text-secondary">无数据</span>
      )}
    </div>
  );
}
