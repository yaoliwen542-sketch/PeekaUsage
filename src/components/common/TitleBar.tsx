import type { MouseEvent } from "react";
import { useI18n } from "../../i18n";
import { useWindowControls } from "../../composables/useWindowControls";
import { cn } from "@/lib/utils";

type TitleBarProps = {
  onDragIntentStart?: () => void;
};

export default function TitleBar({ onDragIntentStart }: TitleBarProps) {
  const { minimizeWindow, closeToTray } = useWindowControls();
  const { t } = useI18n();

  function handleMouseDown(event: MouseEvent<HTMLDivElement>) {
    if (event.button !== 0) {
      return;
    }

    const target = event.target;
    if (target instanceof Element && target.closest("button")) {
      return;
    }

    onDragIntentStart?.();
  }

  // 注意：保留 titlebar 类名作为 JS 测量钩子（WidgetContainer 用它读取标题栏高度）
  return (
    <div
      className="titlebar flex h-8 shrink-0 items-center justify-between border-b border-border bg-titlebar px-2"
      data-tauri-drag-region
      onMouseDown={handleMouseDown}
    >
      <div className="flex items-center gap-2" data-tauri-drag-region>
        <span className="h-2 w-2 rounded-full bg-primary" />
        <span
          className="text-xs font-semibold tracking-[0.3px] text-foreground-secondary"
          data-tauri-drag-region
        >
          PeekaUsage
        </span>
      </div>
      <div className="flex gap-0.5">
        <button
          className={cn(
            "flex h-6 w-6 items-center justify-center rounded-sm text-foreground-muted",
            "transition-colors hover:bg-ghost-hover hover:text-foreground",
          )}
          onClick={() => void minimizeWindow()}
          title={t("titleBar.minimize")}
        >
          <svg width="10" height="10" viewBox="0 0 10 10">
            <rect x="0" y="4" width="10" height="1.5" fill="currentColor" rx="0.5" />
          </svg>
        </button>
        <button
          className={cn(
            "flex h-6 w-6 items-center justify-center rounded-sm text-foreground-muted",
            "transition-colors hover:bg-danger hover:text-white",
          )}
          onClick={() => void closeToTray()}
          title={t("titleBar.hideToTray")}
        >
          <svg width="10" height="10" viewBox="0 0 10 10">
            <path d="M1 1L9 9M9 1L1 9" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
          </svg>
        </button>
      </div>
    </div>
  );
}
