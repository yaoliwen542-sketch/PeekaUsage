import type { CSSProperties } from "react";
import { useI18n } from "../../i18n";
import { getWindowLabel } from "../../i18n/windowLabels";
import type { ApiKeyUsageSummary, UsageSummary } from "../../types/provider";
import type { WidgetDisplayMode } from "../../types/settings";
import { calcUsagePercent, formatCurrency } from "../../utils/formatters";
import { cn } from "@/lib/utils";
import ProviderIcon from "../common/ProviderIcon";
import RateLimitBadge from "./RateLimitBadge";
import SubscriptionBadge, { UsageBar } from "./SubscriptionBadge";

type ProviderCardProps = {
  provider: UsageSummary;
  displayMode?: WidgetDisplayMode;
  useCompactColorMarkers?: boolean;
  isRefreshing?: boolean;
  onRefresh: () => void;
};

/** 大数字区（Hero）模型：百分比型（订阅利用率 / CodingPlan）或余额型 */
type HeroModel =
  | { kind: "percent"; percent: number; caption: string }
  | { kind: "balance"; value: string; caption: string; subline: string | null; percent: number | null };

/** API Key 行状态圆点颜色：正常 / 异常 / 加载中 / 空闲 */
function statusDotClass(item: ApiKeyUsageSummary): string {
  if (item.status === "error") return "bg-danger";
  if (item.status === "loading") return "bg-warning";
  if (item.status === "success") return "bg-success";
  return "bg-white/20";
}

