/** 格式化货币 */
export function formatCurrency(amount: number, currency: string = "USD"): string {
  if (currency === "USD") {
    return `$${amount.toFixed(2)}`;
  }
  return `${amount.toFixed(2)} ${currency}`;
}

/** 格式化百分比 */
export function formatPercent(value: number): string {
  return `${Math.round(value)}%`;
}

/** 计算使用百分比 */
export function calcUsagePercent(used: number, budget: number | null): number {
  if (!budget || budget <= 0) return 0;
  return Math.min(100, (used / budget) * 100);
}

/** 根据百分比获取颜色等级 */
export function getUsageColor(percent: number): string {
  if (percent < 60) return "var(--color-success)";
  if (percent < 85) return "var(--color-warning)";
  return "var(--color-danger)";
}

/** 格式化数字（带 k/M 后缀） */
export function formatNumber(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(0)}k`;
  return n.toString();
}

/** i18n 翻译函数签名（与 i18n/index.tsx 的 t 一致），用于文案跟随语言切换 */
type TranslateFn = (
  key: string,
  params?: Record<string, string | number | null | undefined>,
) => string;

/**
 * 格式化时间（相对时间）。
 * 文案走 i18n 的 common.time.* keys，调用方传入 useI18n() 的 t，
 * 不再硬编码中文。
 */
export function formatRelativeTime(isoString: string, t: TranslateFn): string {
  const diff = Date.now() - new Date(isoString).getTime();
  const seconds = Math.floor(diff / 1000);
  if (seconds < 60) return t("common.time.justNow");
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return t("common.time.minutesAgo", { count: minutes });
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return t("common.time.hoursAgo", { count: hours });
  return t("common.time.daysAgo", { count: Math.floor(hours / 24) });
}

/**
 * 格式化限额重置时间（相对未来的时间）。
 * 文案走 i18n 的 widget.subscription.reset* keys，调用方传入 useI18n() 的 t。
 * 供 SubscriptionBadge / ProviderCard 的窗口行共用，不要在组件里重复实现。
 */
export function formatResetTime(isoStr: string, t: TranslateFn): string {
  const reset = new Date(isoStr);
  const diffMs = reset.getTime() - Date.now();
  if (diffMs <= 0) return t("widget.subscription.resetSoon");
  const diffMin = Math.floor(diffMs / 60000);
  if (diffMin < 60) return t("widget.subscription.resetInMinutes", { count: diffMin });
  const diffHr = Math.floor(diffMin / 60);
  if (diffHr < 24) return t("widget.subscription.resetInHours", { count: diffHr });
  return t("widget.subscription.resetInDays", { count: Math.floor(diffHr / 24) });
}

/**
 * 格式化限额重置时间（精确到分的具体时间点）。
 * - 今天内重置：「14:32 重置」
 * - 明天重置：「明天 08:00 重置」
 * - 后天及以后：「8月5日 00:00 重置」（英文「Aug 5 00:00」）
 * 时间格式按语言区分：中文 24 小时制，英文 12 小时制（AM/PM）。
 * 供窗口行紧凑展示；悬停 title 可继续用 formatResetTime 的相对时间。
 */
export function formatResetTimeExact(isoStr: string, t: TranslateFn, language: string): string {
  const reset = new Date(isoStr);
  const diffMs = reset.getTime() - Date.now();
  if (diffMs <= 0) return t("widget.subscription.resetSoon");

  const now = new Date();
  const isEn = language === "en";
  const timeStr = isEn
    ? reset.toLocaleTimeString("en-US", { hour: "numeric", minute: "2-digit", hour12: true })
    : `${String(reset.getHours()).padStart(2, "0")}:${String(reset.getMinutes()).padStart(2, "0")}`;

  const isToday = reset.getFullYear() === now.getFullYear()
    && reset.getMonth() === now.getMonth()
    && reset.getDate() === now.getDate();
  if (isToday) {
    return t("widget.subscription.resetAtTime", { time: timeStr });
  }

  const tomorrow = new Date(now.getFullYear(), now.getMonth(), now.getDate() + 1);
  const isTomorrow = reset.getFullYear() === tomorrow.getFullYear()
    && reset.getMonth() === tomorrow.getMonth()
    && reset.getDate() === tomorrow.getDate();
  if (isTomorrow) {
    return t("widget.subscription.resetAtTomorrow", { time: timeStr });
  }

  const dateStr = isEn
    ? reset.toLocaleDateString("en-US", { month: "short", day: "numeric" })
    : `${reset.getMonth() + 1}月${reset.getDate()}日`;
  return t("widget.subscription.resetAtDate", { date: dateStr, time: timeStr });
}
