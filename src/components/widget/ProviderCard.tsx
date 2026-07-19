import type { CSSProperties } from "react";
import { useI18n } from "../../i18n";
import { getWindowLabel } from "../../i18n/windowLabels";
import type { ApiKeyUsageSummary, UsageSummary } from "../../types/provider";
import type { WidgetDisplayMode } from "../../types/settings";
import { calcUsagePercent, formatCurrency } from "../../utils/formatters";
import { cn } from "@/lib/utils";
import ProviderIcon from "../common/ProviderIcon";
import RateLimitBadge from "./RateLimitBadge";
import SubscriptionBadge from "./SubscriptionBadge";
import UsageProgressBar from "./UsageProgressBar";

type ProviderCardProps = {
  provider: UsageSummary;
  displayMode?: WidgetDisplayMode;
  useCompactColorMarkers?: boolean;
  isRefreshing?: boolean;
  onRefresh: () => void;
};

/** 紧凑模式指标行的通用样式（网格布局：标签 + 进度条） */
const COMPACT_METRIC_ROW_CLASS = "grid grid-cols-[minmax(0,auto)_minmax(0,1fr)] items-center gap-2";

export default function ProviderCard({
  provider,
  displayMode = "detailed",
  useCompactColorMarkers = false,
  isRefreshing = false,
  onRefresh,
}: ProviderCardProps) {
  const { t, language } = useI18n();
  const hasSubscription = provider.subscriptions.length > 0;
  const hasApiUsage = provider.apiKeyUsages.length > 0;
  const hasMultipleApiKeys = provider.apiKeyUsages.length > 1;
  const compactApiItems = provider.apiKeyUsages.filter((item) => item.usage);
  const compactApiErrors = provider.apiKeyUsages.filter((item) => item.errorMessage);
  const compactSubscriptionErrors = provider.subscriptions.filter((item) => item.usage.status === "error" && item.usage.errorMessage);
  const compactVisibleSubscriptions = provider.subscriptions.filter((subscription) => {
    if (subscription.usage.status !== "success") {
      return false;
    }

    const extra = subscription.usage.extraUsage;
    const hasExtra = !!extra?.isEnabled && extra.monthlyLimitUsd !== null && extra.utilization != null;
    return subscription.usage.windows.length > 0 || hasExtra;
  });
  const useSubscriptionColorMarkers = useCompactColorMarkers && compactVisibleSubscriptions.length > 1;
  const useApiColorMarkers = useCompactColorMarkers && compactApiItems.length > 1;
  const isCompact = displayMode === "compact";

  function usagePercent(item: ApiKeyUsageSummary) {
    if (!item.usage) {
      return 0;
    }

    return calcUsagePercent(item.usage.totalUsed, item.usage.totalBudget);
  }

  // 注意：保留 provider-card 类名作为拖拽状态钩子（widget.css 的 card-shell 规则会给它加阴影/边框）
  return (
    <div
      className={cn(
        "provider-card flex flex-col gap-2 rounded-md border border-border bg-surface p-3 transition-colors hover:border-border-hover",
        provider.status === "error" && "border-danger/30",
        isCompact && "gap-1.5 px-2.5 py-[9px]",
      )}
    >
      <div className="flex items-center justify-between gap-2">
        <div className="flex min-w-0 items-center gap-2">
          <ProviderIcon providerId={provider.providerId} size={isCompact ? 16 : 20} />
          <span className={cn("font-semibold leading-[1.1] text-foreground", isCompact ? "text-[11px]" : "text-[13px]")}>
            {provider.displayName}
          </span>
        </div>
        <button
          className={cn(
            "inline-flex shrink-0 items-center justify-center rounded-full border border-transparent p-0",
            "cursor-pointer text-foreground-secondary transition-colors",
            "hover:border-border hover:bg-ghost-hover hover:text-foreground",
            "focus-visible:border-primary-soft-border focus-visible:shadow-[0_0_0_3px_var(--color-primary-soft-bg)] focus-visible:outline-none",
            "disabled:cursor-not-allowed disabled:opacity-55",
            isCompact ? "h-5 w-5 [&_svg]:size-3" : "h-[26px] w-[26px] [&_svg]:size-3.5",
            isRefreshing && "[&_svg]:animate-spin",
          )}
          disabled={isRefreshing}
          type="button"
          title={t("widget.actions.refreshProvider")}
          aria-label={t("widget.actions.refreshProvider")}
          onPointerDown={(event) => event.stopPropagation()}
          onClick={(event) => {
            event.stopPropagation();
            onRefresh();
          }}
        >
          <svg viewBox="0 0 24 24" aria-hidden="true">
            <path
              d="M20 12a8 8 0 1 1-2.34-5.66"
              fill="none"
              stroke="currentColor"
              strokeLinecap="round"
              strokeWidth="1.8"
            />
            <path
              d="M20 5.5v5h-5"
              fill="none"
              stroke="currentColor"
              strokeLinecap="round"
              strokeLinejoin="round"
              strokeWidth="1.8"
            />
          </svg>
        </button>
      </div>

      {isCompact ? (
        <>
          {(provider.subscriptions.some((item) => item.usage.status === "success" && item.usage.windows.length > 0) || compactApiItems.length > 0) ? (
            <div className="flex flex-col gap-1">
              {provider.subscriptions.map((subscription) => {
                if (subscription.usage.status !== "success") {
                  return null;
                }

                const extra = subscription.usage.extraUsage;
                const hasExtra = !!extra?.isEnabled && extra.monthlyLimitUsd !== null && extra.utilization != null;
                const hasWindows = subscription.usage.windows.length > 0;

                if (!hasWindows && !hasExtra) {
                  return null;
                }

                return (
                  <div
                    key={subscription.subscriptionId}
                    className={cn(
                      "flex flex-col gap-1 rounded-sm border border-muted-surface-border bg-muted-surface px-2 py-1.5",
                      useSubscriptionColorMarkers && cn(
                        "relative pl-3.5",
                        "before:absolute before:top-1.5 before:bottom-1.5 before:left-1.5 before:w-1 before:rounded-full",
                        "before:bg-(--compact-marker-color) before:content-['']",
                      ),
                    )}
                    style={{ "--compact-marker-color": subscription.color } as CSSProperties}
                  >
                    {!useSubscriptionColorMarkers && (
                      <div className="text-[10px] leading-[1.2] font-semibold text-foreground-secondary" title={subscription.subscriptionName}>
                        {subscription.subscriptionName}
                      </div>
                    )}
                    {subscription.usage.windows.map((window, index) => (
                      <div key={`${subscription.subscriptionId}-${window.label}-${index}`} className={COMPACT_METRIC_ROW_CLASS}>
                        <span className="max-w-[72px] truncate text-[10px] font-semibold text-foreground-secondary" title={getWindowLabel(window.label, language)}>
                          {formatCompactSubscriptionWindowLabel(getWindowLabel(window.label, language), t("widget.providerCard.subscriptionShort"))}
                        </span>
                        <div className="min-w-0 [&_.progress-container]:gap-1.5 [&_.progress-label]:min-w-7 [&_.progress-label]:text-[10px]">
                          <UsageProgressBar percent={window.utilization} />
                        </div>
                      </div>
                    ))}
                    {hasExtra && (
                      <div className={COMPACT_METRIC_ROW_CLASS}>
                        <span className="max-w-[72px] truncate text-[10px] font-semibold text-foreground-secondary" title={t("widget.subscription.extraUsageLabel")}>
                          {formatCompactSubscriptionWindowLabel(t("widget.subscription.extraUsageLabel"), t("widget.providerCard.subscriptionShort"))}
                        </span>
                        <div className="min-w-0 [&_.progress-container]:gap-1.5 [&_.progress-label]:min-w-7 [&_.progress-label]:text-[10px]">
                          <UsageProgressBar percent={extra!.utilization!} />
                        </div>
                      </div>
                    )}
                  </div>
                );
              })}

              {compactApiItems.map((item) => (
                <div
                  key={item.keyId}
                  className={cn(
                    COMPACT_METRIC_ROW_CLASS,
                    useApiColorMarkers && "grid-cols-[4px_minmax(0,1fr)] gap-1.5",
                  )}
                  style={useApiColorMarkers ? ({ "--compact-marker-color": item.color } as CSSProperties) : undefined}
                >
                  {useApiColorMarkers ? (
                    <>
                      <span className="h-[18px] w-1 rounded-full bg-(--compact-marker-color)" aria-hidden="true" />
                      <div className="min-w-0 [&_.progress-container]:gap-1.5 [&_.progress-label]:min-w-7 [&_.progress-label]:text-[10px]">
                        <UsageProgressBar percent={usagePercent(item)} />
                      </div>
                    </>
                  ) : (
                    <>
                      <span className="max-w-[72px] truncate text-[10px] font-semibold text-foreground-secondary" title={item.keyName}>
                        {formatCompactApiLabel(item.keyName, t("widget.providerCard.apiShort"))}
                      </span>
                      <div className="min-w-0 [&_.progress-container]:gap-1.5 [&_.progress-label]:min-w-7 [&_.progress-label]:text-[10px]">
                        <UsageProgressBar percent={usagePercent(item)} />
                      </div>
                    </>
                  )}
                </div>
              ))}
            </div>
          ) : null}

          {compactSubscriptionErrors.map((item) => (
            <div key={`${item.subscriptionId}-error`} className="text-[10px] text-danger">
              {item.subscriptionName}: {item.usage.errorMessage}
            </div>
          ))}

          {compactApiErrors.map((item) => (
            <div key={`${item.keyId}-error`} className="text-[10px] text-danger">
              {hasMultipleApiKeys ? `${item.keyName}: ${item.errorMessage}` : item.errorMessage}
            </div>
          ))}

          {provider.status === "error" && !hasSubscription && !hasApiUsage && provider.errorMessage && (
            <div className="text-[10px] text-danger">{provider.errorMessage}</div>
          )}
        </>
      ) : (
        <>
          {provider.subscriptions.map((subscription) => (
            <SubscriptionBadge key={subscription.subscriptionId} subscription={subscription} />
          ))}

          {hasApiUsage && (
            <div className="flex flex-col gap-2 border-t border-dashed border-border pt-1">
              <div className="flex flex-col gap-1">
                {hasSubscription && (
                  <div className="text-[10px] font-semibold uppercase tracking-[0.3px] text-foreground-muted">
                    {t("widget.providerCard.apiLabel")}
                  </div>
                )}
                {provider.usage && (
                  <div className="flex flex-wrap items-center gap-2">
                    <span className="text-[10px] text-foreground-muted">
                      {hasMultipleApiKeys ? t("widget.providerCard.total") : t("widget.providerCard.current")}
                    </span>
                    <span className="text-[13px] font-bold text-primary-hover">
                      {formatCurrency(provider.usage.totalUsed, provider.usage.currency)}
                    </span>
                    {provider.usage.remaining != null && (
                      <span className="text-[11px] text-foreground-secondary">
                        {t("widget.providerCard.balance")}: {formatCurrency(provider.usage.remaining, provider.usage.currency)}
                      </span>
                    )}
                  </div>
                )}
              </div>

              {provider.apiKeyUsages.map((item) => (
                <div
                  key={item.keyId}
                  className={cn(
                    "flex flex-col gap-1 rounded-sm bg-usage-item p-2",
                    item.status === "error" && "bg-usage-item-error",
                  )}
                >
                  <div className="flex items-center justify-between gap-2">
                    <span className="text-[11px] font-semibold text-foreground">{item.keyName}</span>
                    {item.usage && (
                      <span className="text-[13px] font-bold text-primary-hover">
                        {formatCurrency(item.usage.totalUsed, item.usage.currency)}
                      </span>
                    )}
                  </div>

                  {item.usage && (
                    <div className="flex items-center justify-between gap-2">
                      {item.usage.remaining != null && (
                        <span className="text-[11px] text-foreground-secondary">
                          {t("widget.providerCard.balance")}: {formatCurrency(item.usage.remaining, item.usage.currency)}
                        </span>
                      )}
                    </div>
                  )}

                  {item.usage?.totalBudget && (
                    <UsageProgressBar percent={usagePercent(item)} />
                  )}

                  {item.errorMessage && (
                    <div className="text-[10px] text-danger">{item.errorMessage}</div>
                  )}

                  {item.rateLimit && provider.apiKeyUsages.length === 1 && (
                    <RateLimitBadge rateLimit={item.rateLimit} />
                  )}
                </div>
              ))}
            </div>
          )}
        </>
      )}

      {!isCompact && provider.status === "error" && !hasSubscription && !hasApiUsage && (
        <div className="text-[10px] text-danger">{provider.errorMessage}</div>
      )}
    </div>
  );
}

function formatCompactSubscriptionWindowLabel(label: string, fallback: string): string {
  const trimmedLabel = label.trim();
  if (!trimmedLabel) {
    return fallback;
  }

  return trimmedLabel;
}

function formatCompactApiLabel(keyName: string, apiShort: string): string {
  const trimmed = keyName.trim();
  return trimmed || apiShort;
}
