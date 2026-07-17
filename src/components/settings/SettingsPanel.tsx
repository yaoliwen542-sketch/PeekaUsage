import { useEffect, useMemo, useState, type ReactNode } from "react";
import { useI18n } from "../../i18n";
import type { CustomProviderConfig, ProviderConfigItem, ProviderId, ProviderTemplate } from "../../types/provider";
import {
  MAX_POLLING_INTERVAL,
  MIN_POLLING_INTERVAL,
  getEffectivePollingSettings,
  normalizePollingInterval,
  type AppLanguage,
  type PollingMode,
  type PollingSettings,
  type PollingUnit,
} from "../../types/settings";
import { useWindowControls } from "../../composables/useWindowControls";
import { LANGUAGE_OPTIONS } from "../../i18n/messages";
import { useProviderStore } from "../../stores/providerStore";
import { useSettingsStore } from "../../stores/settingsStore";
import { syncLaunchAtStartup } from "../../utils/autostart";
import { getProviderConfigs, getProviderTemplates, setWindowSkipTaskbar } from "../../utils/ipc";
import AppSelect, { type AppSelectGroup, type SelectOption } from "../common/AppSelect";
import ProviderIcon from "../common/ProviderIcon";
import ProviderConfig from "./ProviderConfig";
import { ProviderWizardDialog } from "./ProviderWizardDialog";
import UpdateSettings from "./UpdateSettings";
import { useUpdateStore } from "../../stores/updateStore";

type SettingsPanelProps = {
  onBack: () => void;
};

type SettingsSectionId = "general" | "providers" | "advanced" | "updates";

type SettingsMenuItem =
  {
    id: SettingsSectionId;
    label: string;
  };

