import { useEffect, useState } from "react";
import { createPortal } from "react-dom";
import { useI18n } from "../../i18n";
import type {
  AuthSchemeConfig,
  CustomProviderConfig,
  ScriptConfig,
} from "../../types/provider";
import { getNewApiScriptTemplate, testCustomProviderScript } from "../../utils/ipc";
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

  // Esc 关闭
  useEffect(() => {
    if (!open) {
      return;
    }

    function handleKeyDown(event: KeyboardEvent) {
      if (!testing && event.key === "Escape") {
        handleClose();
      }
    }

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, testing]);

  if (!open || typeof document === "undefined") {
    return null;
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

  return createPortal(
    <div
      className="dialog-overlay wizard-overlay"
      onClick={(event) => {
        if (!testing && event.target === event.currentTarget) {
          handleClose();
        }
      }}
    >
      <div
        className="dialog-card wizard-card"
        aria-label={t("settings.wizard.ariaLabel")}
        aria-modal="true"
        role="dialog"
      >
        <div className="wizard-header">
          <span className="wizard-step-title">{t(stepTitleKey)}</span>
          <span className="wizard-step-indicator">
            {t("settings.wizard.stepIndicator", { current: step, total: 3 })}
          </span>
        </div>

        {/* 步骤进度条 */}
        <div className="wizard-progress" role="presentation">
          {[1, 2, 3].map((s) => (
            <div
              key={s}
              className={`wizard-progress-dot${s === step ? " is-active" : ""}${s < step ? " is-done" : ""}`}
            />
          ))}
        </div>

        <div className="wizard-body">
          {step === 1 && (
            <>
              <div className="field-group">
                <label className="field-label" htmlFor="wizard-display-name">
                  {t("settings.wizard.displayName")}
                </label>
                <input
                  id="wizard-display-name"
                  className="wizard-input"
                  type="text"
                  value={displayName}
                  placeholder={t("settings.wizard.displayNamePlaceholder")}
                  onChange={(event) => setDisplayName(event.target.value)}
                  autoFocus
                />
              </div>

              <div className="field-group">
                <label className="field-label">{t("settings.wizard.icon")}</label>
                <div className="wizard-icon-grid">
                  {ICON_CHOICES.map((iconChoice) => (
                    <button
                      key={iconChoice}
                      type="button"
                      className={`wizard-icon-option${icon === iconChoice ? " is-selected" : ""}`}
                      onClick={() => setIcon(iconChoice)}
                      title={iconChoice}
                      aria-pressed={icon === iconChoice}
                    >
                      <ProviderIcon providerId={iconChoice} size={22} />
                    </button>
                  ))}
                </div>
              </div>

              <div className="field-group">
                <label className="field-label">{t("settings.wizard.authScheme")}</label>
                <div className="wizard-segment" role="group" aria-label={t("settings.wizard.authScheme")}>
                  {AUTH_SCHEME_OPTIONS.map((option) => (
                    <button
                      key={option.value}
                      type="button"
                      className={`wizard-segment-button${authScheme === option.value ? " is-active" : ""}`}
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
              <div className="field-group">
                <label className="field-label">{t("settings.wizard.queryType")}</label>
                <div className="wizard-segment" role="group" aria-label={t("settings.wizard.queryType")}>
                  <button
                    type="button"
                    className="wizard-segment-button is-active"
                    aria-pressed={true}
                    disabled
                  >
                    {t("settings.wizard.queryTypeScript")}
                  </button>
                </div>
                <div className="field-hint">{t("settings.wizard.queryTypeScriptHint")}</div>
              </div>

              <div className="field-group">
                <label className="field-label" htmlFor="wizard-base-url">
                  {t("settings.wizard.baseUrl")}
                </label>
                <input
                  id="wizard-base-url"
                  className="wizard-input"
                  type="text"
                  value={baseUrl}
                  placeholder="https://your-gateway.example.com"
                  onChange={(event) => setBaseUrl(event.target.value)}
                />
                <div className="field-hint">{t("settings.wizard.baseUrlHint")}</div>
              </div>

              <div className="field-group">
                <div className="field-row">
                  <label className="field-label" htmlFor="wizard-script-code">
                    {t("settings.wizard.script")}
                  </label>
                  <button
                    type="button"
                    className="btn btn-sm btn-secondary"
                    disabled={loadingTemplate}
                    onClick={() => void fillNewApiTemplate()}
                  >
                    {loadingTemplate ? t("settings.wizard.loading") : t("settings.wizard.fillNewApiTemplate")}
                  </button>
                </div>
                <textarea
                  id="wizard-script-code"
                  className="wizard-textarea"
                  value={scriptCode}
                  rows={10}
                  spellCheck={false}
                  placeholder={t("settings.wizard.scriptPlaceholder")}
                  onChange={(event) => setScriptCode(event.target.value)}
                />
                <div className="field-hint">{t("settings.wizard.scriptHint")}</div>
              </div>

              {/* 修复 C-3：Script 模板可选的 accessToken / userId 输入框
                  （NewAPI 等 API 网关需要这两个凭据，通过 {{accessToken}} / {{userId}} 注入脚本） */}
              <div className="field-group">
                <label className="field-label" htmlFor="wizard-access-token">
                  {t("settings.wizard.accessToken")}
                </label>
                <input
                  id="wizard-access-token"
                  className="wizard-input"
                  type="text"
                  value={accessToken}
                  placeholder={t("settings.wizard.accessTokenPlaceholder")}
                  onChange={(event) => setAccessToken(event.target.value)}
                  autoComplete="off"
                />
                <div className="field-hint">{t("settings.wizard.accessTokenHint")}</div>
              </div>

              <div className="field-group">
                <label className="field-label" htmlFor="wizard-user-id">
                  {t("settings.wizard.userId")}
                </label>
                <input
                  id="wizard-user-id"
                  className="wizard-input"
                  type="text"
                  value={userId}
                  placeholder={t("settings.wizard.userIdPlaceholder")}
                  onChange={(event) => setUserId(event.target.value)}
                  autoComplete="off"
                />
                <div className="field-hint">{t("settings.wizard.userIdHint")}</div>
              </div>
            </>
          )}

          {step === 3 && (
            <>
              <div className="field-group">
                <label className="field-label" htmlFor="wizard-env-key-name">
                  {t("settings.wizard.envKeyName")}
                </label>
                <input
                  id="wizard-env-key-name"
                  className="wizard-input"
                  type="text"
                  value={envKeyName}
                  placeholder="CUSTOM_PROVIDER_API_KEY"
                  onChange={(event) => setEnvKeyName(event.target.value)}
                />
                <div className="field-hint">{t("settings.wizard.envKeyNameHint")}</div>
              </div>

              <label className="advanced-toggle">
                <span className="advanced-toggle-copy">
                  <span className="advanced-toggle-title">{t("settings.wizard.allowHttp")}</span>
                  <span className="advanced-toggle-hint">{t("settings.wizard.allowHttpHint")}</span>
                </span>
                <span className="switch">
                  <input
                    className="switch-input"
                    type="checkbox"
                    checked={allowHttp}
                    onChange={(event) => setAllowHttp(event.target.checked)}
                  />
                  <span className="switch-track" />
                </span>
              </label>

              {queryType === "script" && (
                <div className="field-group">
                  <div className="field-row">
                    <label className="field-label">{t("settings.wizard.testSection")}</label>
                    <button
                      type="button"
                      className="btn btn-sm btn-primary"
                      disabled={testing || !scriptCode.trim()}
                      onClick={() => void handleTest()}
                    >
                      {testing ? t("settings.wizard.testing") : t("settings.wizard.test")}
                    </button>
                  </div>
                  {testResult && (
                    <div className={`save-result ${testResult.ok ? "is-success" : "is-error"}`}>
                      {testResult.message}
                    </div>
                  )}
                  <div className="field-hint">{t("settings.wizard.testHint")}</div>
                </div>
              )}

              {/* 配置摘要预览 */}
              <div className="wizard-summary">
                <div className="wizard-summary-title">{t("settings.wizard.summaryTitle")}</div>
                <dl className="wizard-summary-list">
                  <div className="wizard-summary-item">
                    <dt>{t("settings.wizard.displayName")}</dt>
                    <dd>{displayName.trim() || "-"}</dd>
                  </div>
                  <div className="wizard-summary-item">
                    <dt>{t("settings.wizard.baseUrl")}</dt>
                    <dd>{baseUrl.trim() || "-"}</dd>
                  </div>
                  <div className="wizard-summary-item">
                    <dt>{t("settings.wizard.queryType")}</dt>
                    <dd>{t("settings.wizard.queryTypeScript")}</dd>
                  </div>
                  <div className="wizard-summary-item">
                    <dt>{t("settings.wizard.authScheme")}</dt>
                    <dd>
                      {t(AUTH_SCHEME_OPTIONS.find((option) => option.value === authScheme)?.labelKey ?? "settings.wizard.authBearer")}
                    </dd>
                  </div>
                  {(accessToken.trim() || userId.trim()) && (
                    <div className="wizard-summary-item">
                      <dt>{t("settings.wizard.credentialsSummary")}</dt>
                      <dd>
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

        <div className="dialog-actions wizard-actions">
          <button
            className="dialog-btn dialog-btn-secondary"
            type="button"
            disabled={testing}
            onClick={handleClose}
          >
            {t("common.cancel")}
          </button>

          {step > 1 && (
            <button
              className="dialog-btn dialog-btn-secondary"
              type="button"
              disabled={testing}
              onClick={() => setStep((current) => Math.max(1, current - 1))}
            >
              {t("settings.wizard.prev")}
            </button>
          )}

          {step < 3 ? (
            <button
              className="dialog-btn dialog-btn-primary"
              type="button"
              disabled={(step === 1 && !step1Valid) || (step === 2 && !step2Valid)}
              onClick={() => setStep((current) => Math.min(3, current + 1))}
            >
              {t("settings.wizard.next")}
            </button>
          ) : (
            <button
              className="dialog-btn dialog-btn-primary"
              type="button"
              disabled={testing || !step1Valid || !step2Valid}
              onClick={handleConfirm}
            >
              {t("settings.wizard.confirm")}
            </button>
          )}
        </div>
      </div>
    </div>,
    document.body,
  );
}

export default ProviderWizardDialog;
