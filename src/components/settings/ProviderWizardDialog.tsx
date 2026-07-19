import { useState } from "react";
import { useI18n } from "../../i18n";
import type {
  AuthSchemeConfig,
  CustomProviderConfig,
  ScriptConfig,
} from "../../types/provider";
import { getNewApiScriptTemplate, testCustomProviderScript } from "../../utils/ipc";
import { cn } from "@/lib/utils";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import ProviderIcon from "../common/ProviderIcon";

type ProviderWizardDialogProps = {
  open: boolean;
  onClose: () => void;
  onConfirm: (config: CustomProviderConfig) => void;
};

const DEFAULT_TIMEOUT_MS = 15000;
const AUTH_SCHEME_OPTIONS: Array<{ value: AuthSchemeConfig; labelKey: string }> = [
  { value: "bearer", labelKey: "settings.wizard.authBearer" },
  { value: "x_api_key", labelKey: "settings.wizard.authXApiKey" },
  { value: "raw_key", labelKey: "settings.wizard.authRawKey" },
];

// 修复 I-3：阶段 1 自定义供应商只支持 Script 查询，向导 UI 不再暴露 Balance 选项
// （QueryTypeConfig 枚举里保留 Balance，阶段 2 实现真正的 Balance 查询后开放）
const ICON_CHOICES = ["custom", "deepseek", "newapi", "openai", "anthropic", "openrouter"];

/** 字段标签通用样式 */
const FIELD_LABEL_CLASS = "text-[11px] font-semibold text-foreground-secondary";
/** 字段提示通用样式 */
const FIELD_HINT_CLASS = "text-[10px] leading-[1.5] text-foreground-muted";
/** 单行输入框通用样式（32px 高、12px 字号，与设置页行式控件统一） */
const WIZARD_INPUT_CLASS = "h-8 rounded-lg border-border bg-surface px-2.5 py-1.5 text-xs";
/** 分段选择器按钮基础样式（与设置页 segmented 控件一致） */
const SEGMENT_BUTTON_CLASS = "flex h-7 cursor-pointer items-center rounded-full border-0 bg-transparent px-3 text-xs text-text-secondary transition-colors duration-150 hover:text-text";

