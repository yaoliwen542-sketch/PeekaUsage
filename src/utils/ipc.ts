import { invoke } from "@tauri-apps/api/core";
import type {
  UsageSummary,
  ProviderConfigItem,
  ProviderId,
  ProviderApiKeyItem,
  ProviderSubscriptionItem,
  ProviderTemplate,
  CustomProviderConfig,
} from "../types/provider";
import type {
  AppDataSnapshot,
  AppSettings,
  WebDavSyncConfig,
} from "../types/settings";
import type { StatsRange, UsageStatsSnapshot } from "../types/stats";

/** 获取所有供应商用量摘要 */
export async function fetchAllUsage(): Promise<UsageSummary[]> {
  return invoke<UsageSummary[]>("fetch_all_usage");
}

/** 获取单个供应商用量摘要 */
export async function fetchProviderUsage(providerId: ProviderId): Promise<UsageSummary> {
  return invoke<UsageSummary>("fetch_provider_usage", { providerId });
}

/** 获取已添加的供应商配置列表 */
export async function getProviderConfigs(): Promise<ProviderConfigItem[]> {
  return invoke<ProviderConfigItem[]>("get_provider_configs");
}

/** 获取支持的供应商列表 */
export async function getSupportedProviders(): Promise<ProviderConfigItem[]> {
  return invoke<ProviderConfigItem[]>("get_supported_providers");
}

/** 保存供应商配置 */
export async function saveProviderConfig(config: {
  providerId: ProviderId;
  apiKeys: Array<Pick<ProviderApiKeyItem, "id" | "name" | "color" | "value">>;
  subscriptions: Array<Pick<ProviderSubscriptionItem, "id" | "name" | "color" | "oauthToken" | "source">>;
  enabled: boolean;
  providerTemplateId?: string | null;
  customConfig?: CustomProviderConfig | null;
}): Promise<void> {
  return invoke("save_provider_config", { config });
}

/** 获取所有可选供应商模板（含内置，用于设置页"新增供应商"下拉） */
export async function getProviderTemplates(): Promise<ProviderTemplate[]> {
  return invoke<ProviderTemplate[]>("get_provider_templates");
}

/** 获取 NewAPI 预置脚本模板 */
export async function getNewApiScriptTemplate(): Promise<string> {
  return invoke<string>("get_newapi_script_template");
}

/** 测试自定义供应商脚本（保存前预演） */
export async function testCustomProviderScript(
  code: string,
  apiKey: string,
  baseUrl: string | null,
  allowHttp: boolean,
  accessToken?: string | null,
  userId?: string | null,
): Promise<string> {
  return invoke<string>("test_custom_provider_script", {
    code,
    apiKey,
    baseUrl,
    allowHttp,
    accessToken: accessToken ?? null,
    userId: userId ?? null,
  });
}

/** 移除供应商配置 */
export async function removeProviderConfig(providerId: ProviderId): Promise<void> {
  return invoke("remove_provider_config", { providerId });
}

export async function saveProviderOrder(order: ProviderId[]): Promise<void> {
  return invoke("save_provider_order", { order });
}

export async function getUsageStatsSnapshot(range: StatsRange): Promise<UsageStatsSnapshot> {
  return invoke<UsageStatsSnapshot>("get_usage_stats_snapshot", { range });
}

/** 激活某个 API Key 并同步到系统环境变量 */
export async function activateProviderApiKey(providerId: ProviderId, apiKeyId: string): Promise<void> {
  return invoke("activate_provider_api_key", { providerId, apiKeyId });
}

/** 验证 API Key */
export async function validateApiKey(
  providerId: ProviderId,
  apiKey: string,
  customConfig?: CustomProviderConfig | null,
): Promise<boolean> {
  return invoke<boolean>("validate_api_key", {
    providerId,
    apiKey,
    customConfig: customConfig ?? null,
  });
}

/** 获取应用设置 */
export async function getSettings(): Promise<AppSettings> {
  return invoke<AppSettings>("get_settings");
}

/** 保存应用设置 */
export async function saveSettings(settings: AppSettings): Promise<void> {
  return invoke("save_settings", { settings });
}

/** 设置是否隐藏 Windows 任务栏图标 */
export async function setWindowSkipTaskbar(skip: boolean): Promise<void> {
  return invoke("set_window_skip_taskbar", { skip });
}

/** 检测到的 OAuth Token */
export interface DetectedToken {
  token: string;
  source: string;
  subscriptionType: string | null;
  environment: "windows" | "wsl" | "native";
  displaySource: string;
  /** OpenAI/Codex 的 account_id（用于 ChatGPT-Account-Id header，多账号场景）；Anthropic 恒为 null */
  accountId?: string | null;
}

/** 检测到的 Token 集合 */
export interface DetectedTokens {
  anthropic: DetectedToken[];
  openai: DetectedToken[];
  /** Gemini OAuth 凭据（token 字段存完整 oauth_creds.json 文本） */
  gemini?: DetectedToken[];
}

/** 自动检测本地 OAuth Token */
export async function detectOAuthTokens(): Promise<DetectedTokens> {
  return invoke<DetectedTokens>("detect_oauth_tokens");
}

/** 导出应用配置快照（配置、密钥、统计） */
export async function exportAppData(): Promise<AppDataSnapshot> {
  return invoke<AppDataSnapshot>("export_app_data");
}

/** 导出应用快照到系统下载目录，返回实际文件路径 */
export async function exportAppDataToDownloads(): Promise<string> {
  return invoke<string>("export_app_data_to_downloads");
}

/** 用快照覆盖应用全部配置 */
export async function importAppData(snapshot: AppDataSnapshot): Promise<void> {
  return invoke("import_app_data", { snapshot });
}

/** 上传快照到 WebDAV */
function buildWebdavEndpoint(syncConfig: WebDavSyncConfig): string {
  const remoteRoot = syncConfig.remoteRoot.trim().replace(/\\/g, "/");
  if (!remoteRoot) {
    return syncConfig.endpoint;
  }

  const segments = remoteRoot.split("/").filter(Boolean);
  if (segments.some((segment) => segment === "." || segment === "..")) {
    throw new Error("WebDAV 远程根目录不能包含 . 或 .. 路径段");
  }

  const endpoint = new URL(syncConfig.endpoint);
  const basePath = endpoint.pathname.replace(/\/+$/, "");
  endpoint.pathname = `${basePath}/${segments.join("/")}/`;
  return endpoint.toString();
}

export async function uploadAppDataToWebdav(
  snapshot: AppDataSnapshot,
  syncConfig: WebDavSyncConfig,
): Promise<void> {
  return invoke("upload_app_data_to_webdav_with_root", {
    snapshot,
    syncConfig: {
      endpoint: syncConfig.endpoint,
      username: syncConfig.username,
      password: syncConfig.password,
    },
    remoteRoot: syncConfig.remoteRoot,
  });
}

/** 从 WebDAV 下载快照 */
export async function downloadAppDataFromWebdav(syncConfig: WebDavSyncConfig): Promise<AppDataSnapshot> {
  return invoke<AppDataSnapshot>("download_app_data_from_webdav", {
    syncConfig: {
      endpoint: buildWebdavEndpoint(syncConfig),
      username: syncConfig.username,
      password: syncConfig.password,
    },
  });
}

/** 获取当前设备保存的 WebDAV 密码 */
export async function getWebdavSyncPassword(): Promise<string> {
  return invoke<string>("get_webdav_sync_password");
}

/** 将 WebDAV 密码保存到当前设备的 KeyStore */
export async function saveWebdavSyncPassword(password: string): Promise<void> {
  return invoke("save_webdav_sync_password", { password });
}
