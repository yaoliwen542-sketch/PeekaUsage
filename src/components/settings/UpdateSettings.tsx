import { useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useI18n } from "../../i18n";
import { useSettingsStore } from "../../stores/settingsStore";
import { useUpdateStore } from "../../stores/updateStore";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";

/* ===== 视觉常量（与 SettingsPanel 中的同款类保持同步） ===== */
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

/** 开关行：整行可点击，右侧为统一 Switch */
function ToggleRow(props: {
  label: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
}) {
  const { label, checked, onChange } = props;
  return (
    <label className={`${SETTING_ROW_CLASS} cursor-pointer`}>
      <span className={ROW_LABEL_CLASS}>{label}</span>
      <Switch checked={checked} aria-label={label} onCheckedChange={onChange} />
    </label>
  );
}

export default function UpdateSettings() {
  const { t } = useI18n();
  const settings = useSettingsStore((state) => state.settings);
  const saveSettings = useSettingsStore((state) => state.saveSettings);

  const { status, isChecking, isInstalling, lastCheckAt, checkUpdate, installUpdate } =
    useUpdateStore();

  const [intervalDraft, setIntervalDraft] = useState<string>(
    String(settings.updateCheckIntervalHours),
  );

  useEffect(() => {
    setIntervalDraft(String(settings.updateCheckIntervalHours));
  }, [settings.updateCheckIntervalHours]);

  function formatLastCheckAt(ts: number | null): string {
    if (!ts) return t("settings.updates.never");
    const date = new Date(ts);
    return date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  }

  async function handleViewChangelog() {
    const version = status?.currentVersion ?? "";
    const url = `https://github.com/yaoliwen542-sketch/PeekaUsage/releases/tag/v${version}`;
    try {
      await openUrl(url);
    } catch (e) {
      console.error("open url failed", e);
    }
  }

  async function handleViewReleaseUrl() {
    if (!status?.releaseUrl) return;
    try {
      await openUrl(status.releaseUrl);
    } catch (e) {
      console.error("open url failed", e);
    }
  }

  function getUpdateButtonLabel(): string {
    if (isInstalling) return t("settings.updates.installing");
    if (status?.state === "available" && status.availableVersion) {
      return `${t("settings.updates.updateTo")} v${status.availableVersion}`;
    }
    return t("settings.updates.upToDate");
  }

  function getCheckStatusLabel(): string {
    if (isChecking) return t("settings.updates.checking");
    if (!status || status.state === "idle") return "";
    if (status.state === "upToDate") return t("settings.updates.upToDate");
    if (status.state === "available") {
      return `${t("settings.updates.available")}: v${status.availableVersion}`;
    }
    if (status.state === "error") {
      return `${t("settings.updates.error")}: ${status.errorMessage ?? ""}`;
    }
    return "";
  }

  const hasUpdate = status?.state === "available";

  return (
    <section className="flex flex-col gap-3">
      {/* 版本与更新 */}
      <div className="flex flex-col gap-1.5">
        <h3 className={GROUP_TITLE_CLASS}>{t("settings.updates.title")}</h3>
        <div className={`${GROUP_CARD_CLASS} divide-y divide-border`}>
          {/* 当前版本 */}
          <div className={SETTING_ROW_CLASS}>
            <span className={ROW_LABEL_CLASS}>{t("settings.updates.currentVersion")}</span>
            <div className="flex shrink-0 items-center gap-2">
              <span className="text-[13px] font-medium tabular-nums text-text">{status?.currentVersion ?? "—"}</span>
              <Button
                variant="softGhost"
                size="xs"
                type="button"
                onClick={() => void handleViewChangelog()}
              >
                {t("settings.updates.viewChangelog")}
              </Button>
            </div>
          </div>

          {/* 检查更新 */}
          <div className={`${SETTING_ROW_CLASS} flex-wrap gap-y-2`}>
            <div className="flex min-w-0 flex-col gap-0.5">
              {lastCheckAt != null && (
                <span className={ROW_HINT_CLASS}>
                  {t("settings.updates.lastCheckAt")}: {formatLastCheckAt(lastCheckAt)}
                </span>
              )}
              {!isChecking && getCheckStatusLabel() && (
                <span
                  className={`text-xs ${
                    hasUpdate
                      ? "font-medium text-success"
                      : status?.state === "error"
                        ? "text-danger"
                        : "text-text-secondary"
                  }`}
                >
                  {getCheckStatusLabel()}
                </span>
              )}
            </div>
            <Button
              variant="soft"
              size="xs"
              type="button"
              disabled={isChecking}
              onClick={() => void checkUpdate()}
            >
              {isChecking ? t("settings.updates.checking") : t("settings.updates.checkUpdate")}
            </Button>
          </div>

          {/* 安装更新 */}
          <div className="px-3.5 py-2.5">
            <Button
              variant={hasUpdate ? "soft" : "softGhost"}
              size="sm"
              className="w-full"
              type="button"
              disabled={!hasUpdate || isInstalling}
              onClick={() => void installUpdate()}
            >
              {getUpdateButtonLabel()}
            </Button>
          </div>
        </div>
      </div>

      {/* 有更新时展示说明 */}
      {hasUpdate && status?.notes && (
        <div className={`${GROUP_CARD_CLASS} px-3.5 py-2.5`}>
          <div className="mb-1 text-xs font-semibold uppercase tracking-wide text-text-tertiary">
            {t("settings.updates.releaseNotes")}
          </div>
          <div className="mb-1.5 max-h-28 overflow-y-auto whitespace-pre-line text-xs leading-[1.6] text-text-secondary">
            {status.notes}
          </div>
          {status.releaseUrl && (
            <Button
              variant="softGhost"
              size="xs"
              type="button"
              onClick={() => void handleViewReleaseUrl()}
            >
              {t("settings.updates.viewChangelog")}
            </Button>
          )}
        </div>
      )}

      {/* 自动检查设置 */}
      <div className="flex flex-col gap-1.5">
        <h3 className={GROUP_TITLE_CLASS}>{t("settings.updates.autoCheck")}</h3>
        <div className={`${GROUP_CARD_CLASS} divide-y divide-border`}>
          <ToggleRow
            label={t("settings.updates.autoCheck")}
            checked={settings.updateAutoCheckEnabled}
            onChange={(checked) => void saveSettings({ updateAutoCheckEnabled: checked })}
          />

          {settings.updateAutoCheckEnabled && (
            <>
              <ToggleRow
                label={t("settings.updates.checkOnLaunch")}
                checked={settings.updateCheckOnLaunch}
                onChange={(checked) => void saveSettings({ updateCheckOnLaunch: checked })}
              />

              <div className={SETTING_ROW_CLASS}>
                <span className={ROW_LABEL_CLASS}>{t("settings.updates.checkInterval")}</span>
                <div className="flex shrink-0 items-center gap-1.5">
                  <input
                    className="h-7 w-14 shrink-0 rounded-lg border border-border bg-surface px-2 text-center text-xs text-text transition-colors duration-150 [appearance:textfield] hover:border-border-hover focus:border-primary-soft-border focus:outline-none focus:ring-1 focus:ring-primary/40 [&::-webkit-inner-spin-button]:appearance-none [&::-webkit-outer-spin-button]:appearance-none"
                    type="number"
                    inputMode="numeric"
                    min={1}
                    max={168}
                    value={intervalDraft}
                    aria-label={t("settings.updates.checkInterval")}
                    onChange={(e) => setIntervalDraft(e.target.value)}
                    onBlur={() => {
                      const parsed = Number.parseInt(intervalDraft, 10);
                      const val = Number.isNaN(parsed) ? 2 : Math.max(1, Math.min(168, parsed));
                      setIntervalDraft(String(val));
                      if (val !== settings.updateCheckIntervalHours) {
                        void saveSettings({ updateCheckIntervalHours: val });
                      }
                    }}
                    onKeyDown={(e) => {
                      if (e.key === "Enter") (e.target as HTMLInputElement).blur();
                    }}
                  />
                  <span className="text-xs text-text-secondary">{t("settings.updates.checkIntervalUnit")}</span>
                </div>
              </div>
            </>
          )}
        </div>
        {settings.updateAutoCheckEnabled && (
          <p className="px-1 text-xs leading-[1.5] text-text-muted">{t("settings.updates.checkIntervalHint")}</p>
        )}
      </div>
    </section>
  );
}
