import { useI18n } from "../../i18n";
import { getWindowLabel } from "../../i18n/windowLabels";
import type { ExtraUsage, SubscriptionUsageSummary } from "../../types/provider";
import { cn } from "@/lib/utils";

/** 用量状态色阈值：与 utils/formatters 的 getUsageColor 保持一致（<60 正常 / 60-85 警告 / >85 危险） */
export function usageFillClass(percent: number): string {
  if (percent < 60) return "bg-success";
  if (percent < 85) return "bg-warning";
  return "bg-danger";
}

type UsageBarProps = {
  percent: number;
  /** md = 6px 主进度条（大数字区），sm = 4px 明细细条 */
  size?: "md" | "sm";
  showLabel?: boolean;
  /** 百分比标签的附加类名（精简模式用更小字号与宽度） */
  labelClassName?: string;
};

/** 通用用量进度条：轨道走 --color-progress-track token，填充按状态色，百分比等宽数字右对齐 */
export function UsageBar({ percent, size = "md", showLabel = true, labelClassName }: UsageBarProps) {
  const clamped = Math.max(0, Math.min(100, percent));

  return (
    <div className="flex min-w-0 flex-1 items-center gap-2">
      <div
        className={cn(
          "flex-1 overflow-hidden rounded-full bg-progress-track",
          size === "md" ? "h-1.5" : "h-1",
        )}
      >
        <div
          className={cn("h-full rounded-full transition-[width] duration-300", usageFillClass(clamped))}
          style={{ width: `${clamped}%` }}
        />
      </div>
      {showLabel && (
        <span
          className={cn(
            "w-9 shrink-0 text-right text-[12px] tabular-nums text-text-muted",
            labelClassName,
          )}
        >
          {Math.round(clamped)}%
        </span>
      )}
    </div>
  );
}

type SubscriptionBadgeProps = {
  subscription: SubscriptionUsageSummary;
};

export default function SubscriptionBadge({ subscription }: SubscriptionBadgeProps) {
  const { t, language } = useI18n();
  const usage = subscription.usage;
  const planLabel = usage.planName ?? t("widget.subscription.fallbackPlan");

  function formatResetTime(isoStr: string): string {
    const reset = new Date(isoStr);
    const diffMs = reset.getTime() - Date.now();
    if (diffMs <= 0) return t("widget.subscription.resetSoon");
    const diffMin = Math.floor(diffMs / 60000);
    if (diffMin < 60) return t("widget.subscription.resetInMinutes", { count: diffMin });
    const diffHr = Math.floor(diffMin / 60);
    if (diffHr < 24) return t("widget.subscription.resetInHours", { count: diffHr });
    return t("widget.subscription.resetInDays", { count: Math.floor(diffHr / 24) });
  }

  /** 订阅窗口明细行：左侧固定宽标签（标签 + 第二行小字），右侧 4px 细进度条 + 百分比 */
  function renderWindowRow(
    key: string,
    label: string,
    percent: number,
    secondaryText: string | null,
    secondaryTitle?: string,
  ) {
    return (
      <div key={key} className="flex items-center gap-2">
        <div className="flex w-[92px] shrink-0 flex-col">
          <span className="truncate text-[12px] leading-[1.35] text-text-secondary" title={label}>
            {label}
          </span>
          {secondaryText && (
            <span className="truncate text-[9px] leading-[1.35] text-text-muted" title={secondaryTitle}>
              {secondaryText}
            </span>
          )}
        </div>
        <UsageBar percent={percent} size="sm" />
      </div>
    );
  }

  function renderExtraUsage(extra: ExtraUsage) {
    if (!extra.isEnabled) return null;
    const label = t("widget.subscription.extraUsageLabel");
    if (extra.monthlyLimitUsd === null) {
      return (
        <div key="extra-usage" className="flex items-center gap-2">
          <div className="flex w-[92px] shrink-0 flex-col">
            <span className="truncate text-[12px] leading-[1.35] text-text-secondary" title={label}>
              {label}
            </span>
          </div>
          <span className="text-[10px] text-text-muted">{t("widget.subscription.extraUsageUnlimited")}</span>
        </div>
      );
    }

    const utilization = extra.utilization ?? 0;
    const usedStr = extra.usedUsd != null ? extra.usedUsd.toFixed(2) : "0.00";
    const limitStr = extra.monthlyLimitUsd.toFixed(2);
    const resetText = extra.resetsAt ? formatResetTime(extra.resetsAt) : t("widget.subscription.extraUsageResetsMonthly");
    return renderWindowRow(
      "extra-usage",
      label,
      utilization,
      `${t("widget.subscription.extraUsageSpent", { used: usedStr, limit: limitStr })} · ${resetText}`,
      extra.resetsAt ?? undefined,
    );
  }

  return (
    <div className="flex flex-col gap-1.5 rounded-lg border border-white/6 bg-white/3 px-2.5 py-2">
      <div className="flex min-w-0 items-baseline justify-between gap-2">
        <span className="min-w-0 truncate text-[12px] font-semibold text-foreground" title={subscription.subscriptionName}>
          {subscription.subscriptionName}
        </span>
        <span className="shrink-0 truncate text-[10px] text-text-muted" title={planLabel}>
          {planLabel}
        </span>
      </div>

      {usage.status === "success" && usage.windows.length > 0 && (
        <div className="flex flex-col gap-1.5">
          {usage.windows.map((win, index) => renderWindowRow(
            `${subscription.subscriptionId}-${win.label}-${index}`,
            getWindowLabel(win.label, language),
            win.utilization,
            win.resetsAt ? formatResetTime(win.resetsAt) : null,
            win.resetsAt ?? undefined,
          ))}
          {usage.extraUsage && renderExtraUsage(usage.extraUsage)}
        </div>
      )}

      {usage.status === "success" && usage.windows.length === 0 && usage.extraUsage?.isEnabled && (
        renderExtraUsage(usage.extraUsage)
      )}

      {usage.status === "error" && (
        <div className="text-[12px] leading-snug text-error">{usage.errorMessage}</div>
      )}
    </div>
  );
}
