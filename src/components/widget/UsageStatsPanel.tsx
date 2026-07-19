import { useEffect, useMemo, useState } from "react";
import { useI18n } from "../../i18n";
import { getWindowLabel } from "../../i18n/windowLabels";
import type { UsageSummary } from "../../types/provider";
import type {
  ProviderStatsSnapshot,
  StatsRange,
  UsageForecast,
  UsageStatsSnapshot,
} from "../../types/stats";
import { formatCurrency, formatPercent } from "../../utils/formatters";
import { getUsageStatsSnapshot } from "../../utils/ipc";
import { cn } from "@/lib/utils";
import ProviderIcon from "../common/ProviderIcon";

type UsageStatsPanelProps = {
  open: boolean;
  providers: UsageSummary[];
  onClose: () => void;
};

/** 指标小块：标签 + 等宽数字值，与卡片内的明细块视觉统一 */
function MetricBlock({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex min-w-0 flex-col gap-0.5 rounded-md bg-white/4 px-2 py-1.5">
      <span className="truncate text-[10px] text-text-muted">{label}</span>
      <strong className="truncate text-[11px] font-semibold tabular-nums text-foreground" title={value}>
        {value}
      </strong>
    </div>
  );
}

export default function UsageStatsPanel({
  open,
  providers,
  onClose,
}: UsageStatsPanelProps) {
  const { language, t } = useI18n();
  const [range, setRange] = useState<StatsRange>("day");
  const [snapshot, setSnapshot] = useState<UsageStatsSnapshot | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refreshToken = useMemo(
    () => providers
      .filter((provider) => provider.enabled)
      .map((provider) => `${provider.providerId}:${provider.lastUpdated ?? ""}:${provider.status}`)
      .join("|"),
    [providers],
  );

  useEffect(() => {
    if (!open) {
      return;
    }

    setRange("day");
  }, [open]);

  useEffect(() => {
    if (!open) {
      return;
    }

    let active = true;
    setLoading(true);
    setError(null);

    void getUsageStatsSnapshot(range)
      .then((nextSnapshot) => {
        if (!active) {
          return;
        }
        setSnapshot(nextSnapshot);
      })
      .catch((reason: unknown) => {
        if (!active) {
          return;
        }
        setError(reason instanceof Error ? reason.message : String(reason));
      })
      .finally(() => {
        if (active) {
          setLoading(false);
        }
      });

    return () => {
      active = false;
    };
  }, [open, range, refreshToken]);

  if (!open) {
    return null;
  }

  const locale = languageToLocale(language);
  const lastSampleAt = getLatestSampleTime(snapshot?.providers ?? []);

  return (
    <div
      className="flex min-h-0 w-full flex-1 flex-col gap-2.5 rounded-xl border border-white/8 bg-surface-elevated p-3 shadow-overlay [backdrop-filter:blur(var(--backdrop-blur))]"
      role="dialog"
      aria-modal="false"
      aria-label={t("widget.stats.title")}
    >
      <div className="flex flex-col gap-2">
        <div className="flex items-center justify-between gap-2">
          <h3 className="m-0 truncate text-[13px] font-bold text-foreground">{t("widget.stats.title")}</h3>
          <button
            className={cn(
              "inline-flex h-7 w-7 shrink-0 items-center justify-center rounded-md border border-transparent",
              "cursor-pointer text-text-secondary transition-colors duration-150",
              "hover:bg-white/8 hover:text-text",
              "focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent/60",
              "[&_svg]:size-3.5",
            )}
            type="button"
            aria-label={t("widget.stats.close")}
            onClick={onClose}
          >
            <svg viewBox="0 0 24 24" aria-hidden="true">
              <path
                d="M6 6l12 12M18 6 6 18"
                fill="none"
                stroke="currentColor"
                strokeLinecap="round"
                strokeWidth="1.8"
              />
            </svg>
          </button>
        </div>

        <div
          className="inline-flex items-center gap-0.5 self-start rounded-lg border border-white/6 bg-white/4 p-0.5"
          role="tablist"
          aria-label={t("widget.stats.range.ariaLabel")}
        >
          {(["day", "month"] as const).map((item) => (
            <button
              key={item}
              className={cn(
                "h-6 min-w-[44px] cursor-pointer rounded-md border border-transparent px-2.5",
                "text-[11px] font-semibold text-text-secondary transition-colors duration-150",
                "hover:text-text focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent/60",
                range === item && "bg-white/10 text-foreground",
              )}
              type="button"
              aria-pressed={range === item}
              onClick={() => setRange(item)}
            >
              {t(`widget.stats.range.${item}`)}
            </button>
          ))}
        </div>
      </div>

      {snapshot?.healthNotices.length ? (
        <div className="flex flex-col gap-1">
          {snapshot.healthNotices.map((notice) => (
            <div
              key={notice.code}
              className={cn(
                "rounded-md border border-white/6 bg-white/3 px-2 py-1.5 text-[11px] leading-snug text-text-secondary",
                notice.level === "warning" && "border-warning/30 bg-warning/10 text-warning",
              )}
            >
              {t(`widget.stats.notices.${notice.code}`)}
            </div>
          ))}
        </div>
      ) : null}

      {loading && !snapshot ? (
        <div className="flex flex-1 items-center justify-center px-3 py-5 text-center text-[12px] text-text-muted">
          {t("widget.stats.loading")}
        </div>
      ) : error ? (
        <div className="flex flex-1 items-center justify-center px-3 py-5 text-center text-[12px] text-error">
          {t("widget.stats.loadFailed", { message: error })}
        </div>
      ) : !snapshot || snapshot.providers.length === 0 ? (
        <div className="flex flex-1 items-center justify-center px-3 py-5 text-center text-[12px] text-text-muted">
          {t("widget.stats.empty")}
        </div>
      ) : (
        <div className="flex min-h-0 flex-1 flex-col gap-2 overflow-y-auto">
          <div className="grid grid-cols-3 gap-1.5">
            <div className="flex min-w-0 flex-col gap-0.5 rounded-lg border border-white/6 bg-white/3 px-2 py-1.5">
              <span className="truncate text-[10px] text-text-muted">{t("widget.stats.summary.providerCount")}</span>
              <strong className="truncate text-[12px] font-bold tabular-nums text-foreground">{snapshot.providers.length}</strong>
            </div>
            <div className="flex min-w-0 flex-col gap-0.5 rounded-lg border border-white/6 bg-white/3 px-2 py-1.5">
              <span className="truncate text-[10px] text-text-muted">{t("widget.stats.summary.lastSampleAt")}</span>
              <strong className="truncate text-[12px] font-bold tabular-nums text-foreground">
                {lastSampleAt ? formatDateTime(lastSampleAt, locale) : t("widget.stats.value.unavailable")}
              </strong>
            </div>
            <div className="flex min-w-0 flex-col gap-0.5 rounded-lg border border-white/6 bg-white/3 px-2 py-1.5">
              <span className="truncate text-[10px] text-text-muted">{t("widget.stats.summary.range")}</span>
              <strong className="truncate text-[12px] font-bold tabular-nums text-foreground">{t(`widget.stats.summary.${range}Description`)}</strong>
            </div>
          </div>

          <div className="flex flex-col gap-2">
            {snapshot.providers.map((provider) => (
              <article
                key={provider.providerId}
                className="flex flex-col gap-2 rounded-xl border border-white/6 bg-white/3 p-2.5"
              >
                <div className="flex items-center justify-between gap-2">
                  <div className="flex min-w-0 items-center gap-1.5 text-[12px] font-bold text-foreground">
                    <ProviderIcon providerId={provider.providerId} size={16} />
                    <span className="truncate">{provider.displayName}</span>
                  </div>
                  <span className="shrink-0 text-[10px] tabular-nums text-text-muted">
                    {provider.lastSampleAt
                      ? formatDateTime(provider.lastSampleAt, locale)
                      : t("widget.stats.value.unavailable")}
                  </span>
                </div>

                {provider.apiSummary ? (
                  <div className="flex flex-col gap-1.5 rounded-lg bg-white/3 p-2">
                    <div className="text-[11px] font-bold text-foreground">{t("widget.stats.api.title")}</div>
                    <div className="grid grid-cols-2 gap-1.5">
                      <MetricBlock
                        label={t(`widget.stats.api.${range}Usage`)}
                        value={formatCurrency(provider.apiSummary.rangeUsed, provider.apiSummary.currency)}
                      />
                      <MetricBlock
                        label={t("widget.stats.api.currentRemaining")}
                        value={provider.apiSummary.currentRemaining != null
                          ? formatCurrency(provider.apiSummary.currentRemaining, provider.apiSummary.currency)
                          : t("widget.stats.forecast.notApplicable")}
                      />
                      <MetricBlock
                        label={t("widget.stats.api.forecast")}
                        value={formatForecast(provider.apiSummary.forecast, t)}
                      />
                      <MetricBlock
                        label={t("widget.stats.api.recentVelocity")}
                        value={provider.apiSummary.recentVelocity != null
                          ? t("widget.stats.value.currencyPerHour", {
                            value: formatCurrency(provider.apiSummary.recentVelocity, provider.apiSummary.currency),
                          })
                          : t("widget.stats.forecast.insufficientData")}
                      />
                    </div>
                  </div>
                ) : null}

                {provider.subscriptionTrends.length ? (
                  <div className="flex flex-col gap-1.5 rounded-lg bg-white/3 p-2">
                    <div className="text-[11px] font-bold text-foreground">{t("widget.stats.subscription.title")}</div>
                    <div className="flex flex-col gap-1.5">
                      {provider.subscriptionTrends.map((trend) => (
                        <div
                          key={`${trend.subscriptionId}-${trend.kind}-${trend.label}`}
                          className="flex flex-col gap-1.5 rounded-lg border border-white/6 bg-white/3 p-2"
                        >
                          <div className="flex items-center justify-between gap-2">
                            <div className="min-w-0">
                              <div className="truncate text-[11px] font-bold text-foreground">{trend.subscriptionName}</div>
                              <div className="truncate text-[10px] text-text-secondary">
                                {trend.kind === "extraUsage"
                                  ? t("widget.subscription.extraUsageLabel")
                                  : getWindowLabel(trend.label, language)}
                              </div>
                            </div>
                            <div className="shrink-0 text-[13px] font-bold tabular-nums text-primary-hover">
                              {formatPercent(trend.currentUtilization)}
                            </div>
                          </div>

                          <div className="grid grid-cols-2 gap-1.5">
                            <MetricBlock
                              label={t("widget.stats.subscription.currentUtilization")}
                              value={formatPercent(trend.currentUtilization)}
                            />
                            <MetricBlock
                              label={t(`widget.stats.subscription.${range}Delta`)}
                              value={formatPercent(trend.rangeDelta)}
                            />
                            <MetricBlock
                              label={t("widget.stats.subscription.forecast")}
                              value={formatForecast(trend.forecast, t)}
                            />
                            <MetricBlock
                              label={t("widget.stats.subscription.recentVelocity")}
                              value={trend.recentVelocity != null
                                ? t("widget.stats.value.percentPerHour", {
                                  value: formatPercent(trend.recentVelocity),
                                })
                                : t("widget.stats.forecast.insufficientData")}
                            />
                          </div>

                          {trend.kind === "extraUsage" && trend.currentUsed != null && trend.currentLimit != null ? (
                            <div className="text-[10px] tabular-nums text-text-secondary">
                              {t("widget.stats.subscription.usedLimit", {
                                used: formatCurrency(trend.currentUsed, trend.currency ?? "USD"),
                                limit: formatCurrency(trend.currentLimit, trend.currency ?? "USD"),
                              })}
                            </div>
                          ) : null}

                          {trend.resetsAt ? (
                            <div className="text-[10px] text-text-muted">
                              {t("widget.stats.subscription.resetAt", {
                                time: formatDateTime(trend.resetsAt, locale),
                              })}
                            </div>
                          ) : null}
                        </div>
                      ))}
                    </div>
                  </div>
                ) : null}
              </article>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}

function languageToLocale(language: string): string {
  if (language === "zh-Hant") {
    return "zh-Hant";
  }

  if (language === "en") {
    return "en-US";
  }

  return "zh-CN";
}

function formatDateTime(value: string, locale: string): string {
  return new Intl.DateTimeFormat(locale, {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  }).format(new Date(value));
}

function getLatestSampleTime(providers: ProviderStatsSnapshot[]): string | null {
  let latest: string | null = null;

  for (const provider of providers) {
    if (!provider.lastSampleAt) {
      continue;
    }

    if (!latest || new Date(provider.lastSampleAt).getTime() > new Date(latest).getTime()) {
      latest = provider.lastSampleAt;
    }
  }

  return latest;
}

function formatForecast(
  forecast: UsageForecast,
  t: (key: string, params?: Record<string, string | number | null | undefined>) => string,
): string {
  switch (forecast.status) {
    case "available":
      if (forecast.hoursRemaining == null || forecast.hoursRemaining <= 0.01) {
        return t("widget.stats.forecast.now");
      }
      return t("widget.stats.forecast.available", {
        duration: formatDuration(forecast.hoursRemaining, t),
      });
    case "unlikelyBeforeReset":
      return t("widget.stats.forecast.unlikelyBeforeReset");
    case "notApplicable":
      return t("widget.stats.forecast.notApplicable");
    case "insufficientData":
    default:
      return t("widget.stats.forecast.insufficientData");
  }
}

function formatDuration(
  hours: number,
  t: (key: string, params?: Record<string, string | number | null | undefined>) => string,
): string {
  if (hours >= 48) {
    return t("widget.stats.duration.days", { value: Math.round(hours / 24) });
  }

  if (hours >= 1) {
    return t("widget.stats.duration.hours", { value: Math.round(hours * 10) / 10 });
  }

  return t("widget.stats.duration.minutes", { value: Math.max(1, Math.round(hours * 60)) });
}
