use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

/// 应用设置
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub polling_interval: u32,
    #[serde(default)]
    pub polling_mode: PollingMode,
    #[serde(default)]
    pub polling_unit: PollingUnit,
    #[serde(default)]
    pub provider_polling_overrides_enabled: bool,
    #[serde(default)]
    pub provider_polling_overrides: HashMap<String, PollingSettings>,
    #[serde(default)]
    pub compact_color_markers_enabled: bool,
    #[serde(default)]
    pub refresh_on_settings_close: bool,
    #[serde(default)]
    pub auto_expand_window_to_fit_content: bool,
    #[serde(default = "default_edge_dock_collapse_enabled")]
    pub edge_dock_collapse_enabled: bool,
    #[serde(default = "default_island_visible")]
    pub island_visible: bool,
    #[serde(default)]
    pub hide_taskbar_icon: bool,
    #[serde(default)]
    pub hide_taskbar_icon_hint_shown: bool,
    #[serde(default)]
    pub language: AppLanguage,
    #[serde(default)]
    pub widget_display_mode: WidgetDisplayMode,
    pub always_on_top: bool,
    pub launch_at_startup: bool,
    #[serde(default = "default_update_auto_check_enabled")]
    pub update_auto_check_enabled: bool,
    #[serde(default = "default_update_check_on_launch")]
    pub update_check_on_launch: bool,
    #[serde(default = "default_update_check_interval_hours")]
    pub update_check_interval_hours: u32,
    pub window_opacity: f64,
    #[serde(default)]
    pub theme: ThemeMode,
    pub window_position: Option<WindowPosition>,
    pub window_size: Option<WindowSize>,
    #[serde(default = "default_provider_card_expanded")]
    pub provider_card_expanded: HashMap<String, bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum PollingMode {
    #[default]
    Auto,
    Manual,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum PollingUnit {
    Seconds,
    #[default]
    Minutes,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PollingSettings {
    pub polling_interval: u32,
    #[serde(default)]
    pub polling_mode: PollingMode,
    #[serde(default)]
    pub polling_unit: PollingUnit,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum ThemeMode {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum AppLanguage {
    #[default]
    #[serde(rename = "zh-Hans")]
    ZhHans,
    #[serde(rename = "zh-Hant")]
    ZhHant,
    #[serde(rename = "en")]
    En,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum WidgetDisplayMode {
    #[default]
    Detailed,
    Compact,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowPosition {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowSize {
    pub width: f64,
    pub height: f64,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            polling_interval: 5,
            polling_mode: PollingMode::default(),
            polling_unit: PollingUnit::default(),
            provider_polling_overrides_enabled: false,
            provider_polling_overrides: HashMap::new(),
            compact_color_markers_enabled: false,
            refresh_on_settings_close: false,
            auto_expand_window_to_fit_content: true,
            edge_dock_collapse_enabled: default_edge_dock_collapse_enabled(),
            island_visible: default_island_visible(),
            hide_taskbar_icon: false,
            hide_taskbar_icon_hint_shown: false,
            language: AppLanguage::default(),
            widget_display_mode: WidgetDisplayMode::default(),
            always_on_top: true,
            launch_at_startup: false,
            window_opacity: 100.0,
            theme: ThemeMode::default(),
            window_position: None,
            window_size: None,
            provider_card_expanded: default_provider_card_expanded(),
            update_auto_check_enabled: default_update_auto_check_enabled(),
            update_check_on_launch: default_update_check_on_launch(),
            update_check_interval_hours: default_update_check_interval_hours(),
        }
    }
}

fn default_edge_dock_collapse_enabled() -> bool {
    true
}

fn default_island_visible() -> bool {
    // 旧配置缺省时默认显示灵动岛，保证老用户升级后行为不变
    true
}

impl AppSettings {
    pub fn normalized(mut self) -> Self {
        self.polling_interval = self.polling_interval.clamp(1, 999);
        self.provider_polling_overrides = self
            .provider_polling_overrides
            .into_iter()
            .filter_map(|(provider_id, settings)| {
                if is_supported_provider_id(&provider_id, None) {
                    Some((provider_id, settings.normalized()))
                } else {
                    None
                }
            })
            .collect();
        #[cfg(not(windows))]
        {
            self.hide_taskbar_icon = false;
        }
        self.window_opacity = self.window_opacity.clamp(10.0, 100.0);
        self
    }
}

impl PollingSettings {
    pub fn normalized(mut self) -> Self {
        self.polling_interval = self.polling_interval.clamp(1, 999);
        self
    }
}

fn default_update_auto_check_enabled() -> bool {
    true
}

fn default_update_check_on_launch() -> bool {
    true
}

fn default_update_check_interval_hours() -> u32 {
    2
}

fn default_provider_card_expanded() -> HashMap<String, bool> {
    // 内置供应商按 registry 全部默认展开
    let mut map = HashMap::new();
    for template in crate::providers::registry::all() {
        map.insert(template.id, true);
    }
    map
}

/// 判断 provider_id 是否受支持
///
/// 规则：
/// - 内置供应商在 registry 里 -> 支持
/// - "custom_" 前缀 -> 支持
/// - 入参 entry 有 custom_config -> 支持
fn is_supported_provider_id(provider_id: &str, entry: Option<&ProviderEntry>) -> bool {
    // 内置供应商在 registry 里
    if crate::providers::registry::get(provider_id).is_some() {
        return true;
    }
    // 自定义供应商
    if provider_id.starts_with("custom_") {
        return true;
    }
    // 有 custom_config 的也算
    if let Some(entry) = entry {
        return entry.custom_config.is_some();
    }
    false
}

/// 供应商下单个 API Key 的元数据
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderApiKeyEntry {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub color: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSubscriptionEntry {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub color: String,
    #[serde(default)]
    pub source: Option<String>,
}

/// 供应商持久化配置
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderEntry {
    pub provider_id: String,
    pub enabled: bool,
    #[serde(default)]
    pub api_keys: Vec<ProviderApiKeyEntry>,
    #[serde(default)]
    pub subscriptions: Vec<ProviderSubscriptionEntry>,
    #[serde(default)]
    pub active_api_key_id: Option<String>,
    #[serde(default)]
    pub manage_api_key_environment: bool,
    // 新增：内置供应商模板 ID（自定义供应商为 None）
    #[serde(default)]
    pub provider_template_id: Option<String>,
    // 新增：自定义供应商配置（内置供应商为 None）
    #[serde(default)]
    pub custom_config: Option<crate::providers::types::CustomProviderConfig>,
}

/// 配置文件内容
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigFile {
    pub settings: AppSettings,
    pub providers: HashMap<String, ProviderEntry>,
    #[serde(default)]
    pub provider_order: Vec<String>,
}

impl Default for ConfigFile {
    fn default() -> Self {
        Self {
            settings: AppSettings::default(),
            providers: HashMap::new(),
            provider_order: Vec::new(),
        }
    }
}

/// 应用配置管理
pub struct AppConfig {
    config: Arc<RwLock<ConfigFile>>,
    config_path: PathBuf,
}

impl AppConfig {
    pub fn new(app_data_dir: PathBuf) -> Self {
        let config_path = app_data_dir.join("config.json");
        let config = if config_path.exists() {
            match std::fs::read_to_string(&config_path) {
                Ok(content) => match serde_json::from_str::<ConfigFile>(&content) {
                    Ok(mut config) => {
                        config.settings = config.settings.normalized();
                        config
                    }
                    Err(error) => {
                        // 修复 M9：解析失败不能静默重置为默认配置。
                        // 先把损坏文件备份为 config.json.bak（用户可手动恢复），
                        // 再回退默认配置，避免用户数据彻底丢失。
                        eprintln!("解析 config.json 失败: {}，回退默认配置", error);
                        crate::config::atomic::backup_corrupted_file(&config_path);
                        ConfigFile::default()
                    }
                },
                Err(_) => ConfigFile::default(),
            }
        } else {
            ConfigFile::default()
        };

        Self {
            config: Arc::new(RwLock::new(config)),
            config_path,
        }
    }

    /// 保存配置到文件
    async fn save(&self) -> Result<(), String> {
        let config = self.config.read().await;
        let content =
            serde_json::to_string_pretty(&*config).map_err(|e| format!("序列化配置失败: {}", e))?;

        if let Some(parent) = self.config_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("创建配置目录失败: {}", e))?;
        }

        // 修复 M9：原子写入（tmp + rename），写入中途崩溃不会留下半截 config.json
        crate::config::atomic::atomic_write(&self.config_path, content.as_bytes())
            .map_err(|e| format!("写入配置文件失败: {}", e))?;

        Ok(())
    }

    pub async fn get_settings(&self) -> AppSettings {
        self.config.read().await.settings.clone()
    }

    pub async fn save_settings(&self, settings: AppSettings) -> Result<(), String> {
        {
            let mut config = self.config.write().await;
            config.settings = settings.normalized();
        }
        self.save().await
    }

    pub async fn get_provider_entry(&self, provider_id: &str) -> Option<ProviderEntry> {
        let config = self.config.read().await;
        config.providers.get(provider_id).cloned()
    }

    pub async fn get_provider_entries(&self) -> HashMap<String, ProviderEntry> {
        self.config.read().await.providers.clone()
    }

    pub async fn save_provider_entry(
        &self,
        provider_id: &str,
        entry: ProviderEntry,
    ) -> Result<(), String> {
        {
            let mut config = self.config.write().await;
            config.providers.insert(provider_id.to_string(), entry);
        }
        self.save().await
    }

    pub async fn get_configured_providers(&self) -> Vec<String> {
        let config = self.config.read().await;
        let mut configured: Vec<String> = config
            .providers
            .iter()
            .filter(|(_, entry)| entry.enabled)
            .map(|(provider_id, _)| provider_id.clone())
            .collect();

        configured
            .sort_by(|left, right| compare_provider_order(&config.provider_order, left, right));
        configured
    }

    pub async fn save_provider_order(&self, order: Vec<String>) -> Result<(), String> {
        {
            let mut config = self.config.write().await;
            config.provider_order = order;
        }
        self.save().await
    }

    pub async fn get_enabled_providers(&self) -> Vec<String> {
        self.get_configured_providers().await
    }

    /// 获取 provider_order（用于设置页排序）
    pub async fn get_provider_order(&self) -> Vec<String> {
        self.config.read().await.provider_order.clone()
    }
}

fn compare_provider_order(order: &[String], left: &str, right: &str) -> std::cmp::Ordering {
    let left_index = order.iter().position(|id| id == left).unwrap_or(usize::MAX);
    let right_index = order
        .iter()
        .position(|id| id == right)
        .unwrap_or(usize::MAX);

    left_index
        .cmp(&right_index)
        .then_with(|| provider_rank(left).cmp(&provider_rank(right)))
        .then_with(|| left.cmp(right))
}

fn provider_rank(provider_id: &str) -> usize {
    // 内置供应商按 registry 顺序排序，自定义供应商排最后
    let templates = crate::providers::registry::all();
    for (i, template) in templates.iter().enumerate() {
        if template.id == provider_id {
            return i;
        }
    }
    usize::MAX
}
