use tauri::State;

use crate::config::app_config::{
    AppConfig, ProviderApiKeyEntry, ProviderEntry, ProviderSubscriptionEntry,
};
use crate::config::encryption::KeyStore;
use crate::config::system_env::sync_active_api_key_envs;
use crate::providers::types::*;
use crate::providers::ProviderManager;
use crate::stats::UsageStatsStore;

const LEGACY_API_KEY_ID: &str = "legacy-default";
const LEGACY_SUBSCRIPTION_ID: &str = "legacy-subscription";

struct ResolvedApiKey {
    id: String,
    name: String,
    color: String,
    value: String,
}

struct ResolvedSubscription {
    id: String,
    name: String,
    color: String,
    value: String,
    source: Option<String>,
}

/// 获取所有已启用供应商的用量
#[tauri::command]
pub async fn fetch_all_usage(
    provider_manager: State<'_, ProviderManager>,
    app_config: State<'_, AppConfig>,
    key_store: State<'_, KeyStore>,
    usage_stats_store: State<'_, UsageStatsStore>,
) -> Result<Vec<UsageSummary>, String> {
    let enabled = app_config.get_enabled_providers().await;
    let mut results = Vec::new();

    for provider_id in enabled {
        let entry = app_config.get_provider_entry(&provider_id).await;
        let summary = build_usage_summary(
            &provider_id,
            entry,
            provider_manager.inner(),
            key_store.inner(),
        )
        .await?;
        results.push(summary);
    }

    if let Err(error) = usage_stats_store.record_summaries(&results).await {
        eprintln!("写入统计历史失败: {}", error);
    }

    Ok(results)
}

/// 获取单个供应商用量
#[tauri::command]
pub async fn fetch_provider_usage(
    provider_id: String,
    provider_manager: State<'_, ProviderManager>,
    app_config: State<'_, AppConfig>,
    key_store: State<'_, KeyStore>,
    usage_stats_store: State<'_, UsageStatsStore>,
) -> Result<UsageSummary, String> {
    let entry = app_config.get_provider_entry(&provider_id).await;
    let summary = build_usage_summary(
        &provider_id,
        entry,
        provider_manager.inner(),
        key_store.inner(),
    )
    .await?;

    if let Err(error) = usage_stats_store
        .record_summaries(std::slice::from_ref(&summary))
        .await
    {
        eprintln!("写入统计历史失败: {}", error);
    }

    Ok(summary)
}

/// 获取已添加的供应商配置
#[tauri::command]
pub async fn get_provider_configs(
    provider_manager: State<'_, ProviderManager>,
    app_config: State<'_, AppConfig>,
    key_store: State<'_, KeyStore>,
) -> Result<Vec<ProviderConfigItem>, String> {
    let configured_providers = app_config.get_configured_providers().await;
    let mut items = Vec::new();

    for provider_id in configured_providers {
        let Some(mut item) = provider_manager.get_provider_config_item(&provider_id) else {
            continue;
        };

        let entry = app_config.get_provider_entry(&provider_id).await;

        // 内置供应商从 registry 查模板；自定义供应商从 custom_config 取
        let (env_key_name, env_oauth_token_name, provider_template_id, custom_config) = match &entry
        {
            Some(entry) if entry.custom_config.is_some() => {
                // 自定义供应商
                let cfg = entry.custom_config.as_ref().unwrap();
                (
                    cfg.env_key_name.clone().unwrap_or_default(),
                    None,
                    None,
                    entry.custom_config.clone(),
                )
            }
            _ => {
                // 内置供应商：从 registry 取
                let template = crate::providers::registry::get(&provider_id);
                (
                    template
                        .as_ref()
                        .map(|t| t.env_key_name.clone())
                        .unwrap_or_default(),
                    template
                        .as_ref()
                        .and_then(|t| t.env_oauth_token_name.clone()),
                    template.as_ref().map(|t| t.id.clone()),
                    None,
                )
            }
        };

        let api_keys = load_provider_api_keys(
            &provider_id,
            entry.as_ref(),
            key_store.inner(),
            &env_key_name,
        )
        .await;

        let active_api_key_id = entry
            .as_ref()
            .and_then(|provider_entry| provider_entry.active_api_key_id.clone());

        item.enabled = true;
        item.api_keys = api_keys
            .into_iter()
            .map(|key| ProviderApiKeyItem {
                is_active_in_environment: active_api_key_id.as_deref() == Some(key.id.as_str()),
                id: key.id,
                name: key.name,
                color: key.color,
                value: mask_value(&key.value),
            })
            .collect();
        item.environment_variable_name = env_key_name;
        item.active_api_key_id = active_api_key_id;
        item.provider_template_id = provider_template_id;
        item.custom_config = custom_config;

        if let Some(env_name) = env_oauth_token_name {
            item.subscriptions = load_provider_subscriptions(
                &provider_id,
                entry.as_ref(),
                key_store.inner(),
                Some(&env_name),
            )
            .await
            .into_iter()
            .map(|subscription| ProviderSubscriptionItem {
                id: subscription.id,
                name: subscription.name,
                color: subscription.color,
                oauth_token: mask_value(&subscription.value),
                source: subscription.source,
            })
            .collect();
        }

        items.push(item);
    }

    Ok(items)
}

