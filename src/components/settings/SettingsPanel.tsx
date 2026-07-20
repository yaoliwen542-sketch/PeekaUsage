import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
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
import { Switch } from "@/components/ui/switch";
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

/* ===== 设置页共享视觉常量（与 ProviderConfig / UpdateSettings 中的同款类保持同步） ===== */
/** 分组标题：12px 大写弱色 */
const GROUP_TITLE_CLASS = "px-1 text-xs font-medium uppercase tracking-wide text-text-tertiary";
/** 分组卡片容器 */
const GROUP_CARD_CLASS = "overflow-hidden rounded-xl border border-border bg-card";
/** 组内行：行式布局，控件靠右 */
const SETTING_ROW_CLASS = "flex items-center justify-between gap-3 px-3.5 py-2.5";
/** 行主文案 */
const ROW_LABEL_CLASS = "text-[13px] leading-[1.35] text-text";
/** 行辅助说明 */
const ROW_HINT_CLASS = "text-xs leading-[1.4] text-text-muted";
/** 数值输入框（轮询间隔等） */
const INTERVAL_INPUT_CLASS = "h-7 w-14 shrink-0 rounded-lg border border-border bg-surface px-2 text-center text-xs text-text transition-colors duration-150 [appearance:textfield] hover:border-border-hover focus:border-primary-soft-border focus:outline-none focus:ring-1 focus:ring-primary/40 [&::-webkit-inner-spin-button]:appearance-none [&::-webkit-outer-spin-button]:appearance-none";

/** 开关行：整行可点击，右侧为统一 Switch */
function ToggleRow(props: {
  label: string;
  hint?: string;
  checked: boolean;
  disabled?: boolean;
  onChange: (checked: boolean) => void;
}) {
  const { label, hint, checked, disabled, onChange } = props;
  return (
    <label className={`${SETTING_ROW_CLASS} ${disabled ? "cursor-default" : "cursor-pointer"}`}>
      <span className="flex min-w-0 flex-col gap-0.5">
        <span className={ROW_LABEL_CLASS}>{label}</span>
        {hint && <span className={ROW_HINT_CLASS}>{hint}</span>}
      </span>
      <Switch
        checked={checked}
        disabled={disabled}
        aria-label={label}
        onCheckedChange={onChange}
      />
    </label>
  );
}

