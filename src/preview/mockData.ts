import type { UsageSummary } from "../types/provider";
import type { AppSettings } from "../types/settings";
import { DEFAULT_SETTINGS } from "../types/settings";
import type { UsageStatsSnapshot } from "../types/stats";

/**
 * UI 预览专用 mock 数据（仅供 preview.html 场景截图使用，不进入正式包）。
 * 覆盖四类典型供应商形态：
 * - OpenAI：订阅多窗口 + 按量 API Key（含 rate limit）
 * - Anthropic：多订阅窗口 + Extra Usage
 * - Kimi：百分比型 Coding Plan（5 小时 / 月度窗口）
 * - OpenRouter：纯余额型 + 一个 Key 报错
 */
export const MOCK_SUMMARIES: UsageSummary[] = [
  {
    providerId: "openai",
    displayName: "OpenAI",
    enabled: true,
    status: "success",
    usage: {
      totalUsed: 63.42,
      totalBudget: 120,
      remaining: 56.58,
      currency: "USD",
      periodStart: null,
      periodEnd: null,
    },
    apiKeyUsages: [
      {
        keyId: "openai-key-1",
        keyName: "主力 Key",
        color: "#3b82f6",
        status: "success",
        usage: {
          totalUsed: 63.42,
          totalBudget: 120,
          remaining: 56.58,
          currency: "USD",
          periodStart: null,
          periodEnd: null,
        },
        rateLimit: {
          requestsPerMinute: 42,
          requestsPerMinuteLimit: 500,
          tokensPerMinute: 18234,
          tokensPerMinuteLimit: 200000,
        },
        errorMessage: null,
      },
    ],
    subscriptions: [
      {
        subscriptionId: "openai-sub-1",
        subscriptionName: "ChatGPT Plus",
        color: "#10b981",
        source: "codex",
        usage: {
          planName: "Plus",
          status: "success",
          errorMessage: null,
          extraUsage: null,
          windows: [
            { label: "five_hour", utilization: 72, resetsAt: new Date(Date.now() + 2.5 * 3600_000).toISOString() },
            { label: "seven_day", utilization: 38, resetsAt: new Date(Date.now() + 4 * 24 * 3600_000).toISOString() },
          ],
        },
      },
    ],
    rateLimit: null,
    lastUpdated: new Date().toISOString(),
    errorMessage: null,
  },
  {
    providerId: "anthropic",
    displayName: "Anthropic",
    enabled: true,
    status: "success",
    usage: null,
    apiKeyUsages: [],
    subscriptions: [
      {
        subscriptionId: "anthropic-sub-1",
        subscriptionName: "Claude Pro",
        color: "#f59e0b",
        source: "claude",
        usage: {
          planName: "Pro",
          status: "success",
          errorMessage: null,
          extraUsage: {
            isEnabled: true,
            monthlyLimitUsd: 50,
            usedUsd: 41.25,
            utilization: 82.5,
            resetsAt: new Date(Date.now() + 9 * 24 * 3600_000).toISOString(),
          },
          windows: [
            { label: "five_hour", utilization: 45, resetsAt: new Date(Date.now() + 1.5 * 3600_000).toISOString() },
            { label: "seven_day", utilization: 61, resetsAt: new Date(Date.now() + 5 * 24 * 3600_000).toISOString() },
            { label: "seven_day_sonnet", utilization: 88, resetsAt: new Date(Date.now() + 5 * 24 * 3600_000).toISOString() },
          ],
        },
      },
    ],
    rateLimit: null,
    lastUpdated: new Date().toISOString(),
    errorMessage: null,
  },
  {
    providerId: "kimi",
    displayName: "Kimi",
    enabled: true,
    status: "success",
    usage: {
      totalUsed: 57,
      totalBudget: null,
      remaining: 43,
      currency: "%",
      periodStart: null,
      periodEnd: null,
      planName: "LEVEL_INTERMEDIATE",
      windows: [
        { label: "five_hour", utilization: 57, resetsAt: new Date(Date.now() + 2 * 3600_000).toISOString() },
        { label: "monthly", utilization: 23, resetsAt: new Date(Date.now() + 12 * 24 * 3600_000).toISOString() },
      ],
    },
    apiKeyUsages: [
      {
        keyId: "kimi-key-1",
        keyName: "Coding Plan",
        color: "#8b5cf6",
        status: "success",
        usage: {
          totalUsed: 57,
          totalBudget: null,
          remaining: 43,
          currency: "%",
          periodStart: null,
          periodEnd: null,
          planName: "LEVEL_INTERMEDIATE",
          windows: [
            { label: "five_hour", utilization: 57, resetsAt: new Date(Date.now() + 2 * 3600_000).toISOString() },
            { label: "monthly", utilization: 23, resetsAt: new Date(Date.now() + 12 * 24 * 3600_000).toISOString() },
          ],
        },
        rateLimit: null,
        errorMessage: null,
      },
    ],
    subscriptions: [],
    rateLimit: null,
    lastUpdated: new Date().toISOString(),
    errorMessage: null,
  },
  {
    providerId: "openrouter",
    displayName: "OpenRouter",
    enabled: true,
    status: "success",
    usage: {
      totalUsed: 3.87,
      totalBudget: null,
      remaining: 12.13,
      currency: "USD",
      periodStart: null,
      periodEnd: null,
    },
    apiKeyUsages: [
      {
        keyId: "openrouter-key-1",
        keyName: "主 Key",
        color: "#06b6d4",
        status: "success",
        usage: {
          totalUsed: 3.87,
          totalBudget: null,
          remaining: 12.13,
          currency: "USD",
          periodStart: null,
          periodEnd: null,
        },
        rateLimit: null,
        errorMessage: null,
      },
      {
        keyId: "openrouter-key-2",
        keyName: "备用 Key",
        color: "#ec4899",
        status: "error",
        usage: null,
        rateLimit: null,
        errorMessage: "HTTP 401：API Key 无效或已过期",
      },
    ],
    subscriptions: [],
    rateLimit: null,
    lastUpdated: new Date().toISOString(),
    errorMessage: null,
  },
];