/// 获取支持的供应商列表
#[tauri::command]
pub async fn get_supported_providers(
    provider_manager: State<'_, ProviderManager>,
) -> Result<Vec<ProviderConfigItem>, String> {
    Ok(provider_manager.get_provider_config_items())
}

/// 保存供应商配置
#[tauri::command]
pub async fn save_provider_config(
    config: ProviderConfig,
    app_config: State<'_, AppConfig>,
    key_store: State<'_, KeyStore>,
) -> Result<(), String> {
    let ProviderConfig {
        provider_id: provider_enum,
        enabled,
        api_keys,
        subscriptions,
        provider_template_id,
        custom_config,
    } = config;

    let provider_id = provider_enum;
    let existing_entry = app_config.get_provider_entry(&provider_id).await;

    // 解析环境变量名（自定义供应商用 custom_config.env_key_name，内置从 registry 查）
    let env_key_name = match &custom_config {
        Some(cfg) => cfg.env_key_name.clone().unwrap_or_default(),
        None => crate::providers::registry::get(&provider_id)
            .map(|t| t.env_key_name)
            .unwrap_or_default(),
    };

    let sanitized_api_keys: Vec<ProviderApiKeyInput> = api_keys
        .into_iter()
        .enumerate()
        .filter_map(|(index, key)| {
            let value = key.value.trim().to_string();
            if value.is_empty() {
                return None;
            }

            let id = if key.id.trim().is_empty() {
                format!("key-{}", index + 1)
            } else {
                key.id.trim().to_string()
            };

            Some(ProviderApiKeyInput {
                id,
                name: normalize_key_name(&key.name, index),
                color: normalize_marker_color(&key.color, index),
                value,
            })
        })
        .collect();

    let sanitized_subscriptions: Vec<ProviderSubscriptionInput> = subscriptions
        .into_iter()
        .enumerate()
        .filter_map(|(index, subscription)| {
            let oauth_token = subscription.oauth_token.trim().to_string();
            if oauth_token.is_empty() {
                return None;
            }

            let id = if subscription.id.trim().is_empty() {
                format!("subscription-{}", index + 1)
            } else {
                subscription.id.trim().to_string()
            };

            Some(ProviderSubscriptionInput {
                id,
                name: normalize_subscription_name(&subscription.name, index),
                color: normalize_marker_color(&subscription.color, index),
                oauth_token,
                source: subscription.source.and_then(normalize_optional_string),
            })
        })
        .collect();

    let provider_entry = ProviderEntry {
        provider_id: provider_id.clone(),
        enabled,
        api_keys: sanitized_api_keys
            .iter()
            .map(|key| ProviderApiKeyEntry {
                id: key.id.clone(),
                name: key.name.clone(),
                color: key.color.clone(),
            })
            .collect(),
        subscriptions: sanitized_subscriptions
            .iter()
            .map(|subscription| ProviderSubscriptionEntry {
                id: subscription.id.clone(),
                name: subscription.name.clone(),
                color: subscription.color.clone(),
                source: subscription.source.clone(),
            })
            .collect(),
        active_api_key_id: existing_entry
            .as_ref()
            .and_then(|entry| entry.active_api_key_id.clone())
            .filter(|active_key_id| {
                sanitized_api_keys
                    .iter()
                    .any(|key| key.id == *active_key_id)
            }),
        manage_api_key_environment: existing_entry
            .as_ref()
            .map(|entry| entry.manage_api_key_environment)
            .unwrap_or(false),
        provider_template_id,
        custom_config,
    };

    app_config
        .save_provider_entry(&provider_id, provider_entry)
        .await?;

    for key in &sanitized_api_keys {
        let storage_key = api_key_storage_key(&provider_id, &key.id);

        if key.value.contains("...") {
            if key_store.get_stored_key(&storage_key).await.is_none() {
                if let Some(legacy_value) = key_store.get_key(&provider_id, &env_key_name).await {
                    key_store.set_key(&storage_key, &legacy_value).await?;
                }
            }
            continue;
        }

        key_store.set_key(&storage_key, &key.value).await?;
    }

    if let Some(entry) = existing_entry.as_ref() {
        for old_key in &entry.api_keys {
            if sanitized_api_keys.iter().any(|key| key.id == old_key.id) {
                continue;
            }

            key_store
                .set_key(&api_key_storage_key(&provider_id, &old_key.id), "")
                .await?;
        }
    }

    key_store.set_key(&provider_id, "").await?;

    if let Some(entry) = existing_entry.as_ref() {
        for old_subscription in &entry.subscriptions {
            if sanitized_subscriptions
                .iter()
                .any(|subscription| subscription.id == old_subscription.id)
            {
                continue;
            }

            key_store
                .set_key(
                    &subscription_storage_key(&provider_id, &old_subscription.id),
                    "",
                )
                .await?;
        }
    }

    for subscription in &sanitized_subscriptions {
        let storage_key = subscription_storage_key(&provider_id, &subscription.id);

        if subscription.oauth_token.contains("...") {
            continue;
        }

        key_store
            .set_key(&storage_key, &subscription.oauth_token)
            .await?;
    }

    key_store
        .set_key(&oauth_storage_key(&provider_id), "")
        .await?;

    sync_active_api_key_envs(app_config.inner(), key_store.inner()).await?;

    Ok(())
}