function BackIcon() {
  return (
    <svg
      className="h-4 w-4"
      viewBox="0 0 16 16"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
      aria-hidden="true"
    >
      <path
        d="M9.5 3.5L5 8l4.5 4.5"
        stroke="currentColor"
        strokeWidth="1.8"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

export default function SettingsPanel({ onBack }: SettingsPanelProps) {
  const settings = useSettingsStore((state) => state.settings);
  const saveSettings = useSettingsStore((state) => state.saveSettings);
  const { updateOpacity } = useWindowControls();
  const { t } = useI18n();
  const hasUpdate = useUpdateStore((state) => state.hasUpdate);
  const [providerConfigs, setProviderConfigs] = useState<ProviderConfigItem[]>([]);
  const [providerTemplates, setProviderTemplates] = useState<ProviderTemplate[]>([]);
  const [creatingProviderId, setCreatingProviderId] = useState<ProviderId | null>(null);
  const [pendingCustomConfig, setPendingCustomConfig] = useState<CustomProviderConfig | null>(null);
  const [isWizardOpen, setIsWizardOpen] = useState(false);
  const [opacityDraft, setOpacityDraft] = useState(settings.windowOpacity);
  const [launchAtStartupPending, setLaunchAtStartupPending] = useState(false);
  const [hideTaskbarPending, setHideTaskbarPending] = useState(false);
  const [generalNotice, setGeneralNotice] = useState<string | null>(null);
  const [pollingIntervalDraft, setPollingIntervalDraft] = useState(String(settings.pollingInterval));
  const [providerPollingIntervalDrafts, setProviderPollingIntervalDrafts] = useState<Partial<Record<ProviderId, string>>>({});
  const [activeSection, setActiveSection] = useState<SettingsSectionId>("general");
  const isWindows = useMemo(
    () => typeof navigator !== "undefined" && /windows/i.test(navigator.userAgent),
    [],
  );

  const languageOptions: Array<SelectOption<AppLanguage>> = LANGUAGE_OPTIONS.map((item) => ({
    value: item.value,
    label: item.label,
  }));

  const pollingModeOptions: Array<SelectOption<PollingMode>> = [
    { value: "auto", label: t("settings.polling.auto") },
    { value: "manual", label: t("settings.polling.manual") },
  ];

  const pollingUnitOptions: Array<SelectOption<PollingUnit>> = [
    { value: "seconds", label: t("common.secondsShort") },
    { value: "minutes", label: t("common.minutesShort") },
  ];

  const sectionItems: SettingsMenuItem[] = useMemo(() => ([
    { id: "general", label: t("settings.sections.general") },
    { id: "providers", label: t("settings.sections.providers") },
    { id: "advanced", label: t("settings.sections.advanced") },
    { id: "updates", label: t("settings.sections.updates") },
  ]), [t]);

  const activeSectionLabel = sectionItems.find((item) => item.id === activeSection)?.label
    ?? t("settings.sections.general");

  const configuredProviderIds = new Set(providerConfigs.map((item) => item.providerId));
  // 可选模板：排除已配置的内置供应商
  const availableTemplates = providerTemplates.filter((item) => !configuredProviderIds.has(item.id));
  const isManualPolling = settings.pollingMode === "manual";
  const configuredPollingProviders = providerConfigs.filter((item) => item.enabled);

  // 从模板构造草稿 ProviderConfigItem（用于创建模式渲染）
  function buildDraftFromTemplate(template: ProviderTemplate): ProviderConfigItem {
    return {
      providerId: template.id,
      displayName: template.displayName,
      enabled: true,
      apiKeys: [
        {
          id: `${template.id}-draft-key`,
          name: t("settings.providerConfig.keyName", { index: 1 }),
          color: "#3b82f6",
          value: "",
          isActiveInEnvironment: false,
        },
      ],
      subscriptions: template.capabilities.hasSubscription
        ? [
            {
              id: `${template.id}-draft-subscription`,
              name: t("settings.providerConfig.subscriptionName", { index: 1 }),
              color: "#3b82f6",
              oauthToken: "",
              source: null,
            },
          ]
        : [],
      capabilities: template.capabilities,
      environmentVariableName: template.envKeyName,
      activeApiKeyId: null,
      providerTemplateId: template.id,
      customConfig: null,
    };
  }

  // 从自定义配置构造草稿 ProviderConfigItem
  function buildDraftFromCustomConfig(customConfig: CustomProviderConfig): ProviderConfigItem {
    // 自定义供应商 ID 用固定前缀，后端会根据 customConfig 落盘
    const providerId = `custom_${Date.now().toString(36)}`;
    return {
      providerId,
      displayName: customConfig.displayName,
      enabled: true,
      apiKeys: [
        {
          id: `${providerId}-draft-key`,
          name: t("settings.providerConfig.keyName", { index: 1 }),
          color: "#3b82f6",
          value: "",
          isActiveInEnvironment: false,
        },
      ],
      subscriptions: [],
      capabilities: {
        hasBalance: customConfig.queryType === "balance",
        hasUsage: false,
        hasRateLimit: false,
        hasSubscription: false,
      },
      environmentVariableName: customConfig.envKeyName ?? "",
      activeApiKeyId: null,
      providerTemplateId: null,
      customConfig,
    };
  }

  // 当前选中的草稿配置（优先自定义，其次内置模板）
  const draftProviderConfig: ProviderConfigItem | null = (() => {
    if (pendingCustomConfig) {
      return buildDraftFromCustomConfig(pendingCustomConfig);
    }
    if (creatingProviderId) {
      const template = availableTemplates.find((item) => item.id === creatingProviderId);
      if (template) {
        return buildDraftFromTemplate(template);
      }
    }
    return null;
  })();

  // 构造"新增供应商"分组下拉
  const providerSelectGroups: Array<AppSelectGroup<string>> = useMemo(() => {
    const subscriptionOptions: Array<SelectOption<string>> = availableTemplates
      .filter((item) => item.capabilities.hasSubscription)
      .map((item) => ({
        value: item.id,
        label: item.displayName,
        icon: item.icon,
        badge: t("settings.providerSelect.badgeSubscription"),
      }));
    // 用量查询类：CodingPlan / Subscription 之外的 usage 查询（如 Kimi / GLM / MiniMax / 火山方舟）
    const usageOptions: Array<SelectOption<string>> = availableTemplates
      .filter((item) => !item.capabilities.hasSubscription
        && !item.capabilities.hasBalance
        && item.queries.some((q) => q.queryType.kind === "coding_plan"))
      .map((item) => ({
        value: item.id,
        label: item.displayName,
        icon: item.icon,
        badge: t("settings.providerSelect.badgeUsage"),
      }));
    const balanceOptions: Array<SelectOption<string>> = availableTemplates
      .filter((item) => !item.capabilities.hasSubscription && item.capabilities.hasBalance)
      .map((item) => ({
        value: item.id,
        label: item.displayName,
        icon: item.icon,
        badge: t("settings.providerSelect.badgeBalance"),
      }));
    const gatewayOptions: Array<SelectOption<string>> = availableTemplates
      .filter((item) => item.queries.some((q) => q.queryType.kind === "script"))
      .map((item) => ({
        value: item.id,
        label: item.displayName,
        icon: item.icon,
        badge: t("settings.providerSelect.badgeGateway"),
      }));
    const customOption: SelectOption<string> = {
      value: "__custom__",
      label: t("settings.providerSelect.customProvider"),
      icon: "custom",
      badge: t("settings.providerSelect.badgeCustom"),
    };

    const groups: Array<AppSelectGroup<string>> = [];
    if (subscriptionOptions.length > 0) {
      groups.push({ label: t("settings.providerSelect.groupSubscription"), options: subscriptionOptions });
    }
    if (usageOptions.length > 0) {
      groups.push({ label: t("settings.providerSelect.groupUsage"), options: usageOptions });
    }
    if (balanceOptions.length > 0) {
      groups.push({ label: t("settings.providerSelect.groupBalance"), options: balanceOptions });
    }
    if (gatewayOptions.length > 0) {
      groups.push({ label: t("settings.providerSelect.groupGateway"), options: gatewayOptions });
    }
    // 自定义入口始终展示
    groups.push({ label: t("settings.providerSelect.groupCustom"), options: [customOption] });
    return groups;
  }, [availableTemplates, t]);

  // 当前"新增供应商"下拉的选中值（自定义流程中无内置选中）
  const providerSelectValue: string = pendingCustomConfig ? "__custom__" : (creatingProviderId ?? "");

  function handleProviderSelectChange(value: string) {
    if (value === "__custom__") {
      // 打开自定义供应商向导
      setPendingCustomConfig(null);
      setIsWizardOpen(true);
      return;
    }
    setPendingCustomConfig(null);
    setCreatingProviderId(value);
  }

  function handleWizardConfirm(config: CustomProviderConfig) {
    setIsWizardOpen(false);
    setCreatingProviderId(null);
    setPendingCustomConfig(config);
  }

  function handleWizardClose() {
    setIsWizardOpen(false);
  }

  async function loadProviderData() {
    try {
      const [configs, templates] = await Promise.all([
        getProviderConfigs(),
        getProviderTemplates(),
      ]);

      setProviderConfigs(configs);
      setProviderTemplates(templates);

      setCreatingProviderId((current) => {
        if (!current) {
          return current;
        }

        const configuredSet = new Set(configs.map((config) => config.providerId));
        const nextAvailable = templates.filter((item) => !configuredSet.has(item.id));
        return nextAvailable.some((item) => item.id === current)
          ? current
          : nextAvailable[0]?.id ?? null;
      });
    } catch {
      setProviderConfigs([]);
      setProviderTemplates([]);
    }
  }

  function getProviderPollingSettings(providerId: ProviderId): PollingSettings {
    return getEffectivePollingSettings(settings, providerId);
  }

  function syncProviderPollingDrafts() {
    const nextDrafts: Partial<Record<ProviderId, string>> = {};

    for (const item of configuredPollingProviders) {
      nextDrafts[item.providerId] = String(getProviderPollingSettings(item.providerId).pollingInterval);
    }

    setProviderPollingIntervalDrafts(nextDrafts);
  }

  async function saveProviderPollingSettings(
    providerId: ProviderId,
    nextSettings: Partial<PollingSettings>,
  ) {
    const current = getProviderPollingSettings(providerId);
    await saveSettings({
      providerPollingOverrides: {
        ...settings.providerPollingOverrides,
        [providerId]: {
          ...current,
          ...nextSettings,
        },
      },
    });
  }

  async function reloadProviders() {
    await loadProviderData();
    await useProviderStore.getState().refreshAll();
  }

  function showHideTaskbarNotice() {
    setGeneralNotice(t("settings.hideTaskbarIcon.notice"));
  }

  async function handleHideTaskbarIconChange(enabled: boolean) {
    if (!isWindows || hideTaskbarPending || enabled === settings.hideTaskbarIcon) {
      return;
    }

    setHideTaskbarPending(true);

    try {
      await setWindowSkipTaskbar(enabled);
      const shouldShowNotice = enabled && !settings.hideTaskbarIconHintShown;
      await saveSettings({
        hideTaskbarIcon: enabled,
        hideTaskbarIconHintShown: settings.hideTaskbarIconHintShown || shouldShowNotice,
      });

      if (shouldShowNotice) {
        showHideTaskbarNotice();
      }
    } catch (error) {
      try {
        await setWindowSkipTaskbar(settings.hideTaskbarIcon);
      } catch (rollbackError) {
        console.error("回滚任务栏图标状态失败：", rollbackError);
      }

      console.error("切换任务栏图标状态失败：", error);
    } finally {
      setHideTaskbarPending(false);
    }
  }

  async function handleLaunchAtStartupChange(enabled: boolean) {
    if (launchAtStartupPending || enabled === settings.launchAtStartup) {
      return;
    }

    setLaunchAtStartupPending(true);

    try {
      await syncLaunchAtStartup(enabled);
      const shouldLinkHideTaskbar = isWindows && enabled && !settings.hideTaskbarIcon;

      if (shouldLinkHideTaskbar) {
        await setWindowSkipTaskbar(true);
      }

      const shouldShowNotice = shouldLinkHideTaskbar && !settings.hideTaskbarIconHintShown;
      await saveSettings({
        launchAtStartup: enabled,
        ...(shouldLinkHideTaskbar
          ? {
              hideTaskbarIcon: true,
              hideTaskbarIconHintShown: settings.hideTaskbarIconHintShown || shouldShowNotice,
            }
          : {}),
      });

      if (shouldShowNotice) {
        showHideTaskbarNotice();
      }
    } catch (error) {
      try {
        await syncLaunchAtStartup(settings.launchAtStartup);
      } catch (rollbackError) {
        console.error("回滚开机自动启动状态失败：", rollbackError);
      }

      console.error("同步开机自动启动失败：", error);
    } finally {
      setLaunchAtStartupPending(false);
    }
  }

  function handleSectionSelect(sectionId: SettingsSectionId) {
    setActiveSection(sectionId);
  }

  useEffect(() => {
    void loadProviderData();
  }, []);

  useEffect(() => {
    setOpacityDraft(settings.windowOpacity);
  }, [settings.windowOpacity]);

  useEffect(() => {
    setPollingIntervalDraft(String(settings.pollingInterval));
  }, [settings.pollingInterval]);

  useEffect(() => {
    syncProviderPollingDrafts();
  }, [
    providerConfigs,
    settings.providerPollingOverrides,
    settings.providerPollingOverridesEnabled,
    settings.pollingInterval,
    settings.pollingMode,
    settings.pollingUnit,
  ]);

  useEffect(() => {
    if (!generalNotice) {
      return undefined;
    }

    const timer = window.setTimeout(() => {
      setGeneralNotice(null);
    }, 5000);

    return () => window.clearTimeout(timer);
  }, [generalNotice]);

  const sectionContent: Record<SettingsSectionId, ReactNode> = {
    general: (
      <section className="settings-section settings-section-page">
        <h3 className="text-[11px] font-semibold uppercase tracking-[0.5px] text-text-tertiary">{t("settings.sections.general")}</h3>
        {generalNotice && (
          <div className="save-result is-success" role="status">
            {generalNotice}
          </div>
        )}
        <div className="setting-row">
          <label>{t("settings.language.label")}</label>
          <div className="setting-select-wrap">
            <AppSelect
              modelValue={settings.language}
              options={languageOptions}
              ariaLabel={t("settings.language.ariaLabel")}
              onChange={(value) => void saveSettings({ language: value })}
            />
          </div>
        </div>

        <div className="setting-row setting-row-polling">
          <label>{t("settings.polling.label")}</label>
          <div className="polling-control">
            <div className="polling-segment" role="group" aria-label={t("settings.polling.modeAriaLabel")}>
              {pollingModeOptions.map((option) => (
                <button
                  key={option.value}
                  className={`polling-segment-button${settings.pollingMode === option.value ? " is-active" : ""}`}
                  type="button"
                  aria-pressed={settings.pollingMode === option.value}
                  onClick={() => void saveSettings({ pollingMode: option.value })}
                >
                  {option.label}
                </button>
              ))}
            </div>
            {!isManualPolling && (
              <div className="polling-auto-inline">
                <input
                  className="polling-interval-input"
                  type="number"
                  inputMode="numeric"
                  min={MIN_POLLING_INTERVAL}
                  max={MAX_POLLING_INTERVAL}
                  value={pollingIntervalDraft}
                  aria-label={t("settings.polling.intervalAriaLabel")}
                  onChange={(event) => setPollingIntervalDraft(event.target.value)}
                  onBlur={() => {
                    const parsed = Number.parseInt(pollingIntervalDraft, 10);
                    const nextValue = normalizePollingInterval(
                      Number.isNaN(parsed) ? settings.pollingInterval : parsed,
                    );
                    setPollingIntervalDraft(String(nextValue));

                    if (nextValue !== settings.pollingInterval) {
                      void saveSettings({ pollingInterval: nextValue });
                    }
                  }}
                  onKeyDown={(event) => {
                    if (event.key === "Enter") {
                      (event.target as HTMLInputElement).blur();
                    }
                  }}
                />
                <div className="polling-segment polling-unit-segment" role="group" aria-label={t("settings.polling.unitAriaLabel")}>
                  {pollingUnitOptions.map((option) => (
                    <button
                      key={option.value}
                      className={`polling-segment-button${settings.pollingUnit === option.value ? " is-active" : ""}`}
                      type="button"
                      aria-pressed={settings.pollingUnit === option.value}
                      onClick={() => void saveSettings({ pollingUnit: option.value })}
                    >
                      {option.label}
                    </button>
                  ))}
                </div>
              </div>
            )}
          </div>
        </div>

        <div className="setting-row setting-row-slider">
          <label htmlFor="window-opacity-range">{t("settings.opacity.label")}</label>
          <div className="flex items-center gap-2">
            <input
              id="window-opacity-range"
              className="h-1.5 flex-1 cursor-pointer appearance-none rounded-full bg-border"
              type="range"
              min="10"
              max="100"
              step="1"
              value={opacityDraft}
              onInput={(event) => {
                const value = Number.parseInt((event.target as HTMLInputElement).value, 10);
                setOpacityDraft(value);
                void updateOpacity(value, false);
              }}
              onChange={(event) => {
                const value = Number.parseInt((event.target as HTMLInputElement).value, 10);
                setOpacityDraft(value);
                void updateOpacity(value, true);
              }}
            />
            <span className="text-xs font-medium text-text-secondary w-10 text-right">{opacityDraft}%</span>
          </div>
        </div>

        <label className="setting-row setting-row-toggle">
          <span className="flex flex-col gap-0.5">
            <span className="text-xs font-medium text-text">{t("settings.launchAtStartup.label")}</span>
            <span className="text-[11px] text-text-tertiary">{t("settings.launchAtStartup.hint")}</span>
          </span>
          <span className="switch">
            <input
              className="switch-input"
              type="checkbox"
              checked={settings.launchAtStartup}
              disabled={launchAtStartupPending}
              onChange={(event) => void handleLaunchAtStartupChange(event.target.checked)}
            />
            <span className="switch-track" />
          </span>
        </label>

        <label className="setting-row setting-row-toggle">
          <span className="flex flex-col gap-0.5">
            <span className="text-xs font-medium text-text">{t("settings.hideTaskbarIcon.label")}</span>
            <span className="text-[11px] text-text-tertiary">
              {isWindows
                ? t("settings.hideTaskbarIcon.hint")
                : t("settings.hideTaskbarIcon.unsupportedHint")}
            </span>
          </span>
          <span className="switch">
            <input
              className="switch-input"
              type="checkbox"
              checked={isWindows ? settings.hideTaskbarIcon : false}
              disabled={!isWindows || hideTaskbarPending || launchAtStartupPending}
              onChange={(event) => void handleHideTaskbarIconChange(event.target.checked)}
            />
            <span className="switch-track" />
          </span>
        </label>

        <label className="setting-row setting-row-toggle">
          <span className="flex flex-col gap-0.5">
            <span className="text-xs font-medium text-text">{t("settings.refreshOnBack.label")}</span>
            <span className="text-[11px] text-text-tertiary">{t("settings.refreshOnBack.hint")}</span>
          </span>
          <span className="switch">
            <input
              className="switch-input"
              type="checkbox"
              checked={settings.refreshOnSettingsClose}
              onChange={(event) => void saveSettings({ refreshOnSettingsClose: event.target.checked })}
            />
            <span className="switch-track" />
          </span>
        </label>

        <label className="setting-row setting-row-toggle">
          <span className="flex flex-col gap-0.5">
            <span className="text-xs font-medium text-text">{t("settings.autoExpandWindow.label")}</span>
            <span className="text-[11px] text-text-tertiary">{t("settings.autoExpandWindow.hint")}</span>
          </span>
          <span className="switch">
            <input
              className="switch-input"
              type="checkbox"
              checked={settings.autoExpandWindowToFitContent}
              onChange={(event) => void saveSettings({ autoExpandWindowToFitContent: event.target.checked })}
            />
            <span className="switch-track" />
          </span>
        </label>

        <label className="setting-row setting-row-toggle">
          <span className="flex flex-col gap-0.5">
            <span className="text-xs font-medium text-text">{t("settings.edgeDockCollapse.label")}</span>
            <span className="text-[11px] text-text-tertiary">{t("settings.edgeDockCollapse.hint")}</span>
          </span>
          <span className="switch">
            <input
              className="switch-input"
              type="checkbox"
              checked={settings.edgeDockCollapseEnabled}
              onChange={(event) => void saveSettings({ edgeDockCollapseEnabled: event.target.checked })}
            />
            <span className="switch-track" />
          </span>
        </label>

        <label className="setting-row setting-row-toggle">
          <span className="flex flex-col gap-0.5">
            <span className="text-xs font-medium text-text">{t("settings.compactColorMarkers.label")}</span>
            <span className="text-[11px] text-text-tertiary">{t("settings.compactColorMarkers.hint")}</span>
          </span>
          <span className="switch">
            <input
              className="switch-input"
              type="checkbox"
              checked={settings.compactColorMarkersEnabled}
              onChange={(event) => void saveSettings({ compactColorMarkersEnabled: event.target.checked })}
            />
            <span className="switch-track" />
          </span>
        </label>
      </section>
    ),
    providers: (
      <section className="settings-section settings-section-page">
        <div className="flex items-center justify-between gap-2">
          <h3 className="text-[11px] font-semibold uppercase tracking-[0.5px] text-text-tertiary">{t("settings.sections.providers")}</h3>
          {!draftProviderConfig && providerSelectGroups.some((group) => group.options.length > 0) && (
            <div className="add-provider-select">
              <AppSelect
                modelValue={providerSelectValue}
                groups={providerSelectGroups}
                placeholder={t("settings.providerSelect.addPlaceholder")}
                ariaLabel={t("settings.providerSelect.addPlaceholder")}
                className="provider-add-select"
                onChange={(value) => handleProviderSelectChange(value)}
                renderSelected={(option) => (
                  <span className={`provider-select-value${option ? "" : " is-placeholder"}`}>
                    {option ? (
                      <>
                        <ProviderIcon providerId={option.icon ?? option.value} size={18} />
                        <span className="provider-select-text">{option.label}</span>
                        {option.badge && <span className="provider-select-badge">{option.badge}</span>}
                      </>
                    ) : (
                      <span className="provider-select-text">{t("settings.providerSelect.addPlaceholder")}</span>
                    )}
                  </span>
                )}
                renderOption={({ option }) => (
                  <span className="select-option-meta">
                    <span className="select-option-meta-icon">
                      <ProviderIcon providerId={option.icon ?? option.value} size={18} />
                    </span>
                    <span className="select-option-meta-text">
                      <span className="select-option-meta-label">{option.label}</span>
                      {option.description && (
                        <span className="select-option-meta-description">{option.description}</span>
                      )}
                    </span>
                    {option.badge && <span className="select-option-badge">{option.badge}</span>}
                  </span>
                )}
              />
            </div>
          )}
        </div>

        {draftProviderConfig && (
          <ProviderConfig
            config={draftProviderConfig}
            expanded
            mode="create"
            selectableProviders={[]}
            onCanceled={() => {
              setCreatingProviderId(null);
              setPendingCustomConfig(null);
            }}
            onSaved={() => void (async () => {
              setCreatingProviderId(null);
              setPendingCustomConfig(null);
              await reloadProviders();
            })()}
          />
        )}

        {providerConfigs.length === 0 && !draftProviderConfig && (
          <div className="border border-dashed border-primary/30 rounded-md p-4 text-xs text-text-secondary text-center">
            <span>{t("settings.providersSection.empty")}</span>
          </div>
        )}

        {providerConfigs.map((config) => (
          <ProviderConfig
            key={config.providerId}
            config={config}
            expanded={settings.providerCardExpanded[config.providerId] ?? true}
            onExpandedChange={(expanded) => void saveSettings({
              providerCardExpanded: {
                ...settings.providerCardExpanded,
                [config.providerId]: expanded,
              },
            })}
            onSaved={() => void (async () => {
              setCreatingProviderId(null);
              setPendingCustomConfig(null);
              await reloadProviders();
            })()}
            onRemoved={() => void reloadProviders()}
            onEnvironmentChanged={() => loadProviderData()}
          />
        ))}

        <ProviderWizardDialog
          open={isWizardOpen}
          onClose={handleWizardClose}
          onConfirm={handleWizardConfirm}
        />
      </section>
    ),
    advanced: (
      <section className="settings-section settings-section-page">
        <div className="flex items-center justify-between gap-2">
          <h3 className="text-[11px] font-semibold uppercase tracking-[0.5px] text-text-tertiary">{t("settings.sections.advanced")}</h3>
        </div>

        <label className="flex items-center justify-between gap-2 rounded-md border border-border bg-surface px-3 py-2 transition-colors hover:border-border-strong">
          <span className="flex flex-col gap-0.5">
            <span className="text-xs font-medium text-text">{t("settings.advancedSection.title")}</span>
            <span className="text-[11px] text-text-tertiary">{t("settings.advancedSection.hint")}</span>
          </span>
          <span className="switch">
            <input
              className="switch-input"
              type="checkbox"
              checked={settings.providerPollingOverridesEnabled}
              onChange={(event) => void saveSettings({ providerPollingOverridesEnabled: event.target.checked })}
            />
            <span className="switch-track" />
          </span>
        </label>

        {settings.providerPollingOverridesEnabled && configuredPollingProviders.length === 0 && (
          <div className="border border-dashed border-primary/30 rounded-md p-4 text-xs text-text-secondary text-center">
            <span>{t("settings.advancedSection.empty")}</span>
          </div>
        )}

        {settings.providerPollingOverridesEnabled && configuredPollingProviders.length > 0 && (
          <div className="provider-polling-list">
            {configuredPollingProviders.map((config) => {
              const providerPollingSettings = getProviderPollingSettings(config.providerId);

              return (
                <div key={config.providerId} className="provider-polling-item">
                  <div className="provider-polling-meta">
                    <ProviderIcon providerId={config.providerId} size={16} />
                    <span className="provider-polling-name">{config.displayName}</span>
                  </div>

                  <div className="polling-control polling-control-compact">
                    <div
                      className="polling-segment polling-segment-compact"
                      role="group"
                      aria-label={`${config.displayName} ${t("settings.polling.modeAriaLabel")}`}
                    >
                      {pollingModeOptions.map((option) => (
                        <button
                          key={option.value}
                          className={`polling-segment-button polling-segment-button-compact${providerPollingSettings.pollingMode === option.value ? " is-active" : ""}`}
                          type="button"
                          aria-pressed={providerPollingSettings.pollingMode === option.value}
                          onClick={() => void saveProviderPollingSettings(config.providerId, { pollingMode: option.value })}
                        >
                          {option.label}
                        </button>
                      ))}
                    </div>

                    {providerPollingSettings.pollingMode !== "manual" && (
                      <div className="polling-auto-inline polling-auto-inline-compact">
                        <input
                          className="polling-interval-input polling-interval-input-compact"
                          type="number"
                          inputMode="numeric"
                          min={MIN_POLLING_INTERVAL}
                          max={MAX_POLLING_INTERVAL}
                          value={providerPollingIntervalDrafts[config.providerId] ?? String(providerPollingSettings.pollingInterval)}
                          aria-label={`${config.displayName} ${t("settings.polling.intervalAriaLabel")}`}
                          onChange={(event) => setProviderPollingIntervalDrafts((current) => ({
                            ...current,
                            [config.providerId]: event.target.value,
                          }))}
                          onBlur={() => {
                            const rawValue = providerPollingIntervalDrafts[config.providerId] ?? "";
                            const parsed = Number.parseInt(rawValue, 10);
                            const nextValue = normalizePollingInterval(
                              Number.isNaN(parsed) ? providerPollingSettings.pollingInterval : parsed,
                            );

                            setProviderPollingIntervalDrafts((current) => ({
                              ...current,
                              [config.providerId]: String(nextValue),
                            }));

                            if (nextValue !== providerPollingSettings.pollingInterval) {
                              void saveProviderPollingSettings(config.providerId, { pollingInterval: nextValue });
                            }
                          }}
                          onKeyDown={(event) => {
                            if (event.key === "Enter") {
                              (event.target as HTMLInputElement).blur();
                            }
                          }}
                        />
                        <div
                          className="polling-segment polling-unit-segment polling-segment-compact"
                          role="group"
                          aria-label={`${config.displayName} ${t("settings.polling.unitAriaLabel")}`}
                        >
                          {pollingUnitOptions.map((option) => (
                            <button
                              key={option.value}
                              className={`polling-segment-button polling-segment-button-compact${providerPollingSettings.pollingUnit === option.value ? " is-active" : ""}`}
                              type="button"
                              aria-pressed={providerPollingSettings.pollingUnit === option.value}
                              onClick={() => void saveProviderPollingSettings(config.providerId, { pollingUnit: option.value })}
                            >
                              {option.label}
                            </button>
                          ))}
                        </div>
                      </div>
                    )}
                  </div>
                </div>
              );
            })}
          </div>
        )}
      </section>
    ),
    updates: <UpdateSettings />,
  };

  return (
    <div className="flex flex-1 flex-col overflow-hidden bg-background">
      <div className="settings-header">
        <button
          className="flex h-7 w-7 items-center justify-center rounded-md text-text-secondary transition-colors hover:bg-surface-elevated hover:text-text"
          type="button"
          aria-label={t("common.back")}
          onClick={onBack}
        >
          <BackIcon />
        </button>

        <div className="settings-title-group">
          <span className="settings-title">{t("settings.title")}</span>
          <span className="settings-subtitle">{activeSectionLabel}</span>
        </div>
      </div>

      <div className="settings-subnav" role="tablist" aria-label={t("settings.navigationAriaLabel")}>
        {sectionItems.map((item) => {
          const isActive = item.id === activeSection;

          return (
            <button
              key={item.id}
              className={`settings-subnav-item${isActive ? " is-active" : ""}`}
              type="button"
              role="tab"
              aria-selected={isActive}
              onClick={() => handleSectionSelect(item.id)}
            >
              <span className="settings-subnav-label">{item.label}</span>
              {item.id === "updates" && hasUpdate && (
                <span className="settings-subnav-badge" aria-hidden="true" />
              )}
            </button>
          );
        })}
      </div>

      <div className="settings-body">
        {sectionContent[activeSection]}
      </div>
    </div>
  );
}