/** 自定义供应商 3 步创建向导 */
export function ProviderWizardDialog({ open, onClose, onConfirm }: ProviderWizardDialogProps) {
  const { t } = useI18n();
  const [step, setStep] = useState(1);
  const [displayName, setDisplayName] = useState("");
  const [icon, setIcon] = useState<string>("custom");
  const [authScheme, setAuthScheme] = useState<AuthSchemeConfig>("bearer");
  const [baseUrl, setBaseUrl] = useState("");
  // 修复 I-3：queryType 固定为 "script"（阶段 1 不开放 Balance）
  const queryType = "script" as const;
  const [scriptCode, setScriptCode] = useState("");
  const [allowHttp, setAllowHttp] = useState(false);
  const [envKeyName, setEnvKeyName] = useState("");
  // 修复 C-3：NewAPI 等 Script 模板需要的 accessToken / userId
  // 阶段 1 临时方案：明文存储于 custom_config，阶段 2 迁移到 KeyStore
  const [accessToken, setAccessToken] = useState("");
  const [userId, setUserId] = useState("");
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<{ ok: boolean; message: string } | null>(null);
  const [loadingTemplate, setLoadingTemplate] = useState(false);

  // 重置状态（关闭或确认后调用）
  function resetState() {
    setStep(1);
    setDisplayName("");
    setIcon("custom");
    setAuthScheme("bearer");
    setBaseUrl("");
    setScriptCode("");
    setAllowHttp(false);
    setEnvKeyName("");
    setAccessToken("");
    setUserId("");
    setTesting(false);
    setTestResult(null);
    setLoadingTemplate(false);
  }

  // 关闭并重置
  function handleClose() {
    if (testing) {
      return;
    }
    resetState();
    onClose();
  }

  // 预填 NewAPI 脚本模板（一键填充 NewAPI 预设：脚本 + 提示用户填 accessToken/userId）
  async function fillNewApiTemplate() {
    if (loadingTemplate) {
      return;
    }
    setLoadingTemplate(true);
    try {
      const code = await getNewApiScriptTemplate();
      setScriptCode(code);
      setTestResult(null);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setTestResult({ ok: false, message: t("settings.wizard.templateLoadFailed", { message }) });
    } finally {
      setLoadingTemplate(false);
    }
  }

  // 测试脚本
  async function handleTest() {
    if (testing || !scriptCode.trim()) {
      return;
    }
    setTesting(true);
    setTestResult(null);
    try {
      // 修复 C-3：测试时传入 accessToken / userId，与真实查询链路一致
      const result = await testCustomProviderScript(
        scriptCode,
        "test-key",
        baseUrl.trim() || null,
        allowHttp,
        accessToken.trim() || null,
        userId.trim() || null,
      );
      setTestResult({ ok: true, message: result });
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setTestResult({ ok: false, message });
    } finally {
      setTesting(false);
    }
  }

  // 校验：每步是否可继续
  // 修复 I-3：queryType 固定为 script，校验条件简化
  const step1Valid = displayName.trim().length > 0;
  const step2Valid = scriptCode.trim().length > 0 && baseUrl.trim().length > 0;

  // 确认创建
  function handleConfirm() {
    if (!step1Valid || !step2Valid) {
      return;
    }
    const trimmedDisplayName = displayName.trim();
    const trimmedBaseUrl = baseUrl.trim();
    const trimmedEnvKeyName = envKeyName.trim();
    const trimmedAccessToken = accessToken.trim();
    const trimmedUserId = userId.trim();

    const script: ScriptConfig = {
      code: scriptCode,
      language: "javascript",
      timeoutMs: DEFAULT_TIMEOUT_MS,
    };

    const config: CustomProviderConfig = {
      displayName: trimmedDisplayName,
      baseUrl: trimmedBaseUrl,
      authScheme,
      envKeyName: trimmedEnvKeyName || null,
      icon: icon === "custom" ? null : icon,
      queryType,
      script,
      allowHttp,
      // 修复 C-3：accessToken / userId 随 custom_config 保存（阶段 1 临时方案）
      accessToken: trimmedAccessToken || null,
      userId: trimmedUserId || null,
    };

    onConfirm(config);
    resetState();
  }

  const stepTitleKey = step === 1
    ? "settings.wizard.step1Title"
    : step === 2
      ? "settings.wizard.step2Title"
      : "settings.wizard.step3Title";

  return (
    <Dialog
      open={open}
      onOpenChange={(nextOpen) => {
        // 测试进行中时忽略 Esc / 遮罩点击触发的关闭
        if (!nextOpen && !testing) {
          handleClose();
        }
      }}
    >
      <DialogContent
        className="w-full gap-3 rounded-xl border-border bg-surface px-5 pt-[18px] pb-4 shadow-drag sm:max-w-[520px]"
        showCloseButton={false}
        aria-label={t("settings.wizard.ariaLabel")}
      >
        <DialogHeader className="flex-row items-center justify-between gap-3 space-y-0">
          <DialogTitle className="text-sm font-semibold text-foreground">{t(stepTitleKey)}</DialogTitle>
          <span className="shrink-0 text-[11px] text-foreground-muted">
            {t("settings.wizard.stepIndicator", { current: step, total: 3 })}
          </span>
        </DialogHeader>

        {/* 步骤进度条 */}
        <div className="flex items-center gap-1.5" role="presentation">
          {[1, 2, 3].map((s) => (
            <div
              key={s}
              className={cn(
                "h-[3px] flex-1 rounded-[2px] bg-border transition-colors",
                s === step && "bg-primary-soft-border",
                s < step && "bg-primary-soft-text",
              )}
            />
          ))}
        </div>

        <div className="flex max-h-[calc(100vh-200px)] flex-col gap-3 overflow-y-auto pr-0.5">
          {step === 1 && (
            <>
              <div className="flex flex-col gap-2">
                <label className={FIELD_LABEL_CLASS} htmlFor="wizard-display-name">
                  {t("settings.wizard.displayName")}
                </label>
                <Input
                  id="wizard-display-name"
                  className={WIZARD_INPUT_CLASS}
                  type="text"
                  value={displayName}
                  placeholder={t("settings.wizard.displayNamePlaceholder")}
                  onChange={(event) => setDisplayName(event.target.value)}
                  autoFocus
                />
              </div>

              <div className="flex flex-col gap-2">
                <label className={FIELD_LABEL_CLASS}>{t("settings.wizard.icon")}</label>
                <div className="flex flex-wrap gap-2">
                  {ICON_CHOICES.map((iconChoice) => (
                    <button
                      key={iconChoice}
                      type="button"
                      className={cn(
                        "flex h-9 w-9 cursor-pointer items-center justify-center rounded-lg border border-border bg-surface p-0",
                        "transition-[border-color,background,box-shadow] hover:border-border-hover hover:bg-surface-hover",
                        icon === iconChoice && "border-primary-soft-border bg-primary-soft-bg shadow-[0_0_0_2px_var(--color-primary-soft-bg)]",
                      )}
                      onClick={() => setIcon(iconChoice)}
                      title={iconChoice}
                      aria-pressed={icon === iconChoice}
                    >
                      <ProviderIcon providerId={iconChoice} size={22} />
                    </button>
                  ))}
                </div>
              </div>

              <div className="flex flex-col gap-2">
                <label className={FIELD_LABEL_CLASS}>{t("settings.wizard.authScheme")}</label>
                <div className="inline-flex gap-0.5 self-start rounded-full bg-ghost p-0.5" role="group" aria-label={t("settings.wizard.authScheme")}>
                  {AUTH_SCHEME_OPTIONS.map((option) => (
                    <button
                      key={option.value}
                      type="button"
                      className={cn(
                        SEGMENT_BUTTON_CLASS,
                        authScheme === option.value && "bg-surface-elevated font-medium text-text shadow-sm",
                      )}
                      aria-pressed={authScheme === option.value}
                      onClick={() => setAuthScheme(option.value)}
                    >
                      {t(option.labelKey)}
                    </button>
                  ))}
                </div>
              </div>
            </>
          )}

          {step === 2 && (
            <>
              {/* 修复 I-3：阶段 1 自定义供应商只支持 Script 查询，移除 Balance/Script 切换器 */}
              <div className="flex flex-col gap-2">
                <label className={FIELD_LABEL_CLASS}>{t("settings.wizard.queryType")}</label>
                <div className="inline-flex gap-0.5 self-start rounded-full bg-ghost p-0.5" role="group" aria-label={t("settings.wizard.queryType")}>
                  <button
                    type="button"
                    className={cn(SEGMENT_BUTTON_CLASS, "bg-surface-elevated font-medium text-text shadow-sm")}
                    aria-pressed={true}
                    disabled
                  >
                    {t("settings.wizard.queryTypeScript")}
                  </button>
                </div>
                <div className={FIELD_HINT_CLASS}>{t("settings.wizard.queryTypeScriptHint")}</div>
              </div>

              <div className="flex flex-col gap-2">
                <label className={FIELD_LABEL_CLASS} htmlFor="wizard-base-url">
                  {t("settings.wizard.baseUrl")}
                </label>
                <Input
                  id="wizard-base-url"
                  className={WIZARD_INPUT_CLASS}
                  type="text"
                  value={baseUrl}
                  placeholder="https://your-gateway.example.com"
                  onChange={(event) => setBaseUrl(event.target.value)}
                />
                <div className={FIELD_HINT_CLASS}>{t("settings.wizard.baseUrlHint")}</div>
              </div>

              <div className="flex flex-col gap-2">
                <div className="flex items-center justify-between gap-2">
                  <label className={FIELD_LABEL_CLASS} htmlFor="wizard-script-code">
                    {t("settings.wizard.script")}
                  </label>
                  <Button
                    type="button"
                    variant="softGhost"
                    size="xs"
                    disabled={loadingTemplate}
                    onClick={() => void fillNewApiTemplate()}
                  >
                    {loadingTemplate ? t("settings.wizard.loading") : t("settings.wizard.fillNewApiTemplate")}
                  </Button>
                </div>
                <textarea
                  id="wizard-script-code"
                  className={cn(
                    "w-full resize-y rounded-lg border border-border bg-surface px-2.5 py-2 font-mono text-[11px] leading-[1.5] text-foreground tab-size-2",
                    "transition-[border-color,box-shadow] focus:border-primary-soft-border focus:shadow-[0_0_0_3px_var(--color-primary-soft-bg)] focus:outline-none",
                  )}
                  value={scriptCode}
                  rows={10}
                  spellCheck={false}
                  placeholder={t("settings.wizard.scriptPlaceholder")}
                  onChange={(event) => setScriptCode(event.target.value)}
                />
                <div className={FIELD_HINT_CLASS}>{t("settings.wizard.scriptHint")}</div>
              </div>

              {/* 修复 C-3：Script 模板可选的 accessToken / userId 输入框
                  （NewAPI 等 API 网关需要这两个凭据，通过 {{accessToken}} / {{userId}} 注入脚本） */}
              <div className="flex flex-col gap-2">
                <label className={FIELD_LABEL_CLASS} htmlFor="wizard-access-token">
                  {t("settings.wizard.accessToken")}
                </label>
                <Input
                  id="wizard-access-token"
                  className={WIZARD_INPUT_CLASS}
                  type="text"
                  value={accessToken}
                  placeholder={t("settings.wizard.accessTokenPlaceholder")}
                  onChange={(event) => setAccessToken(event.target.value)}
                  autoComplete="off"
                />
                <div className={FIELD_HINT_CLASS}>{t("settings.wizard.accessTokenHint")}</div>
              </div>

              <div className="flex flex-col gap-2">
                <label className={FIELD_LABEL_CLASS} htmlFor="wizard-user-id">
                  {t("settings.wizard.userId")}
                </label>
                <Input
                  id="wizard-user-id"
                  className={WIZARD_INPUT_CLASS}
                  type="text"
                  value={userId}
                  placeholder={t("settings.wizard.userIdPlaceholder")}
                  onChange={(event) => setUserId(event.target.value)}
                  autoComplete="off"
                />
                <div className={FIELD_HINT_CLASS}>{t("settings.wizard.userIdHint")}</div>
              </div>
            </>
          )}

          {step === 3 && (
            <>
              <div className="flex flex-col gap-2">
                <label className={FIELD_LABEL_CLASS} htmlFor="wizard-env-key-name">
                  {t("settings.wizard.envKeyName")}
                </label>
                <Input
                  id="wizard-env-key-name"
                  className={WIZARD_INPUT_CLASS}
                  type="text"
                  value={envKeyName}
                  placeholder="CUSTOM_PROVIDER_API_KEY"
                  onChange={(event) => setEnvKeyName(event.target.value)}
                />
                <div className={FIELD_HINT_CLASS}>{t("settings.wizard.envKeyNameHint")}</div>
              </div>

              <label
                className="flex cursor-pointer items-center justify-between gap-2.5 rounded-lg border border-border bg-surface px-3 py-2.5"
                htmlFor="wizard-allow-http"
              >
                <span className="flex min-w-0 flex-col gap-0.5">
                  <span className="text-xs font-semibold text-foreground">{t("settings.wizard.allowHttp")}</span>
                  <span className="text-[10px] leading-[1.3] text-foreground-secondary">{t("settings.wizard.allowHttpHint")}</span>
                </span>
                <Switch
                  id="wizard-allow-http"
                  checked={allowHttp}
                  onCheckedChange={setAllowHttp}
                />
              </label>

              {queryType === "script" && (
                <div className="flex flex-col gap-2">
                  <div className="flex items-center justify-between gap-2">
                    <label className={FIELD_LABEL_CLASS}>{t("settings.wizard.testSection")}</label>
                    <Button
                      type="button"
                      variant="soft"
                      size="xs"
                      disabled={testing || !scriptCode.trim()}
                      onClick={() => void handleTest()}
                    >
                      {testing ? t("settings.wizard.testing") : t("settings.wizard.test")}
                    </Button>
                  </div>
                  {testResult && (
                    <div
                      className={cn(
                        "rounded-lg border px-2.5 py-2 text-[11px]",
                        testResult.ok
                          ? "border-success-soft-border bg-success-soft-bg text-success-soft-text"
                          : "border-danger-soft-border bg-danger-soft-bg text-danger-soft-text",
                      )}
                    >
                      {testResult.message}
                    </div>
                  )}
                  <div className={FIELD_HINT_CLASS}>{t("settings.wizard.testHint")}</div>
                </div>
              )}

              {/* 配置摘要预览 */}
              <div className="mt-1 rounded-lg border border-border bg-surface-hover px-3 py-2.5">
                <div className="mb-1.5 text-[11px] font-semibold uppercase tracking-[0.04em] text-foreground-muted">
                  {t("settings.wizard.summaryTitle")}
                </div>
                <dl className="m-0 flex flex-col gap-1">
                  <div className="flex items-baseline gap-2 text-xs">
                    <dt className="w-[88px] shrink-0 text-foreground-muted">{t("settings.wizard.displayName")}</dt>
                    <dd className="m-0 flex-1 break-all text-foreground">{displayName.trim() || "-"}</dd>
                  </div>
                  <div className="flex items-baseline gap-2 text-xs">
                    <dt className="w-[88px] shrink-0 text-foreground-muted">{t("settings.wizard.baseUrl")}</dt>
                    <dd className="m-0 flex-1 break-all text-foreground">{baseUrl.trim() || "-"}</dd>
                  </div>
                  <div className="flex items-baseline gap-2 text-xs">
                    <dt className="w-[88px] shrink-0 text-foreground-muted">{t("settings.wizard.queryType")}</dt>
                    <dd className="m-0 flex-1 break-all text-foreground">{t("settings.wizard.queryTypeScript")}</dd>
                  </div>
                  <div className="flex items-baseline gap-2 text-xs">
                    <dt className="w-[88px] shrink-0 text-foreground-muted">{t("settings.wizard.authScheme")}</dt>
                    <dd className="m-0 flex-1 break-all text-foreground">
                      {t(AUTH_SCHEME_OPTIONS.find((option) => option.value === authScheme)?.labelKey ?? "settings.wizard.authBearer")}
                    </dd>
                  </div>
                  {(accessToken.trim() || userId.trim()) && (
                    <div className="flex items-baseline gap-2 text-xs">
                      <dt className="w-[88px] shrink-0 text-foreground-muted">{t("settings.wizard.credentialsSummary")}</dt>
                      <dd className="m-0 flex-1 break-all text-foreground">
                        {[
                          accessToken.trim() && t("settings.wizard.accessToken"),
                          userId.trim() && t("settings.wizard.userId"),
                        ]
                          .filter(Boolean)
                          .join(" / ")}
                      </dd>
                    </div>
                  )}
                </dl>
              </div>
            </>
          )}
        </div>

        <DialogFooter className="flex-row flex-wrap justify-end gap-2 sm:justify-end">
          <Button
            variant="softGhost"
            size="xs"
            className="min-w-[76px]"
            type="button"
            disabled={testing}
            onClick={handleClose}
          >
            {t("common.cancel")}
          </Button>

          {step > 1 && (
            <Button
              variant="softGhost"
              size="xs"
              className="min-w-[76px]"
              type="button"
              disabled={testing}
              onClick={() => setStep((current) => Math.max(1, current - 1))}
            >
              {t("settings.wizard.prev")}
            </Button>
          )}

          {step < 3 ? (
            <Button
              variant="soft"
              size="xs"
              className="min-w-[76px]"
              type="button"
              disabled={(step === 1 && !step1Valid) || (step === 2 && !step2Valid)}
              onClick={() => setStep((current) => Math.min(3, current + 1))}
            >
              {t("settings.wizard.next")}
            </Button>
          ) : (
            <Button
              variant="soft"
              size="xs"
              className="min-w-[76px]"
              type="button"
              disabled={testing || !step1Valid || !step2Valid}
              onClick={handleConfirm}
            >
              {t("settings.wizard.confirm")}
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

export default ProviderWizardDialog;