/// 移除供应商配置
#[tauri::command]
pub async fn remove_provider_config(
    provider_id: String,
    app_config: State<'_, AppConfig>,
    key_store: State<'_, KeyStore>,
    usage_stats_store: State<'_, UsageStatsStore>,
) -> Result<(), String> {
    let existing_entry = app_config.get_provider_entry(&provider_id).await;

    if let Some(entry) = existing_entry.as_ref() {
        for key in &entry.api_keys {
            key_store
                .set_key(&api_key_storage_key(&provider_id, &key.id), "")
                .await?;
        }

        for subscription in &entry.subscriptions {
            key_store
                .set_key(
                    &subscription_storage_key(&provider_id, &subscription.id),
                    "",
                )
                .await?;
        }
    }

    key_store.set_key(&provider_id, "").await?;
    key_store
        .set_key(&oauth_storage_key(&provider_id), "")
        .await?;

    app_config
        .save_provider_entry(
            &provider_id,
            ProviderEntry {
                provider_id: provider_id.clone(),
                enabled: false,
                api_keys: Vec::new(),
                subscriptions: Vec::new(),
                active_api_key_id: None,
                manage_api_key_environment: existing_entry
                    .as_ref()
                    .map(|entry| entry.manage_api_key_environment)
                    .unwrap_or(false),
                provider_template_id: None,
                custom_config: None,
            },
        )
        .await?;

    if let Err(error) = usage_stats_store.remove_provider(&provider_id).await {
        eprintln!("清理统计历史失败: {}", error);
    }

    sync_active_api_key_envs(app_config.inner(), key_store.inner()).await
}

/// 验证 API Key
#[tauri::command]
pub async fn validate_api_key(
    provider_id: String,
    api_key: String,
    custom_config: Option<CustomProviderConfig>,
    provider_manager: State<'_, ProviderManager>,
) -> Result<bool, String> {
    provider_manager
        .validate_key(&provider_id, &api_key, custom_config.as_ref())
        .await
}

#[tauri::command]
pub async fn save_provider_order(
    order: Vec<String>,
    app_config: State<'_, AppConfig>,
) -> Result<(), String> {
    let mut deduped = Vec::new();

    for provider_id in order {
        // 内置供应商在 registry 里 OR 自定义供应商（custom_ 前缀）
        let is_valid = crate::providers::registry::get(&provider_id).is_some()
            || provider_id.starts_with("custom_");
        if !is_valid {
            continue;
        }

        if !deduped.contains(&provider_id) {
            deduped.push(provider_id);
        }
    }

    app_config.save_provider_order(deduped).await
}

