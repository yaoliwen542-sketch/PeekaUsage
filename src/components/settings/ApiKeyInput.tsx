import { useState } from "react";
import { useI18n } from "../../i18n";

type ApiKeyInputProps = {
  modelValue: string;
  placeholder?: string;
  onChange: (value: string) => void;
};

/** 密钥 / Token 输入框：等宽字体 + 眼睛图标切换明文。
 * 样式全 Tailwind 表达，与设置页其他输入框同一视觉体系 */
export default function ApiKeyInput({
  modelValue,
  placeholder,
  onChange,
}: ApiKeyInputProps) {
  const { t } = useI18n();
  const [showKey, setShowKey] = useState(false);

  return (
    <div
      className="flex h-8 items-center rounded-lg border border-border bg-background transition-[border-color,box-shadow] duration-150 hover:border-border-hover focus-within:border-primary-soft-border focus-within:ring-1 focus-within:ring-primary/40"
    >
      <input
        type={showKey ? "text" : "password"}
        value={modelValue}
        placeholder={placeholder ?? t("settings.providerConfig.apiKeyInputPlaceholder")}
        onChange={(event) => onChange(event.target.value)}
        className="min-w-0 flex-1 bg-transparent px-2.5 font-mono text-xs text-text outline-none placeholder:font-sans placeholder:text-text-muted"
        spellCheck={false}
        autoComplete="off"
      />
      <button
        className="mr-1 flex h-6 w-6 shrink-0 items-center justify-center rounded-md text-text-secondary transition-colors duration-150 hover:bg-ghost-hover hover:text-text focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-primary/50"
        onClick={() => setShowKey((value) => !value)}
        title={showKey ? t("common.hide") : t("common.show")}
        aria-label={showKey ? t("common.hide") : t("common.show")}
        type="button"
      >
        {showKey ? (
          <svg width="13" height="13" viewBox="0 0 16 16" fill="none" aria-hidden="true">
            <path d="M2.5 2.5l11 11" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" />
            <path d="M6.6 6.9a2 2 0 002.8 2.8M5.2 5.5C3.9 6.4 2.9 7.6 2.3 8c1.6 2.3 3.6 3.5 5.7 3.5 1 0 1.9-.3 2.8-.7M7 4.6c.3-.1.7-.1 1-.1 2.1 0 4.1 1.2 5.7 3.5-.5.7-1.2 1.4-2 2" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" />
          </svg>
        ) : (
          <svg width="13" height="13" viewBox="0 0 16 16" fill="none" aria-hidden="true">
            <path d="M2.3 8C3.9 5.7 5.9 4.5 8 4.5s4.1 1.2 5.7 3.5C12.1 10.3 10.1 11.5 8 11.5S3.9 10.3 2.3 8z" stroke="currentColor" strokeWidth="1.4" strokeLinejoin="round" />
            <circle cx="8" cy="8" r="1.8" stroke="currentColor" strokeWidth="1.4" />
          </svg>
        )}
      </button>
    </div>
  );
}