/** 单个 Key 的利用率：CodingPlan 百分比型直接取 totalUsed，余额型按预算折算 */
function keyUsagePercent(item: ApiKeyUsageSummary): number {
  if (!item.usage) return 0;
  if (item.usage.currency === "%") return item.usage.totalUsed;
  return calcUsagePercent(item.usage.totalUsed, item.usage.totalBudget);
}

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
  const isCompact = displayMode === "compact";
  const isProviderErrorOnly = provider.status === "error" && !hasSubscription && !hasApiUsage;

  // ===== 精简模式数据准备（逻辑与旧版一致） =====
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

  /** 构建大数字区：订阅利用率 > 余额 > 已用金额 > 单 Key 回退 */
  function buildHero(): HeroModel | null {
    let best: { percent: number; caption: string } | null = null;
    const successfulSubs = provider.subscriptions.filter((item) => item.usage.status === "success");

    for (const subscription of successfulSubs) {
      const planSuffix = subscription.usage.planName ? ` · ${subscription.usage.planName}` : "";
      const nameSuffix = successfulSubs.length > 1 ? ` · ${subscription.subscriptionName}` : planSuffix;

      for (const win of subscription.usage.windows) {
        const caption = `${getWindowLabel(win.label, language)}${nameSuffix}`;
        if (!best || win.utilization > best.percent) {
          best = { percent: win.utilization, caption };
        }
      }

      const extra = subscription.usage.extraUsage;
      if (extra?.isEnabled && extra.monthlyLimitUsd !== null && extra.utilization != null) {
        const caption = `${t("widget.subscription.extraUsageLabel")}${nameSuffix}`;
        if (!best || extra.utilization > best.percent) {
          best = { percent: extra.utilization, caption };
        }
      }
    }

    if (best) {
      return { kind: "percent", percent: best.percent, caption: best.caption };
    }

    const aggregate = provider.usage ?? provider.apiKeyUsages.find((item) => item.usage)?.usage ?? null;
    if (!aggregate) {
      return null;
    }

    const fallbackKeyName = hasMultipleApiKeys
      ? t("widget.providerCard.apiLabel")
      : (provider.apiKeyUsages.find((item) => item.usage)?.keyName ?? t("widget.providerCard.apiShort"));

    if (aggregate.currency === "%") {
      // CodingPlan 百分比型：大数字即利用率
      return { kind: "percent", percent: aggregate.totalUsed, caption: fallbackKeyName };
    }

    const usedText = `${t("widget.providerCard.total")} ${formatCurrency(aggregate.totalUsed, aggregate.currency)}`;
    const budgetText = aggregate.totalBudget != null
      ? ` · ${t("widget.providerCard.budget")} ${formatCurrency(aggregate.totalBudget, aggregate.currency)}`
      : "";
    const percent = aggregate.totalBudget != null
      ? calcUsagePercent(aggregate.totalUsed, aggregate.totalBudget)
      : null;

    if (aggregate.remaining != null) {
      return {
        kind: "balance",
        value: formatCurrency(aggregate.remaining, aggregate.currency),
        caption: t("widget.providerCard.balance"),
        subline: `${usedText}${budgetText}`,
        percent,
      };
    }

    return {
      kind: "balance",
      value: formatCurrency(aggregate.totalUsed, aggregate.currency),
      caption: hasMultipleApiKeys ? t("widget.providerCard.total") : t("widget.providerCard.current"),
      subline: aggregate.totalBudget != null ? budgetText.slice(3) : null,
      percent,
    };
  }

  /** 大数字区：余额 / 利用率是视觉主角 */
  function renderHero(hero: HeroModel) {
    if (hero.kind === "percent") {
      return (
        <div className="flex flex-col gap-1.5">
          <div className="flex items-end justify-between gap-2">
            <span className="text-[20px] font-bold leading-6 tabular-nums text-foreground">
              {Math.round(hero.percent)}%
            </span>
            <span className="min-w-0 truncate pb-px text-[12px] text-text-muted" title={hero.caption}>
              {hero.caption}
            </span>
          </div>
          <UsageBar percent={hero.percent} size="md" showLabel={false} />
        </div>
      );
    }

    return (
      <div className="flex flex-col gap-1">
        <div className="flex items-end justify-between gap-2">
          <span className="min-w-0 truncate text-[20px] font-bold leading-6 tabular-nums text-foreground" title={hero.value}>
            {hero.value}
          </span>
          <span className="shrink-0 pb-px text-[12px] text-text-muted">{hero.caption}</span>
        </div>
        {hero.subline && (
          <div className="truncate text-[11px] tabular-nums text-text-muted" title={hero.subline}>
            {hero.subline}
          </div>
        )}
        {hero.percent != null && <UsageBar percent={hero.percent} size="md" showLabel={false} />}
      </div>
    );
  }

  /** 详细模式：单个 API Key 行（状态圆点 + 名称 + 金额/百分比 + 细进度条） */
  function renderApiKeyRow(item: ApiKeyUsageSummary) {
    const usage = item.usage;
    const isPercent = usage?.currency === "%";
    const percent = usage ? keyUsagePercent(item) : null;
    const showBar = !!usage && (isPercent || usage.totalBudget != null);
    const mainValue = usage
      ? (isPercent ? `${Math.round(percent ?? 0)}%` : formatCurrency(usage.totalUsed, usage.currency))
      : null;

    return (
      <div key={item.keyId} className="flex flex-col gap-1 rounded-md bg-white/3 px-2 py-1.5">
        <div className="flex items-center gap-2">
          <span className={cn("h-1.5 w-1.5 shrink-0 rounded-full", statusDotClass(item))} aria-hidden="true" />
          <span className="min-w-0 flex-1 truncate text-[12px] font-medium text-foreground" title={item.keyName}>
            {item.keyName}
          </span>
          {mainValue && (
            <span className="shrink-0 text-[12px] font-semibold tabular-nums text-foreground">{mainValue}</span>
          )}
        </div>

        {usage && !isPercent && usage.remaining != null && (
          <div className="truncate pl-3.5 text-[11px] tabular-nums text-text-secondary">
            {t("widget.providerCard.balance")} {formatCurrency(usage.remaining, usage.currency)}
          </div>
        )}

        {showBar && (
          <div className="pl-3.5">
            <UsageBar percent={percent ?? 0} size="sm" />
          </div>
        )}

        {item.errorMessage && (
          <div className="pl-3.5 text-[12px] leading-snug text-error">{item.errorMessage}</div>
        )}

        {item.rateLimit && provider.apiKeyUsages.length === 1 && (
          <div className="pl-3.5">
            <RateLimitBadge rateLimit={item.rateLimit} />
          </div>
        )}
      </div>
    );
  }

  /** 详细模式：按量 API 区块（多 Key 时含合计行） */
  function renderApiSection() {
    const aggregate = provider.usage;

    return (
      <div className={cn("flex flex-col gap-1.5", hasSubscription && "border-t border-white/6 pt-2")}>
        {hasSubscription && (
          <div className="text-[10px] font-semibold uppercase tracking-[0.3px] text-text-muted">
            {t("widget.providerCard.apiLabel")}
          </div>
        )}

        {hasMultipleApiKeys && aggregate && (
          <div className="flex items-center justify-between gap-2">
            <span className="shrink-0 text-[11px] text-text-muted">{t("widget.providerCard.total")}</span>
            <div className="flex min-w-0 items-baseline gap-2">
              <span className="truncate text-[12px] font-semibold tabular-nums text-foreground">
                {formatCurrency(aggregate.totalUsed, aggregate.currency)}
              </span>
              {aggregate.remaining != null && (
                <span className="truncate text-[11px] tabular-nums text-text-secondary">
                  {t("widget.providerCard.balance")} {formatCurrency(aggregate.remaining, aggregate.currency)}
                </span>
              )}
            </div>
          </div>
        )}

        {provider.apiKeyUsages.map((item) => renderApiKeyRow(item))}
      </div>
    );
  }

  /** 精简模式：订阅窗口摘要行（标签 + 细进度条 + 百分比） */
  function renderCompactWindowRow(key: string, label: string, percent: number) {
    return (
      <div key={key} className="flex items-center gap-2">
        <span
          className="w-[72px] shrink-0 truncate text-[10px] font-semibold text-text-secondary"
          title={label}
        >
          {label || t("widget.providerCard.subscriptionShort")}
        </span>
        <UsageBar percent={percent} size="sm" labelClassName="w-7 text-[10px]" />
      </div>
    );
  }

  /** 精简模式：API Key 摘要行（百分比型走进度条，余额型直接显示余额/金额） */
  function renderCompactApiRow(item: ApiKeyUsageSummary) {
    const usage = item.usage!;
    const isPercent = usage.currency === "%";
    const percent = keyUsagePercent(item);
    const hasBar = isPercent || usage.totalBudget != null;
    const valueText = isPercent
      ? null
      : formatCurrency(usage.remaining ?? usage.totalUsed, usage.currency);

    const label = (
      <span
        className="w-[72px] shrink-0 truncate text-[10px] font-semibold text-text-secondary"
        title={item.keyName}
      >
        {item.keyName.trim() || t("widget.providerCard.apiShort")}
      </span>
    );

    const content = hasBar ? (
      <UsageBar percent={percent} size="sm" labelClassName="w-7 text-[10px]" />
    ) : (
      <span className="min-w-0 flex-1 truncate text-right text-[10px] tabular-nums text-text-secondary" title={valueText ?? undefined}>
        {valueText}
      </span>
    );

    if (useApiColorMarkers) {
      return (
        <div key={item.keyId} className="flex items-center gap-1.5">
          <span
            className="h-[18px] w-1 shrink-0 rounded-full bg-(--compact-marker-color)"
            style={{ "--compact-marker-color": item.color } as CSSProperties}
            aria-hidden="true"
          />
          {content}
        </div>
      );
    }

    return (
      <div key={item.keyId} className="flex items-center gap-2">
        {label}
        {content}
      </div>
    );
  }

  const hero = isCompact ? null : buildHero();
  // 单订阅且 Hero 大数字 caption 已含计划名（单订阅时 caption = 窗口标签 · 计划名），
  // 订阅明细块不再重复显示计划名
  const hideSubscriptionPlanLabel = provider.subscriptions.length === 1
    && provider.subscriptions[0]?.usage.status === "success"
    && hero?.kind === "percent";
  const hasCompactContent = provider.subscriptions.some((item) => item.usage.status === "success" && item.usage.windows.length > 0)
    || compactApiItems.length > 0;

  // 注意：保留 provider-card 类名作为拖拽状态钩子（widget.css 的 card-shell 规则会给它加阴影/边框）
  return (
    <div
      className={cn(
        "provider-card flex flex-col gap-2.5 rounded-xl border border-white/6 bg-card p-3",
        "transition-[border-color,box-shadow] duration-150 hover:border-white/12",
        isProviderErrorOnly && "border-danger/30",
        isCompact && "gap-1.5 px-2.5 py-2",
      )}
    >
      <div className="flex items-center justify-between gap-2">
        <div className="flex min-w-0 items-center gap-2">
          <ProviderIcon providerId={provider.providerId} size={isCompact ? 16 : 20} />
          <span
            className={cn("truncate font-semibold leading-[1.2] text-foreground", isCompact ? "text-[11px]" : "text-[13px]")}
            title={provider.displayName}
          >
            {provider.displayName}
          </span>
        </div>
        <button
          className={cn(
            "inline-flex shrink-0 items-center justify-center rounded-md border border-transparent",
            "cursor-pointer text-text-secondary transition-colors duration-150",
            "hover:bg-white/8 hover:text-text",
            "focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent/60",
            "disabled:cursor-not-allowed disabled:opacity-50",
            isCompact ? "h-6 w-6 [&_svg]:size-3" : "h-7 w-7 [&_svg]:size-3.5",
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
          {hasCompactContent && (
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
                      "flex flex-col gap-1 rounded-md border border-white/6 bg-white/3 px-2 py-1.5",
                      useSubscriptionColorMarkers && cn(
                        "relative pl-3",
                        "before:absolute before:top-1.5 before:bottom-1.5 before:left-1.5 before:w-0.5 before:rounded-full",
                        "before:bg-(--compact-marker-color) before:content-['']",
                      ),
                    )}
                    style={{ "--compact-marker-color": subscription.color } as CSSProperties}
                  >
                    {!useSubscriptionColorMarkers && (
                      <div className="truncate text-[10px] leading-[1.2] font-semibold text-text-secondary" title={subscription.subscriptionName}>
                        {subscription.subscriptionName}
                      </div>
                    )}
                    {subscription.usage.windows.map((window, index) => renderCompactWindowRow(
                      `${subscription.subscriptionId}-${window.label}-${index}`,
                      getWindowLabel(window.label, language),
                      window.utilization,
                    ))}
                    {hasExtra && renderCompactWindowRow(
                      `${subscription.subscriptionId}-extra`,
                      t("widget.subscription.extraUsageLabel"),
                      extra!.utilization!,
                    )}
                  </div>
                );
              })}

              {compactApiItems.map((item) => renderCompactApiRow(item))}
            </div>
          )}

          {compactSubscriptionErrors.map((item) => (
            <div key={`${item.subscriptionId}-error`} className="text-[11px] leading-snug text-error">
              {item.subscriptionName}: {item.usage.errorMessage}
            </div>
          ))}

          {compactApiErrors.map((item) => (
            <div key={`${item.keyId}-error`} className="text-[11px] leading-snug text-error">
              {hasMultipleApiKeys ? `${item.keyName}: ${item.errorMessage}` : item.errorMessage}
            </div>
          ))}

          {isProviderErrorOnly && provider.errorMessage && (
            <div className="text-[11px] leading-snug text-error">{provider.errorMessage}</div>
          )}
        </>
      ) : (
        <>
          {isProviderErrorOnly ? (
            <div className="text-[12px] leading-snug text-error">{provider.errorMessage}</div>
          ) : (
            <>
              {hero && renderHero(hero)}
              {!hero && !hasSubscription && !hasApiUsage && (
                <div className="text-[12px] text-text-muted">{t("widget.providerCard.unavailable")}</div>
              )}

              {provider.subscriptions.map((subscription) => (
                <SubscriptionBadge
                  key={subscription.subscriptionId}
                  subscription={subscription}
                  hidePlanLabel={hideSubscriptionPlanLabel}
                />
              ))}

              {hasApiUsage && renderApiSection()}
            </>
          )}
        </>
      )}
    </div>
  );
}