#[tauri::command]
pub async fn activate_provider_api_key(
    provider_id: String,
    api_key_id: String,
    app_config: State<'_, AppConfig>,
    key_store: State<'_, KeyStore>,
) -> Result<(), String> {
    let Some(mut entry) = app_config.get_provider_entry(&provider_id).await else {
        return Err("未找到供应商配置".to_string());
    };

    if !entry.api_keys.iter().any(|key| key.id == api_key_id) {
        return Err("未找到要激活的 API Key".to_string());
    }

    let storage_key = api_key_storage_key(&provider_id, &api_key_id);
    let has_stored_value = key_store
        .get_stored_key(&storage_key)
        .await
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false);

    if !has_stored_value {
        return Err("当前 API Key 尚未保存，请先保存配置".to_string());
    }

    entry.active_api_key_id = Some(api_key_id);
    entry.manage_api_key_environment = true;
    app_config.save_provider_entry(&provider_id, entry).await?;
    sync_active_api_key_envs(app_config.inner(), key_store.inner()).await
}

async fn build_usage_summary(
    provider_id: &str,
    entry: Option<ProviderEntry>,
    provider_manager: &ProviderManager,
    key_store: &KeyStore,
) -> Result<UsageSummary, String> {
    let base_item = provider_manager
        .get_provider_config_item(provider_id)
        .ok_or_else(|| format!("未知供应商: {}", provider_id))?;

    // 解析环境变量名（自定义供应商用 custom_config.env_key_name，内置从 registry 查）
    let env_key_name = match entry.as_ref().and_then(|e| e.custom_config.as_ref()) {
        Some(cfg) => cfg.env_key_name.clone().unwrap_or_default(),
        None => crate::providers::registry::get(provider_id)
            .map(|t| t.env_key_name)
            .unwrap_or_default(),
    };
    let env_oauth_token_name = match entry.as_ref().and_then(|e| e.custom_config.as_ref()) {
        // 自定义供应商阶段 1 不支持订阅
        Some(_) => None,
        None => crate::providers::registry::get(provider_id).and_then(|t| t.env_oauth_token_name),
    };

    let api_keys =
        load_provider_api_keys(provider_id, entry.as_ref(), key_store, &env_key_name).await;
    let subscriptions = if let Some(env_name) = env_oauth_token_name.as_deref() {
        load_provider_subscriptions(provider_id, entry.as_ref(), key_store, Some(env_name)).await
    } else {
        Vec::new()
    };

    // 自定义供应商的 custom_config 引用（用于 fetch_api_usage）
    let custom_config_ref = entry.as_ref().and_then(|e| e.custom_config.as_ref());

    let mut api_key_usages = Vec::new();
    let mut successful_usages = Vec::new();
    let mut api_errors = Vec::new();
    let mut rate_limit = None;

    for api_key in &api_keys {
        match provider_manager
            .fetch_api_usage(provider_id, &api_key.value, custom_config_ref)
            .await
        {
            Ok((usage, item_rate_limit)) => {
                if api_keys.len() == 1 {
                    rate_limit = item_rate_limit.clone();
                }

                successful_usages.push(usage.clone());
                api_key_usages.push(ApiKeyUsageSummary {
                    key_id: api_key.id.clone(),
                    key_name: api_key.name.clone(),
                    color: api_key.color.clone(),
                    status: ProviderStatus::Success,
                    usage: Some(usage),
                    rate_limit: item_rate_limit,
                    error_message: None,
                });
            }
            Err(error) => {
                api_errors.push(format!("{}: {}", api_key.name, error));
                api_key_usages.push(ApiKeyUsageSummary {
                    key_id: api_key.id.clone(),
                    key_name: api_key.name.clone(),
                    color: api_key.color.clone(),
                    status: ProviderStatus::Error,
                    usage: None,
                    rate_limit: None,
                    error_message: Some(error),
                });
            }
        }
    }

    let usage = aggregate_usage_data(&successful_usages);
    let mut subscription_summaries = Vec::new();
    let mut subscription_errors = Vec::new();

    for subscription in subscriptions {
        let sub_usage = provider_manager
            .fetch_subscription_usage(provider_id, &subscription.value, custom_config_ref)
            .await;
        if matches!(sub_usage.status, ProviderStatus::Error) {
            if let Some(error_message) = sub_usage.error_message.clone() {
                subscription_errors.push(format!("{}: {}", subscription.name, error_message));
            }
        }
        subscription_summaries.push(SubscriptionUsageSummary {
            subscription_id: subscription.id,
            subscription_name: subscription.name,
            color: subscription.color,
            source: subscription.source,
            usage: sub_usage,
        });
    }

    let has_subscription_data = subscription_summaries
        .iter()
        .any(|item| matches!(item.usage.status, ProviderStatus::Success));

    let has_usage_data = usage.is_some();
    let status = if has_usage_data || has_subscription_data {
        ProviderStatus::Success
    } else {
        ProviderStatus::Error
    };

    let error_message = if has_usage_data || has_subscription_data {
        None
    } else if !api_errors.is_empty() {
        Some(api_errors.join("；"))
    } else if !subscription_errors.is_empty() {
        Some(subscription_errors.join("；"))
    } else {
        Some("未配置 API Key 或 OAuth Token".into())
    };

    let summary = UsageSummary {
        provider_id: provider_id.to_string(),
        display_name: base_item.display_name,
        enabled: entry.as_ref().map(|item| item.enabled).unwrap_or(true),
        status,
        api_key_usages,
        usage,
        subscriptions: subscription_summaries,
        rate_limit,
        last_updated: Some(chrono::Utc::now().to_rfc3339()),
        error_message,
    };

    provider_manager
        .cache_summary(provider_id, summary.clone())
        .await;

    Ok(summary)
}

