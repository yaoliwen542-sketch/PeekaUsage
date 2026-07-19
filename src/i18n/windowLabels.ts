import type { AppLanguage } from "../types/settings";
import { windowLabels } from "./messages";

/**
 * 订阅窗口标签 i18n：把后端返回的机器常量（five_hour / seven_day 等）
 * 映射成当前语言文案，找不到映射时原样返回。
 * ProviderCard / SubscriptionBadge / UsageStatsPanel / IslandWidget 共用。
 */
export function getWindowLabel(label: string, language: AppLanguage): string {
  const messages = windowLabels[label];
  if (messages && messages[language]) {
    return messages[language] as string;
  }
  return label;
}
