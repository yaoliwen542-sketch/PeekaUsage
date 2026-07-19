import type { RateLimitData } from "../../types/provider";
import { formatNumber } from "../../utils/formatters";

type RateLimitBadgeProps = {
  rateLimit: RateLimitData;
};

/** 速率限制徽标：统一的幽灵小胶囊样式，数字等宽 */
export default function RateLimitBadge({ rateLimit }: RateLimitBadgeProps) {
  const badgeClass = "rounded-md border border-white/6 bg-white/4 px-1.5 py-0.5 text-[10px] leading-[1.3] tabular-nums text-text-secondary";

  return (
    <div className="flex flex-wrap gap-1">
      {rateLimit.requestsPerMinute != null && rateLimit.requestsPerMinuteLimit != null && (
        <span className={badgeClass}>
          RPM: {rateLimit.requestsPerMinute}/{rateLimit.requestsPerMinuteLimit}
        </span>
      )}
      {rateLimit.tokensPerMinute != null && rateLimit.tokensPerMinuteLimit != null && (
        <span className={badgeClass}>
          TPM: {formatNumber(rateLimit.tokensPerMinute)}/{formatNumber(rateLimit.tokensPerMinuteLimit)}
        </span>
      )}
    </div>
  );
}