async fn load_provider_api_keys(
    provider_id: &str,
    entry: Option<&ProviderEntry>,
    key_store: &KeyStore,
    env_key_name: &str,
) -> Vec<ResolvedApiKey> {
    let mut api_keys = Vec::new();

    if let Some(entry) = entry {
        for (index, key) in entry.api_keys.iter().enumerate() {
            if let Some(value) = key_store
                .get_stored_key(&api_key_storage_key(provider_id, &key.id))
                .await
                .filter(|value| !value.is_empty())
            {
                api_keys.push(ResolvedApiKey {
                    id: key.id.clone(),
                    name: normalize_key_name(&key.name, index),
                    color: normalize_marker_color(&key.color, index),
                    value,
                });
            }
        }
    }

    if !api_keys.is_empty() {
        return api_keys;
    }

    if let Some(legacy_value) = key_store.get_key(provider_id, env_key_name).await {
        if !legacy_value.is_empty() {
            api_keys.push(ResolvedApiKey {
                id: LEGACY_API_KEY_ID.to_string(),
                name: "默认 Key".to_string(),
                color: normalize_marker_color("", 0),
                value: legacy_value,
            });
        }
    }

    api_keys
}

async fn load_provider_subscriptions(
    provider_id: &str,
    entry: Option<&ProviderEntry>,
    key_store: &KeyStore,
    env_var_name: Option<&str>,
) -> Vec<ResolvedSubscription> {
    let mut subscriptions = Vec::new();

    if let Some(entry) = entry {
        for (index, subscription) in entry.subscriptions.iter().enumerate() {
            if let Some(value) = key_store
                .get_stored_key(&subscription_storage_key(provider_id, &subscription.id))
                .await
                .filter(|value| !value.is_empty())
            {
                subscriptions.push(ResolvedSubscription {
                    id: subscription.id.clone(),
                    name: normalize_subscription_name(&subscription.name, index),
                    color: normalize_marker_color(&subscription.color, index),
                    value,
                    source: subscription.source.clone(),
                });
            }
        }
    }

    if !subscriptions.is_empty() {
        return subscriptions;
    }

    if let Some(env_var_name) = env_var_name {
        if let Some(legacy_value) = key_store
            .get_key(&oauth_storage_key(provider_id), env_var_name)
            .await
            .filter(|value| !value.is_empty())
        {
            subscriptions.push(ResolvedSubscription {
                id: LEGACY_SUBSCRIPTION_ID.to_string(),
                name: default_subscription_name(provider_id),
                color: normalize_marker_color("", 0),
                value: legacy_value,
                source: None,
            });
        }
    }

    subscriptions
}