/** 分段控件：与新子导航一致的 segmented 风格 */
function SegmentedControl<T extends string>(props: {
  options: Array<{ value: T; label: string }>;
  value: T;
  ariaLabel: string;
  onChange: (value: T) => void;
}) {
  const { options, value, ariaLabel, onChange } = props;
  return (
    <div className="inline-flex gap-0.5 rounded-full bg-ghost p-0.5" role="group" aria-label={ariaLabel}>
      {options.map((option) => {
        const isActive = option.value === value;
        return (
          <button
            key={option.value}
            className={`flex h-6 items-center justify-center rounded-full px-2.5 text-[11.5px] whitespace-nowrap transition-colors duration-150 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-primary/50 ${
              isActive
                ? "bg-surface-elevated font-medium text-text shadow-sm"
                : "text-text-secondary hover:text-text"
            }`}
            type="button"
            aria-pressed={isActive}
            onClick={() => onChange(option.value)}
          >
            {option.label}
          </button>
        );
      })}
    </div>
  );
}

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
  const { t, language } = useI18n();
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

  // 已配置供应商集合与可选模板：memo 化保证引用稳定，
  // 否则每次渲染都生成新数组，会连带击穿下游 draft 的 useMemo
  const configuredProviderIds = useMemo(
    () => new Set(providerConfigs.map((item) => item.providerId)),
    [providerConfigs],
  );
  // 可选模板：排除已配置的内置供应商
  const availableTemplates = useMemo(
    () => providerTemplates.filter((item) => !configuredProviderIds.has(item.id)),
    [providerTemplates, configuredProviderIds],
  );
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
  // providerId 由调用方生成并传入，保证同一份 customConfig 在重渲染间 id 稳定
  function buildDraftFromCustomConfig(customConfig: CustomProviderConfig, providerId: ProviderId): ProviderConfigItem {
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

  // 自定义供应商草稿 id：只在 customConfig 变化时生成一次，
  // 避免父组件重渲染（如后台更新检查返回）导致 id 重新生成、表单被重置
  const customDraftIdRef = useRef<{ config: CustomProviderConfig; providerId: ProviderId } | null>(null);

  // 当前选中的草稿配置（优先自定义，其次内置模板）。
  // memo 化保证草稿对象引用稳定：ProviderConfig 的同步 effect 依赖 config，
  // 引用不稳会把用户正在输入的表单重置回初始值
  const draftProviderConfig: ProviderConfigItem | null = useMemo(() => {
    if (pendingCustomConfig) {
      if (customDraftIdRef.current?.config !== pendingCustomConfig) {
        // 自定义供应商 ID 用固定前缀，后端会根据 customConfig 落盘
        customDraftIdRef.current = {
          config: pendingCustomConfig,
          providerId: `custom_${Date.now().toString(36)}`,
        };
      }
      return buildDraftFromCustomConfig(pendingCustomConfig, customDraftIdRef.current.providerId);
    }
    customDraftIdRef.current = null;
    if (creatingProviderId) {
      const template = availableTemplates.find((item) => item.id === creatingProviderId);
      if (template) {
        return buildDraftFromTemplate(template);
      }
    }
    return null;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pendingCustomConfig, creatingProviderId, availableTemplates, language]);

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

  // 创建模式卡片内「选择供应商」下拉的数据源。
  // 修复：之前误传空数组，导致下拉没有任何选项（打不开、点不动），
  // 且当前选中的供应商（如 kimi）无法回显、永远显示占位文案。
  // 自定义草稿本身也要并入选项，否则自定义流程中选中项同样无法回显。
  const createModeSelectableProviders = useMemo<ProviderConfigItem[]>(() => {
    const templateDrafts = availableTemplates.map((template) => buildDraftFromTemplate(template));
    if (pendingCustomConfig && draftProviderConfig) {
      return [...templateDrafts, draftProviderConfig];
    }
    return templateDrafts;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [availableTemplates, pendingCustomConfig, draftProviderConfig, language]);

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
      <section className="flex flex-col gap-3">
        {generalNotice && (
          <div
            className="rounded-lg border border-success-soft-border bg-success-soft-bg px-3 py-2 text-xs text-success-soft-text"
            role="status"
          >
            {generalNotice}
          </div>
        )}

        {/* 偏好：语言 / 全局刷新 / 透明度 */}
        <div className="flex flex-col gap-1.5">
          <h3 className={GROUP_TITLE_CLASS}>{t("settings.groups.preferences")}</h3>
          <div className={`${GROUP_CARD_CLASS} divide-y divide-border`}>
            <div className={SETTING_ROW_CLASS}>
              <span className={ROW_LABEL_CLASS}>{t("settings.language.label")}</span>
              <div className="w-[148px] shrink-0">
                <AppSelect
                  modelValue={settings.language}
                  options={languageOptions}
                  ariaLabel={t("settings.language.ariaLabel")}
                  onChange={(value) => void saveSettings({ language: value })}
                />
              </div>
            </div>

            <div className={`${SETTING_ROW_CLASS} flex-wrap gap-y-2`}>
              <span className={ROW_LABEL_CLASS}>{t("settings.polling.label")}</span>
              {/* 控件组内部绝不换行：空间不足时整组落到下一行右对齐，
                  避免「自动/手动」「5」「秒/分」被拆散挤压成混乱多行 */}
              <div className="ml-auto flex max-w-full flex-nowrap items-center justify-end gap-1.5">
                <SegmentedControl
                  options={pollingModeOptions}
                  value={settings.pollingMode}
                  ariaLabel={t("settings.polling.modeAriaLabel")}
                  onChange={(value) => void saveSettings({ pollingMode: value })}
                />
                {!isManualPolling && (
                  <>
                    <input
                      className={INTERVAL_INPUT_CLASS}
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
                    <SegmentedControl
                      options={pollingUnitOptions}
                      value={settings.pollingUnit}
                      ariaLabel={t("settings.polling.unitAriaLabel")}
                      onChange={(value) => void saveSettings({ pollingUnit: value })}
                    />
                  </>
                )}
              </div>
            </div>

            <div className={SETTING_ROW_CLASS}>
              <label className={`${ROW_LABEL_CLASS} shrink-0`} htmlFor="window-opacity-range">{t("settings.opacity.label")}</label>
              <div className="flex min-w-0 flex-1 items-center gap-2">
                <input
                  id="window-opacity-range"
                  className="opacity-slider h-1.5 min-w-0 flex-1 cursor-pointer appearance-none rounded-full bg-border"
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
                <span className="w-9 shrink-0 text-right text-xs font-medium tabular-nums text-text-secondary">{opacityDraft}%</span>
              </div>
            </div>
          </div>
        </div>

        {/* 窗口与行为 */}
        <div className="flex flex-col gap-1.5">
          <h3 className={GROUP_TITLE_CLASS}>{t("settings.groups.windowBehavior")}</h3>
          <div className={`${GROUP_CARD_CLASS} divide-y divide-border`}>
            <ToggleRow
              label={t("settings.launchAtStartup.label")}
              hint={t("settings.launchAtStartup.hint")}
              checked={settings.launchAtStartup}
              disabled={launchAtStartupPending}
              onChange={(checked) => void handleLaunchAtStartupChange(checked)}
            />
            <ToggleRow
              label={t("settings.hideTaskbarIcon.label")}
              hint={isWindows
                ? t("settings.hideTaskbarIcon.hint")
                : t("settings.hideTaskbarIcon.unsupportedHint")}
              checked={isWindows ? settings.hideTaskbarIcon : false}
              disabled={!isWindows || hideTaskbarPending || launchAtStartupPending}
              onChange={(checked) => void handleHideTaskbarIconChange(checked)}
            />
            <ToggleRow
              label={t("settings.refreshOnBack.label")}
              hint={t("settings.refreshOnBack.hint")}
              checked={settings.refreshOnSettingsClose}
              onChange={(checked) => void saveSettings({ refreshOnSettingsClose: checked })}
            />
            <ToggleRow
              label={t("settings.autoExpandWindow.label")}
              hint={t("settings.autoExpandWindow.hint")}
              checked={settings.autoExpandWindowToFitContent}
              onChange={(checked) => void saveSettings({ autoExpandWindowToFitContent: checked })}
            />
            <ToggleRow
              label={t("settings.edgeDockCollapse.label")}
              hint={t("settings.edgeDockCollapse.hint")}
              checked={settings.edgeDockCollapseEnabled}
              onChange={(checked) => void saveSettings({ edgeDockCollapseEnabled: checked })}
            />
            <ToggleRow
              label={t("settings.compactColorMarkers.label")}
              hint={t("settings.compactColorMarkers.hint")}
              checked={settings.compactColorMarkersEnabled}
              onChange={(checked) => void saveSettings({ compactColorMarkersEnabled: checked })}
            />
            <ToggleRow
              label={t("settings.islandVisible.label")}
              hint={t("settings.islandVisible.hint")}
              checked={settings.islandVisible}
              onChange={(checked) => void saveSettings({ islandVisible: checked })}
            />
          </div>
        </div>
      </section>
    ),
    providers: (
      <section className="flex flex-col gap-3">
        <div className="flex items-center justify-between gap-2">
          <h3 className={GROUP_TITLE_CLASS}>{t("settings.sections.providers")}</h3>
          {!draftProviderConfig && providerSelectGroups.some((group) => group.options.length > 0) && (
            <div className="w-[172px] max-w-[56%] shrink-0">
              <AppSelect
                modelValue={providerSelectValue}
                groups={providerSelectGroups}
                placeholder={t("settings.providerSelect.addPlaceholder")}
                ariaLabel={t("settings.providerSelect.addPlaceholder")}
                triggerClassName="min-h-7 rounded-lg px-2.5 py-1"
                onChange={(value) => handleProviderSelectChange(value)}
                renderSelected={(option) => (
                  <span className={`flex min-w-0 items-center gap-1.5 ${option ? "" : "text-text-muted"}`}>
                    {option ? (
                      <>
                        <ProviderIcon providerId={option.icon ?? option.value} size={16} />
                        <span className="truncate text-xs text-text">{option.label}</span>
                        {option.badge && (
                          <span className="shrink-0 rounded-full border border-primary-soft-border bg-primary-soft-bg px-1.5 py-px text-[9px] font-semibold text-primary-soft-text">
                            {option.badge}
                          </span>
                        )}
                      </>
                    ) : (
                      <span className="truncate text-xs">{t("settings.providerSelect.addPlaceholder")}</span>
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
            selectableProviders={createModeSelectableProviders}
            onProviderChange={(providerId) => {
              // 选中当前草稿自身时不动作；切到其它模板则放弃自定义草稿
              if (providerId === draftProviderConfig.providerId) {
                return;
              }
              setPendingCustomConfig(null);
              setCreatingProviderId(providerId);
            }}
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
          <div className="rounded-xl border border-dashed border-primary-soft-border bg-ghost p-4 text-center text-xs text-text-secondary">
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
      <section className="flex flex-col gap-3">
        <div className={GROUP_CARD_CLASS}>
          <ToggleRow
            label={t("settings.advancedSection.title")}
            hint={t("settings.advancedSection.hint")}
            checked={settings.providerPollingOverridesEnabled}
            onChange={(checked) => void saveSettings({ providerPollingOverridesEnabled: checked })}
          />
        </div>

        {settings.providerPollingOverridesEnabled && configuredPollingProviders.length === 0 && (
          <div className="rounded-xl border border-dashed border-primary-soft-border bg-ghost p-4 text-center text-xs text-text-secondary">
            <span>{t("settings.advancedSection.empty")}</span>
          </div>
        )}

        {settings.providerPollingOverridesEnabled && configuredPollingProviders.length > 0 && (
          <div className={`${GROUP_CARD_CLASS} divide-y divide-border`}>
            {configuredPollingProviders.map((config) => {
              const providerPollingSettings = getProviderPollingSettings(config.providerId);

              return (
                <div key={config.providerId} className="flex flex-col gap-2 px-3.5 py-2.5">
                  <div className="flex min-w-0 items-center gap-2">
                    <ProviderIcon providerId={config.providerId} size={16} />
                    <span className="truncate text-[13px] font-medium text-text">{config.displayName}</span>
                  </div>

                  <div className="flex flex-nowrap items-center gap-1.5">
                    <SegmentedControl
                      options={pollingModeOptions}
                      value={providerPollingSettings.pollingMode}
                      ariaLabel={`${config.displayName} ${t("settings.polling.modeAriaLabel")}`}
                      onChange={(value) => void saveProviderPollingSettings(config.providerId, { pollingMode: value })}
                    />

                    {providerPollingSettings.pollingMode !== "manual" && (
                      <>
                        <input
                          className={INTERVAL_INPUT_CLASS}
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
                        <SegmentedControl
                          options={pollingUnitOptions}
                          value={providerPollingSettings.pollingUnit}
                          ariaLabel={`${config.displayName} ${t("settings.polling.unitAriaLabel")}`}
                          onChange={(value) => void saveProviderPollingSettings(config.providerId, { pollingUnit: value })}
                        />
                      </>
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
      {/* 头部：返回 + 标题（单行，不再渲染子页副标题） */}
      <header className="flex h-11 shrink-0 items-center gap-1.5 border-b border-border px-3">
        <button
          className="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg text-text-secondary transition-colors duration-150 hover:bg-ghost-hover hover:text-text focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-primary/50"
          type="button"
          aria-label={t("common.back")}
          onClick={onBack}
        >
          <BackIcon />
        </button>
        <h1 className="truncate text-[15px] font-semibold text-text">{t("settings.title")}</h1>
      </header>

      {/* 子导航：segmented 控件 */}
      <div className="shrink-0 border-b border-border px-3 py-1.5">
        <div className="flex gap-0.5 rounded-full bg-ghost p-0.5" role="tablist" aria-label={t("settings.navigationAriaLabel")}>
          {sectionItems.map((item) => {
            const isActive = item.id === activeSection;

            return (
              <button
                key={item.id}
                className={`flex h-7 flex-1 items-center justify-center gap-1 rounded-full px-2 text-[12.5px] whitespace-nowrap transition-colors duration-150 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-primary/50 ${
                  isActive
                    ? "bg-surface-elevated font-medium text-text shadow-sm"
                    : "text-text-secondary hover:text-text"
                }`}
                type="button"
                role="tab"
                aria-selected={isActive}
                onClick={() => handleSectionSelect(item.id)}
              >
                <span className="truncate">{item.label}</span>
                {item.id === "updates" && hasUpdate && (
                  <span className="h-1.5 w-1.5 shrink-0 rounded-full bg-danger" aria-hidden="true" />
                )}
              </button>
            );
          })}
        </div>
      </div>

      <div className="flex-1 overflow-y-auto px-3 py-3">
        {sectionContent[activeSection]}
      </div>
    </div>
  );
}