export const MOCK_STATS_SNAPSHOT: UsageStatsSnapshot = {
  range: "day",
  generatedAt: new Date().toISOString(),
  healthNotices: [{ code: "insufficientSamples", level: "warning" }],
  providers: [
    {
      providerId: "openai",
      displayName: "OpenAI",
      lastSampleAt: new Date().toISOString(),
      apiSummary: {
        currency: "USD",
        currentTotalUsed: 63.42,
        rangeUsed: 18.42,
        currentRemaining: 56.58,
        recentVelocity: 2.4,
        forecast: { status: "available", hoursRemaining: 23.5, estimatedAt: null },
      },
      subscriptionTrends: [
        {
          subscriptionId: "openai-sub-1",
          subscriptionName: "ChatGPT Plus",
          kind: "window",
          label: "five_hour",
          currency: null,
          currentUtilization: 72,
          rangeDelta: 12,
          currentUsed: null,
          currentLimit: null,
          resetsAt: new Date(Date.now() + 2.5 * 3600_000).toISOString(),
          recentVelocity: 4.8,
          forecast: { status: "available", hoursRemaining: 14.2, estimatedAt: null },
        },
        {
          subscriptionId: "openai-sub-1",
          subscriptionName: "ChatGPT Plus",
          kind: "window",
          label: "seven_day",
          currency: null,
          currentUtilization: 38,
          rangeDelta: 5,
          currentUsed: null,
          currentLimit: null,
          resetsAt: new Date(Date.now() + 4 * 24 * 3600_000).toISOString(),
          recentVelocity: 0.6,
          forecast: { status: "unlikelyBeforeReset", hoursRemaining: null, estimatedAt: null },
        },
      ],
    },
    {
      providerId: "anthropic",
      displayName: "Anthropic",
      lastSampleAt: new Date().toISOString(),
      apiSummary: null,
      subscriptionTrends: [
        {
          subscriptionId: "anthropic-sub-1",
          subscriptionName: "Claude Pro",
          kind: "window",
          label: "seven_day_sonnet",
          currency: null,
          currentUtilization: 88,
          rangeDelta: 21,
          currentUsed: null,
          currentLimit: null,
          resetsAt: new Date(Date.now() + 5 * 24 * 3600_000).toISOString(),
          recentVelocity: 3.1,
          forecast: { status: "available", hoursRemaining: 38.6, estimatedAt: null },
        },
        {
          subscriptionId: "anthropic-sub-1",
          subscriptionName: "Claude Pro",
          kind: "extraUsage",
          label: "extra_usage",
          currency: "USD",
          currentUtilization: 82.5,
          rangeDelta: 9.5,
          currentUsed: 41.25,
          currentLimit: 50,
          resetsAt: new Date(Date.now() + 9 * 24 * 3600_000).toISOString(),
          recentVelocity: 0.4,
          forecast: { status: "insufficientData", hoursRemaining: null, estimatedAt: null },
        },
      ],
    },
  ],
};

/** 按场景生成设置 mock */
export function buildMockSettings(overrides: Partial<AppSettings> = {}): AppSettings {
  return {
    ...DEFAULT_SETTINGS,
    updateAutoCheckEnabled: false,
    updateCheckOnLaunch: false,
    ...overrides,
  };
}