fn aggregate_usage_data(items: &[UsageData]) -> Option<UsageData> {
    let first = items.first()?;
    let currency = first.currency.clone();
    let mut total_used = 0.0;
    let mut total_budget = Some(0.0);
    let mut remaining = Some(0.0);
    let mut period_start = first.period_start.clone();
    let mut period_end = first.period_end.clone();

    for item in items {
        total_used += item.total_used;

        total_budget = match (total_budget, item.total_budget) {
            (Some(acc), Some(value)) => Some(acc + value),
            _ => None,
        };

        remaining = match (remaining, item.remaining) {
            (Some(acc), Some(value)) => Some(acc + value),
            _ => None,
        };

        period_start = min_optional_iso(period_start, item.period_start.clone());
        period_end = max_optional_iso(period_end, item.period_end.clone());
    }

    Some(UsageData {
        total_used,
        total_budget,
        remaining,
        currency,
        period_start,
        period_end,
    })
}

fn min_optional_iso(current: Option<String>, next: Option<String>) -> Option<String> {
    match (current, next) {
        (Some(left), Some(right)) => Some(if left <= right { left } else { right }),
        (Some(left), None) => Some(left),
        (None, Some(right)) => Some(right),
        (None, None) => None,
    }
}

fn max_optional_iso(current: Option<String>, next: Option<String>) -> Option<String> {
    match (current, next) {
        (Some(left), Some(right)) => Some(if left >= right { left } else { right }),
        (Some(left), None) => Some(left),
        (None, Some(right)) => Some(right),
        (None, None) => None,
    }
}

fn api_key_storage_key(provider_id: &str, key_id: &str) -> String {
    format!("{}::api_key::{}", provider_id, key_id)
}

fn oauth_storage_key(provider_id: &str) -> String {
    format!("{}_oauth", provider_id)
}

fn subscription_storage_key(provider_id: &str, subscription_id: &str) -> String {
    format!("{}::subscription::{}", provider_id, subscription_id)
}

fn normalize_key_name(name: &str, index: usize) -> String {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        format!("密钥 {}", index + 1)
    } else {
        trimmed.to_string()
    }
}

fn normalize_subscription_name(name: &str, index: usize) -> String {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        format!("订阅 {}", index + 1)
    } else {
        trimmed.to_string()
    }
}

fn normalize_marker_color(color: &str, index: usize) -> String {
    const MARKER_COLORS: [&str; 8] = [
        "#3b82f6", "#10b981", "#f59e0b", "#ef4444", "#8b5cf6", "#06b6d4", "#ec4899", "#84cc16",
    ];

    let trimmed = color.trim();
    if MARKER_COLORS.contains(&trimmed) {
        trimmed.to_string()
    } else {
        MARKER_COLORS[index % MARKER_COLORS.len()].to_string()
    }
}

fn normalize_optional_string(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// 旧版订阅默认名称：按 provider_id 字符串映射显示文案
fn default_subscription_name(provider_id: &str) -> String {
    // 优先从 registry 拿 display_name，找不到就回退用 provider_id
    let display = crate::providers::registry::get(provider_id)
        .map(|t| t.display_name)
        .unwrap_or_else(|| provider_id.to_string());
    format!("{} 订阅", display)
}

fn mask_value(value: &str) -> String {
    if value.is_empty() {
        return String::new();
    }

    if value.len() <= 8 {
        return "****".to_string();
    }

    format!("{}...{}", &value[..4], &value[value.len() - 4..])
}

/// 获取所有可选供应商模板（含内置，用于设置页"新增供应商"下拉）
#[tauri::command]
pub async fn get_provider_templates(
    provider_manager: State<'_, std::sync::Arc<ProviderManager>>,
) -> Result<Vec<ProviderTemplate>, String> {
    Ok(provider_manager.get_provider_templates())
}

/// 获取 NewAPI 预置脚本模板
#[tauri::command]
pub async fn get_newapi_script_template() -> Result<String, String> {
    Ok(crate::providers::registry::newapi_script_template().to_string())
}

/// 测试自定义供应商脚本（保存前预演）
#[tauri::command]
pub async fn test_custom_provider_script(
    provider_manager: State<'_, std::sync::Arc<ProviderManager>>,
    code: String,
    api_key: String,
    base_url: Option<String>,
    allow_http: bool,
) -> Result<String, String> {
    // 执行脚本，返回成功/失败信息
    let result = crate::providers::script_engine::run(
        provider_manager.http_client_ref(),
        &code,
        &api_key,
        base_url.as_deref(),
        allow_http,
        15000,
    )
    .await
    .map_err(|e| e.to_string())?;

    Ok(format!(
        "查询成功：已用 {} / 总额 {:?} / 剩余 {:?} ({})",
        result.total_used, result.total_budget, result.remaining, result.currency
    ))
}
