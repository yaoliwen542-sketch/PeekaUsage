use tauri::State;

use futures::StreamExt;

use crate::config::app_config::{
    AppConfig, ProviderApiKeyEntry, ProviderEntry, ProviderSubscriptionEntry,
};
use crate::config::encryption::{is_masked_placeholder, mask_secret, KeyStore};
use crate::config::system_env::sync_active_api_key_envs;
use crate::providers::types::*;
use crate::providers::ProviderManager;
use crate::stats::UsageStatsStore;

const LEGACY_API_KEY_ID: &str = "legacy-default";
const LEGACY_SUBSCRIPTION_ID: &str = "legacy-subscription";

/// 修复 M14：用量拉取的最大并发数。
/// 串行拉取时 N 个供应商 × M 个 key 顺序 await，单次刷新最坏 N×M×30s；
/// 并发到 4 路足以明显缩短总耗时，又不会瞬间打满连接/触发对端限流。
/// fetch_all_usage 外层（供应商间）与 build_usage_summary 内层（key/订阅间）
/// 都用该值限流，理论峰值 4×4=16 路并发，实际远低于此（单供应商 key 数通常 ≤3）。
const USAGE_FETCH_CONCURRENCY: usize = 4;

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

    // 修复 M14：各供应商的拉取并发执行（buffered 限流且保序）。
    // 返回顺序仍与 get_enabled_providers() 一致，即受 provider_order 影响，
    // 前端卡片排序不会因为并发完成先后而打乱。
    let results: Vec<UsageSummary> =
        futures::stream::iter(enabled.into_iter().map(|provider_id| {
            let provider_manager = provider_manager.inner();
            let key_store = key_store.inner();
            let app_config = app_config.inner();
            async move {
                let entry = app_config.get_provider_entry(&provider_id).await;
                // 修复 H-1：单个供应商失败不能拖垮整个命令，
                // 把错误收敛到该供应商自己的 error_message，其余供应商照常返回
                match build_usage_summary(&provider_id, entry.clone(), provider_manager, key_store)
                    .await
                {
                    Ok(summary) => summary,
                    Err(error) => {
                        eprintln!("获取供应商 {} 用量失败: {}", provider_id, error);
                        build_error_summary(&provider_id, entry.as_ref(), error)
                    }
                }
            }
        }))
        .buffered(USAGE_FETCH_CONCURRENCY)
        .collect()
        .await;

    // 统计副作用保持在全部结果收集完成后只执行一次
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
    // 返回所有已存在的 provider 条目（不论 enabled），
    // 设置页需要显示所有内置供应商卡片让用户填 Key，不能只返回 enabled 的。
    let provider_entries = app_config.get_provider_entries().await;
    let provider_order = app_config.get_provider_order().await;
    let mut configured_providers: Vec<String> = provider_entries.keys().cloned().collect();
    configured_providers.sort_by(|left, right| {
        let li = provider_order
            .iter()
            .position(|id| id == left)
            .unwrap_or(usize::MAX);
        let ri = provider_order
            .iter()
            .position(|id| id == right)
            .unwrap_or(usize::MAX);
        li.cmp(&ri).then_with(|| left.cmp(right))
    });
    let mut items = Vec::new();

    for provider_id in configured_providers {
        let entry = app_config.get_provider_entry(&provider_id).await;

        // 内置供应商走 registry，自定义供应商从 custom_config 派生配置项，
        // 否则设置页重进后看不到自定义供应商卡片，无法编辑/删除
        let Some(mut item) = provider_manager.resolve_config_item(
            &provider_id,
            entry.as_ref().and_then(|e| e.custom_config.as_ref()),
        ) else {
            eprintln!("跳过无法解析的供应商配置: {}", provider_id);
            continue;
        };

        // 内置供应商从 registry 查模板；自定义供应商从 custom_config 取
        let (env_key_name, env_oauth_token_name, provider_template_id, custom_config) = match &entry
        {
            Some(entry) if entry.custom_config.is_some() => {
                // 自定义供应商
                let cfg = entry.custom_config.as_ref().unwrap();
                // NewAPI 凭据（阶段 2）：读到旧明文自动迁 KeyStore，返回前端的值一律掩码
                let display_cfg = prepare_custom_config_for_display(
                    &provider_id,
                    cfg,
                    app_config.inner(),
                    key_store.inner(),
                )
                .await;
                (
                    cfg.env_key_name.clone().unwrap_or_default(),
                    None,
                    None,
                    Some(display_cfg),
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

        item.enabled = entry.as_ref().map(|e| e.enabled).unwrap_or(false);
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
        mut custom_config,
    } = config;

    let provider_id = provider_enum;
    let existing_entry = app_config.get_provider_entry(&provider_id).await;

    // 修复 C-5：脚本 timeout_ms 在入口 clamp 到 ≤ 60000，避免用户配置过大导致 JS 引擎长时间阻塞
    if let Some(cfg) = custom_config.as_mut() {
        if let Some(script) = cfg.script.as_mut() {
            if script.timeout_ms > 60000 {
                script.timeout_ms = 60000;
            }
            if script.timeout_ms == 0 {
                script.timeout_ms = 15000;
            }
        }
    }

    // NewAPI 凭据迁 KeyStore（阶段 2）：accessToken/userId 不再随 custom_config 明文落盘。
    // 明文新值写 KeyStore、空串清除、None 或未修改的掩码占位符保持原值；
    // 保存进 config.json 的 custom_config 一律清掉这两个字段。
    if let Some(cfg) = custom_config.as_mut() {
        persist_custom_credential(
            cfg.access_token.as_deref(),
            &custom_access_token_storage_key(&provider_id),
            key_store.inner(),
        )
        .await?;
        persist_custom_credential(
            cfg.user_id.as_deref(),
            &custom_user_id_storage_key(&provider_id),
            key_store.inner(),
        )
        .await?;
        cfg.access_token = None;
        cfg.user_id = None;
    }

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
        let stored = key_store.get_stored_key(&storage_key).await;

        // 修复 L6：掩码判定改为「值等于上次保存值的掩码串」，
        // 真实包含 "..." 的 key 不再被误判为占位符而丢失
        if is_masked_placeholder(&key.value, stored.as_deref()) {
            continue;
        }

        // 旧版兼容：值形似掩码且当前无已存储值时，尝试从旧版单 key 存储迁移
        if key.value.contains("...") && stored.is_none() {
            if let Some(legacy_value) = key_store.get_key(&provider_id, &env_key_name).await {
                key_store.set_key(&storage_key, &legacy_value).await?;
                continue;
            }
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
        let stored = key_store.get_stored_key(&storage_key).await;

        // 修复 L6：同 API Key，掩码判定改为与已存储值的掩码串精确比较
        if is_masked_placeholder(&subscription.oauth_token, stored.as_deref()) {
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

    // NewAPI 凭据（阶段 2 起存 KeyStore）一并清理；无对应条目时为空操作
    key_store
        .set_key(&custom_access_token_storage_key(&provider_id), "")
        .await?;
    key_store
        .set_key(&custom_user_id_storage_key(&provider_id), "")
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
    key_store: State<'_, KeyStore>,
) -> Result<bool, String> {
    provider_manager
        .validate_key(
            &provider_id,
            &api_key,
            custom_config.as_ref(),
            Some(key_store.inner()),
        )
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
        .resolve_config_item(
            provider_id,
            entry.as_ref().and_then(|e| e.custom_config.as_ref()),
        )
        .ok_or_else(|| {
            format!(
                "未知供应商: {}（既不是内置供应商，也缺少自定义供应商配置，请检查该供应商的配置条目）",
                provider_id
            )
        })?;

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

    // 修复 M14：同供应商多个 key 的查询并发执行（限流 + 保序），
    // 结果仍按配置顺序归并，多 key 场景不再串行等待。
    //
    // 注意：future 在 for 循环里直接构造（而非 iter().map(closure)）。
    // Tauri 命令宏会给 State 引用引入高阶生命周期，"闭包返回借用/捕获外部
    // 引用的 async block" 会触发 FnOnce lifetime 不可泛化的编译错误；
    // 循环构造 + stream::iter(Vec<Future>) 是成熟可行的等价写法。
    let single_key = api_keys.len() == 1;
    let mut api_key_futures = Vec::with_capacity(api_keys.len());
    for api_key in &api_keys {
        // 克隆 key 值，future 持有自有数据
        let key_value = api_key.value.clone();
        api_key_futures.push(async move {
            provider_manager
                .fetch_api_usage(provider_id, &key_value, custom_config_ref, Some(key_store))
                .await
        });
    }
    let api_key_results: Vec<Result<(UsageData, Option<RateLimitData>), String>> =
        futures::stream::iter(api_key_futures)
            .buffered(USAGE_FETCH_CONCURRENCY)
            .collect()
            .await;

    let mut api_key_usages = Vec::new();
    let mut successful_usages = Vec::new();
    let mut api_errors = Vec::new();
    let mut rate_limit = None;

    for (api_key, result) in api_keys.iter().zip(api_key_results) {
        match result {
            Ok((usage, item_rate_limit)) => {
                if single_key {
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

    // 修复 M14：多订阅查询同样并发执行（限流 + 保序）
    // key_store 透传给 Gemini 订阅链路用于缓存刷新后的 token（修复 M8）
    let mut subscription_futures = Vec::with_capacity(subscriptions.len());
    for subscription in &subscriptions {
        // 克隆 token 值，future 持有自有数据
        let token_value = subscription.value.clone();
        subscription_futures.push(async move {
            provider_manager
                .fetch_subscription_usage(
                    provider_id,
                    &token_value,
                    Some(key_store),
                    custom_config_ref,
                )
                .await
        });
    }
    let subscription_usages: Vec<SubscriptionUsage> = futures::stream::iter(subscription_futures)
        .buffered(USAGE_FETCH_CONCURRENCY)
        .collect()
        .await;

    for (subscription, sub_usage) in subscriptions.into_iter().zip(subscription_usages) {
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

    Ok(summary)
}

/// 单个供应商拉取失败时的兜底摘要
///
/// 保证 `fetch_all_usage` 里一个供应商出错只影响自己的卡片，
/// display_name 尽力从 registry / custom_config 解析，兜底用 provider_id。
fn build_error_summary(
    provider_id: &str,
    entry: Option<&ProviderEntry>,
    error: String,
) -> UsageSummary {
    let display_name = entry
        .and_then(|e| e.custom_config.as_ref())
        .map(|cfg| cfg.display_name.clone())
        .or_else(|| {
            crate::providers::registry::get(provider_id).map(|template| template.display_name)
        })
        .unwrap_or_else(|| provider_id.to_string());

    UsageSummary {
        provider_id: provider_id.to_string(),
        display_name,
        enabled: entry.map(|item| item.enabled).unwrap_or(true),
        status: ProviderStatus::Error,
        api_key_usages: Vec::new(),
        usage: None,
        subscriptions: Vec::new(),
        rate_limit: None,
        last_updated: Some(chrono::Utc::now().to_rfc3339()),
        error_message: Some(error),
    }
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

    let period_start = items.iter().fold(first.period_start.clone(), |acc, item| {
        min_optional_iso(acc, item.period_start.clone())
    });
    let period_end = items.iter().fold(first.period_end.clone(), |acc, item| {
        max_optional_iso(acc, item.period_end.clone())
    });
    let windows = merge_windows_by_label(items);

    // 百分比型（Coding Plan 配额）：跨 Key 求和没有语义——两个 Key 各用 70%
    // 不等于合计 140%。聚合取各 Key 最差值（最高利用率）作为整体状态，
    // 预算恒为 100，剩余 = 100 - 最高利用率。
    let plan_name = items.iter().find_map(|item| item.plan_name.clone());

    if currency == "%" {
        let total_used = items
            .iter()
            .map(|item| item.total_used)
            .fold(0.0_f64, f64::max)
            .clamp(0.0, 100.0);
        return Some(UsageData {
            total_used,
            total_budget: Some(100.0),
            remaining: Some((100.0 - total_used).clamp(0.0, 100.0)),
            currency,
            period_start,
            period_end,
            windows,
            plan_name,
        });
    }

    let mut total_used = 0.0;
    let mut total_budget = Some(0.0);
    let mut remaining = Some(0.0);

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
    }

    Some(UsageData {
        total_used,
        total_budget,
        remaining,
        currency,
        period_start,
        period_end,
        windows,
        plan_name,
    })
}

/// 分窗口利用率按标签合并：同名窗口取最高利用率，保持首次出现顺序
fn merge_windows_by_label(items: &[UsageData]) -> Vec<crate::providers::types::SubscriptionWindow> {
    let mut windows: Vec<crate::providers::types::SubscriptionWindow> = Vec::new();
    for item in items {
        for window in &item.windows {
            match windows.iter_mut().find(|w| w.label == window.label) {
                Some(existing) => {
                    if window.utilization > existing.utilization {
                        existing.utilization = window.utilization;
                    }
                    if existing.resets_at.is_none() {
                        existing.resets_at = window.resets_at.clone();
                    }
                }
                None => windows.push(window.clone()),
            }
        }
    }
    windows
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

/// NewAPI 凭据（accessToken）在 KeyStore 中的键名（阶段 2：不再明文落盘 config.json）
fn custom_access_token_storage_key(provider_id: &str) -> String {
    format!("{}::custom::access_token", provider_id)
}

/// NewAPI 凭据（userId）在 KeyStore 中的键名（阶段 2：不再明文落盘 config.json）
fn custom_user_id_storage_key(provider_id: &str) -> String {
    format!("{}::custom::user_id", provider_id)
}

/// 保存自定义供应商的 NewAPI 凭据到 KeyStore（阶段 2 修复：凭据不再明文落盘）
///
/// incoming 语义：
/// - None：前端未提供该字段 -> 保持 KeyStore 原值
/// - 掩码占位符（等于已存储值的掩码串）：前端原样回显 -> 保持 KeyStore 原值
/// - 其它（含空串）：写入 KeyStore；空串由 KeyStore::set_key 转为删除
async fn persist_custom_credential(
    incoming: Option<&str>,
    storage_key: &str,
    key_store: &KeyStore,
) -> Result<(), String> {
    match incoming {
        None => Ok(()),
        Some(value) => {
            let stored = key_store.get_stored_key(storage_key).await;
            if is_masked_placeholder(value, stored.as_deref()) {
                return Ok(());
            }
            key_store.set_key(storage_key, value.trim()).await
        }
    }
}

/// 准备返回前端的自定义供应商配置（NewAPI 凭据，阶段 2 修复）
///
/// 两件事：
/// 1. 旧配置自动迁移：config.json 里残留明文 accessToken/userId 时写入 KeyStore，
///    并持久化清掉 custom_config 里的明文（此后不再明文落盘）；
///    迁移失败只打日志，下次读取会重试，不阻断配置展示。
/// 2. 前端回显：从 KeyStore 取实际值掩码后放入返回的 custom_config，
///    与 API Key 的掩码语义一致（前端原样回传 = 未修改）。
async fn prepare_custom_config_for_display(
    provider_id: &str,
    cfg: &CustomProviderConfig,
    app_config: &AppConfig,
    key_store: &KeyStore,
) -> CustomProviderConfig {
    let mut display = cfg.clone();
    let mut migrated = false;

    if let Some(plain) = cfg.access_token.as_deref().filter(|s| !s.is_empty()) {
        match key_store
            .set_key(&custom_access_token_storage_key(provider_id), plain)
            .await
        {
            Ok(()) => migrated = true,
            Err(error) => eprintln!(
                "迁移 {} 的 accessToken 到 KeyStore 失败: {}",
                provider_id, error
            ),
        }
    }
    if let Some(plain) = cfg.user_id.as_deref().filter(|s| !s.is_empty()) {
        match key_store
            .set_key(&custom_user_id_storage_key(provider_id), plain)
            .await
        {
            Ok(()) => migrated = true,
            Err(error) => eprintln!("迁移 {} 的 userId 到 KeyStore 失败: {}", provider_id, error),
        }
    }

    if migrated {
        if let Some(mut entry) = app_config.get_provider_entry(provider_id).await {
            if let Some(stored_cfg) = entry.custom_config.as_mut() {
                stored_cfg.access_token = None;
                stored_cfg.user_id = None;
            }
            if let Err(error) = app_config.save_provider_entry(provider_id, entry).await {
                eprintln!("清除 {} 的明文 NewAPI 凭据失败: {}", provider_id, error);
            }
        }
    }

    let stored_token = key_store
        .get_stored_key(&custom_access_token_storage_key(provider_id))
        .await;
    let stored_user = key_store
        .get_stored_key(&custom_user_id_storage_key(provider_id))
        .await;
    display.access_token = stored_token
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(mask_secret);
    display.user_id = stored_user
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(mask_secret);

    display
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
    // 修复 L6：统一走 char 边界安全的共享实现（原实现按字节切片，多字节 UTF-8 落在边界会 panic）
    mask_secret(value)
}

/// 获取所有可选供应商模板（含内置，用于设置页"新增供应商"下拉）
#[tauri::command]
pub async fn get_provider_templates(
    provider_manager: State<'_, ProviderManager>,
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
    provider_manager: State<'_, ProviderManager>,
    code: String,
    api_key: String,
    base_url: Option<String>,
    allow_http: bool,
    access_token: Option<String>,
    user_id: Option<String>,
) -> Result<String, String> {
    // 执行脚本，返回成功/失败信息
    let result = crate::providers::script_engine::run(
        provider_manager.http_client_ref(),
        &code,
        &api_key,
        base_url.as_deref(),
        allow_http,
        15000,
        access_token.as_deref(),
        user_id.as_deref(),
    )
    .await
    .map_err(|e| e.to_string())?;

    Ok(format!(
        "查询成功：已用 {} / 总额 {} / 剩余 {} ({})",
        result.total_used,
        result
            .total_budget
            .map_or("未知".to_string(), |v| format!("{:.2}", v)),
        result
            .remaining
            .map_or("未知".to_string(), |v| format!("{:.2}", v)),
        result.currency
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn percent_usage(total_used: f64, windows: Vec<(&str, f64)>) -> UsageData {
        UsageData {
            total_used,
            total_budget: Some(100.0),
            remaining: Some(100.0 - total_used),
            currency: "%".to_string(),
            period_start: None,
            period_end: None,
            windows: windows
                .into_iter()
                .map(|(label, utilization)| SubscriptionWindow {
                    label: label.to_string(),
                    utilization,
                    resets_at: None,
                })
                .collect(),
            plan_name: None,
        }
    }

    fn money_usage(total_used: f64, budget: f64, remaining: f64) -> UsageData {
        UsageData {
            total_used,
            total_budget: Some(budget),
            remaining: Some(remaining),
            currency: "USD".to_string(),
            period_start: None,
            period_end: None,
            windows: Vec::new(),
            plan_name: None,
        }
    }

    #[test]
    fn test_aggregate_percent_multi_key_uses_max_not_sum() {
        // 两个 Kimi Key 各 20% / 40%：合计绝不能是 60% / 余额 140%
        let items = vec![
            percent_usage(20.0, vec![("five_hour", 0.0), ("weekly_limit", 20.0)]),
            percent_usage(40.0, vec![("five_hour", 0.0), ("weekly_limit", 40.0)]),
        ];
        let agg = aggregate_usage_data(&items).unwrap();
        assert!((agg.total_used - 40.0).abs() < 1e-6);
        assert_eq!(agg.total_budget, Some(100.0));
        assert!((agg.remaining.unwrap() - 60.0).abs() < 1e-6);
        assert_eq!(agg.currency, "%");
        // 同名窗口取最高利用率，保持首次出现顺序
        assert_eq!(agg.windows.len(), 2);
        assert_eq!(agg.windows[0].label, "five_hour");
        assert!((agg.windows[0].utilization - 0.0).abs() < 1e-6);
        assert_eq!(agg.windows[1].label, "weekly_limit");
        assert!((agg.windows[1].utilization - 40.0).abs() < 1e-6);
    }

    #[test]
    fn test_aggregate_percent_single_key_passthrough() {
        let items = vec![percent_usage(
            80.0,
            vec![("five_hour", 80.0), ("weekly_limit", 16.0)],
        )];
        let agg = aggregate_usage_data(&items).unwrap();
        assert!((agg.total_used - 80.0).abs() < 1e-6);
        assert!((agg.remaining.unwrap() - 20.0).abs() < 1e-6);
        assert_eq!(agg.windows.len(), 2);
    }

    #[test]
    fn test_aggregate_money_multi_key_sums() {
        // 金额型保持求和语义不变
        let items = vec![money_usage(3.5, 10.0, 6.5), money_usage(1.5, 5.0, 3.5)];
        let agg = aggregate_usage_data(&items).unwrap();
        assert!((agg.total_used - 5.0).abs() < 1e-6);
        assert_eq!(agg.total_budget, Some(15.0));
        assert!((agg.remaining.unwrap() - 10.0).abs() < 1e-6);
    }

    #[test]
    fn test_aggregate_empty_returns_none() {
        assert!(aggregate_usage_data(&[]).is_none());
    }
}
