import type { CSSProperties } from "react";
import { useI18n } from "../../i18n";
import type { ProviderId } from "../../types/provider";
import openaiIcon from "../../assets/provider-icons/openai.svg";
import anthropicIcon from "../../assets/provider-icons/anthropic.png";
import openrouterIcon from "../../assets/provider-icons/openrouter.jpeg";
import deepseekIcon from "../../assets/provider-icons/deepseek.svg";
import newapiIcon from "../../assets/provider-icons/newapi.svg";
import customIcon from "../../assets/provider-icons/custom.svg";

type ProviderIconProps = {
  providerId: ProviderId;
  size?: number;
};

/** 已知供应商图标的映射表，键为图标名/供应商 ID */
const iconSrcMap: Record<string, string> = {
  openai: openaiIcon,
  anthropic: anthropicIcon,
  openrouter: openrouterIcon,
  deepseek: deepseekIcon,
  newapi: newapiIcon,
  new_api: newapiIcon,
  custom: customIcon,
};

/** 已知供应商的图标 alt 文案映射（找不到时用通用文案） */
const iconAltMap: Record<string, string> = {
  openai: "OpenAI",
  anthropic: "Anthropic",
  openrouter: "OpenRouter",
  deepseek: "DeepSeek",
  newapi: "NewAPI",
  new_api: "NewAPI",
  custom: "Custom",
};

/** 解析图标资源：优先按供应商 ID 匹配，找不到时回退到 custom.svg */
function resolveIconSrc(providerId: ProviderId): string {
  if (providerId && iconSrcMap[providerId]) {
    return iconSrcMap[providerId];
  }
  // 自定义供应商 ID 形如 "custom_xxx"，统一回退到 custom.svg
  return customIcon;
}

export default function ProviderIcon({
  providerId,
  size = 18,
}: ProviderIconProps) {
  const { t } = useI18n();
  const iconStyle: CSSProperties = {
    width: `${size}px`,
    height: `${size}px`,
  };

  const src = resolveIconSrc(providerId);
  const altName = (providerId && iconAltMap[providerId]) || providerId || "Custom";

  return (
    <span className={`provider-icon is-${providerId || "custom"}`} style={iconStyle} aria-hidden="true">
      <img
        src={src}
        alt={t("providerIcon.alt", { providerName: altName })}
      />
    </span>
  );
}
