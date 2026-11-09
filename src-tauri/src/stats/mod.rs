use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use chrono::{DateTime, Datelike, Duration, Local, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::config::app_config::{AppSettings, PollingMode};
use crate::providers::types::{ExtraUsage, UsageSummary};

const STATS_FILE_VERSION: u32 = 1;
const HISTORY_RETENTION_DAYS: i64 = 180;
const DEDUPE_WINDOW_MINUTES: i64 = 5;
const FORECAST_LOOKBACK_HOURS: i64 = 6;
const FORECAST_MIN_SAMPLES: usize = 3;
const STALE_SAMPLE_HOURS: i64 = 2;
const FLOAT_EPSILON: f64 = 0.000_001;
/// 修复 L12：统计落盘节流间隔。此前每次刷新都全量重写 usage_stats.json，
/// 高频轮询下写放大明显；样本已做 5 分钟去重，30s 节流不会丢失有效数据。
const STATS_SAVE_THROTTLE: std::time::Duration = std::time::Duration::from_secs(30);
/// 阶段四 perf：全文件样本数硬上限（按 captured_at 丢最旧）。
///
/// 估算依据（Task 3 真机诊断）：180 天保留 × 无样本数上限时文件线性膨胀，
/// 实测 3 供应商 + 5 分钟自动轮询下 140,833 样本 / 35.6MB（约 253 字节/样本）。
/// 50,000 条 ≈ 12MB 封顶；默认轻量用法（单供应商 × 5 分钟轮询 ≈ 288 样本/天）
/// 下 50k ≈ 173 天，180 天保留语义基本不受影响；极端高频用法以丢最旧为代价
/// 换取体量有界。UI 快照只消费 Day/Month 区间 + 6h 预测窗口，最旧样本被裁
/// 只影响导出/WebDAV 备份的历史长度，不影响任何展示。
const MAX_TOTAL_SAMPLES: usize = 50_000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum StatsRange {
    Day,
    Month,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum StatsHealthNoticeLevel {
    Info,
    Warning,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StatsHealthNoticeCode {
    EnableAutoRefresh,
    InsufficientSamples,
    StaleData,
    StartingToCollect,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatsHealthNotice {
    pub code: StatsHealthNoticeCode,
    pub level: StatsHealthNoticeLevel,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum UsageForecastStatus {
    Available,
    InsufficientData,
    NotApplicable,
    UnlikelyBeforeReset,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageForecast {
    pub status: UsageForecastStatus,
    pub estimated_at: Option<String>,
    pub hours_remaining: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SubscriptionTrendKind {
    Window,
    ExtraUsage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiStatsSummary {
    pub current_total_used: f64,
    pub range_used: f64,
    pub current_remaining: Option<f64>,
    pub currency: String,
    pub recent_velocity: Option<f64>,
    pub forecast: UsageForecast,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionTrendSummary {
    pub subscription_id: String,
    pub subscription_name: String,
    pub kind: SubscriptionTrendKind,
    pub label: String,
    pub current_utilization: f64,
    pub range_delta: f64,
    pub recent_velocity: Option<f64>,
    pub forecast: UsageForecast,
    pub resets_at: Option<String>,
    pub current_used: Option<f64>,
    pub current_limit: Option<f64>,
    pub currency: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderStatsSnapshot {
    pub provider_id: String,
    pub display_name: String,
    pub api_summary: Option<ApiStatsSummary>,
    #[serde(default)]
    pub subscription_trends: Vec<SubscriptionTrendSummary>,
    pub last_sample_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageStatsSnapshot {
    pub range: StatsRange,
    pub generated_at: String,
    #[serde(default)]
    pub health_notices: Vec<StatsHealthNotice>,
    #[serde(default)]
    pub providers: Vec<ProviderStatsSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UsageValueSample {
    captured_at: String,
    total_used: f64,
    total_budget: Option<f64>,
    remaining: Option<f64>,
    period_start: Option<String>,
    period_end: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UsageSeriesHistory {
    label: String,
    currency: String,
    #[serde(default)]
    samples: Vec<UsageValueSample>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UtilizationSample {
    captured_at: String,
    utilization: f64,
    resets_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UtilizationSeriesHistory {
    label: String,
    #[serde(default)]
    samples: Vec<UtilizationSample>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ExtraUsageSample {
    captured_at: String,
    used: f64,
    limit: Option<f64>,
    utilization: Option<f64>,
    resets_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ExtraUsageHistory {
    label: String,
    currency: String,
    #[serde(default)]
    samples: Vec<ExtraUsageSample>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SubscriptionHistory {
    subscription_name: String,
    #[serde(default)]
    windows: HashMap<String, UtilizationSeriesHistory>,
    extra_usage: Option<ExtraUsageHistory>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ProviderStatsHistory {
    display_name: String,
    api_summary: Option<UsageSeriesHistory>,
    #[serde(default)]
    api_keys: HashMap<String, UsageSeriesHistory>,
    #[serde(default)]
    subscriptions: HashMap<String, SubscriptionHistory>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UsageStatsFile {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    providers: HashMap<String, ProviderStatsHistory>,
}

impl Default for UsageStatsFile {
    fn default() -> Self {
        Self {
            version: STATS_FILE_VERSION,
            providers: HashMap::new(),
        }
    }
}

pub struct UsageStatsStore {
    stats: Arc<RwLock<UsageStatsFile>>,
    stats_path: PathBuf,
    /// 修复 L12：落盘节流状态（dirty 标记 + 上次落盘时间）
    write_state: Arc<RwLock<StatsWriteState>>,
    /// 阶段四 perf：后台加载完成标志。true = usage_stats.json 已读入并换入内存。
    /// 读取类路径（快照/导出/导入/移除）先 `wait_until_loaded`，写入类路径（record）
    /// 在加载完成前直接丢弃样本，避免把样本写进空历史后被换入的文件覆盖。
    loaded_tx: tokio::sync::watch::Sender<bool>,
    loaded_rx: tokio::sync::watch::Receiver<bool>,
}

/// 修复 L12：统计落盘节流状态
struct StatsWriteState {
    /// 内存中有尚未落盘的修改
    dirty: bool,
    /// 上次落盘（或尝试落盘）的时间；None 表示启动后从未写入
    last_save_at: Option<std::time::Instant>,
}

/// 从磁盘读入 usage_stats.json（损坏备份为 .bak 再回退空历史，M9 语义不变）。
/// 纯同步函数，供后台任务 `spawn_blocking` 调用与单测直接验证。
fn load_stats_file(stats_path: &std::path::Path) -> UsageStatsFile {
    if !stats_path.exists() {
        return UsageStatsFile::default();
    }

    match std::fs::read_to_string(stats_path) {
        Ok(content) => match serde_json::from_str::<UsageStatsFile>(&content) {
            Ok(stats) => stats,
            Err(error) => {
                // 修复 M9：统计文件损坏时备份为 usage_stats.json.bak 再回退空历史，
                // 避免 180 天历史样本彻底丢失且无任何现场可查。
                eprintln!("解析 usage_stats.json 失败: {}，回退空统计", error);
                crate::config::atomic::backup_corrupted_file(stats_path);
                UsageStatsFile::default()
            }
        },
        Err(_) => UsageStatsFile::default(),
    }
}

impl UsageStatsStore {
    /// 阶段四 perf：不再同步读盘。构造只建空历史 + 未加载标志，
    /// 真正的 usage_stats.json 解析由 [`Self::start_background_load`] 在后台完成，
    /// 启动关键路径（setup 闭包）不再被 35MB 级文件的同步解析阻塞
    /// （Task 3 实测该段 620-629ms，占 setup 总耗时 ~97%）。
    pub fn new(app_data_dir: PathBuf) -> Self {
        let stats_path = app_data_dir.join("usage_stats.json");
        let (loaded_tx, loaded_rx) = tokio::sync::watch::channel(false);
        Self {
            stats: Arc::new(RwLock::new(UsageStatsFile::default())),
            stats_path,
            write_state: Arc::new(RwLock::new(StatsWriteState {
                dirty: false,
                last_save_at: None,
            })),
            loaded_tx,
            loaded_rx,
        }
    }

    /// 后台加载 usage_stats.json 并换入内存（lib.rs setup 时调用一次）。
    /// 文件读+解析放 `spawn_blocking`，不占 async worker；换入在写锁下完成，
    /// 完成后置 loaded 标志。读取/导入/移除路径会等该标志，行为与同步加载等价。
    pub fn start_background_load(&self) {
        let stats = Arc::clone(&self.stats);
        let stats_path = self.stats_path.clone();
        let loaded_tx = self.loaded_tx.clone();
        tauri::async_runtime::spawn(async move {
            let handle = tauri::async_runtime::spawn_blocking(move || {
                let mut file = load_stats_file(&stats_path);
                prune_stats_file(&mut file, Utc::now());
                file
            });
            // 读失败/损坏已回退空历史（load_stats_file 兜底），不会 panic；
            // 无论结果如何都要换入并置标志，否则读取路径会永远等待。
            let loaded = handle.await.unwrap_or_default();
            {
                let mut guard = stats.write().await;
                *guard = loaded;
            }
            let _ = loaded_tx.send(true);
        });
    }

    /// 等待后台加载完成。store 存活期间 sender 不会 drop，
    /// `wait_for` 的 Err 分支只在 store 已销毁时出现，此时直接返回即可。
    pub async fn wait_until_loaded(&self) {
        if *self.loaded_rx.borrow() {
            return;
        }
        let _ = self.loaded_rx.clone().wait_for(|loaded| *loaded).await;
    }

    /// 直接同步执行后台加载（供单测使用，生产路径走 [`Self::start_background_load`]）。
    #[cfg(test)]
    pub async fn load_from_disk(&self) {
        let mut loaded = load_stats_file(&self.stats_path);
        prune_stats_file(&mut loaded, Utc::now());
        {
            let mut guard = self.stats.write().await;
            *guard = loaded;
        }
        let _ = self.loaded_tx.send(true);
    }

    async fn save(&self) -> Result<(), String> {
        let stats = self.stats.read().await;
        // 阶段四 perf：compact 序列化。pretty 格式额外膨胀 ~30%（Task 3 实测 35MB），
        // 每次 30s 节流落盘都要付出序列化+写盘代价；JSON 语义两种格式完全等价。
        let content = serde_json::to_string(&*stats)
            .map_err(|error| format!("序列化统计数据失败: {}", error))?;

        if let Some(parent) = self.stats_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("创建统计目录失败: {}", error))?;
        }

        // 修复 M9：原子写入（tmp + rename），写入中途崩溃不会留下半截 usage_stats.json
        crate::config::atomic::atomic_write(&self.stats_path, content.as_bytes())
            .map_err(|error| format!("写入统计文件失败: {}", error))?;

        Ok(())
    }

    pub async fn record_summaries(&self, summaries: &[UsageSummary]) -> Result<(), String> {
        // 阶段四 perf：后台加载完成前丢弃样本（选择"丢弃+日志"而非缓冲）。
        // 启动后数秒内的采样对 180 天趋势无意义，缓冲会让写入/换入两个状态机
        // 互相纠缠。首刷通常在加载完成后才返回（加载 ~0.1-0.6s），实际丢弃概率低。
        if !*self.loaded_rx.borrow() {
            eprintln!(
                "统计历史尚未加载完成，丢弃本次 {} 条样本（启动加载窗口内的采样）",
                summaries.len()
            );
            return Ok(());
        }

        let now = Utc::now();
        {
            let mut stats = self.stats.write().await;
            for summary in summaries {
                record_summary(&mut stats, summary, now);
            }
            prune_stats_file(&mut stats, now);
        }

        // 修复 L12：写放大控制。内存样本先标记 dirty，距上次落盘不足
        // STATS_SAVE_THROTTLE 时跳过写盘（数据仍在内存，到点或退出时由
        // flush_if_dirty 兜底落盘）；remove_provider 等结构性修改走 force_save。
        {
            let mut state = self.write_state.write().await;
            state.dirty = true;
            if let Some(last_save_at) = state.last_save_at {
                if last_save_at.elapsed() < STATS_SAVE_THROTTLE {
                    return Ok(());
                }
            }
            state.last_save_at = Some(std::time::Instant::now());
            state.dirty = false;
        }

        match self.save().await {
            Ok(()) => Ok(()),
            Err(error) => {
                // 落盘失败：重新标记 dirty，避免丢失"有未落盘修改"的事实
                self.write_state.write().await.dirty = true;
                Err(error)
            }
        }
    }

    /// 强制落盘（绕过节流），用于 remove_provider 等结构性修改
    async fn force_save(&self) -> Result<(), String> {
        let result = self.save().await;
        let mut state = self.write_state.write().await;
        state.last_save_at = Some(std::time::Instant::now());
        state.dirty = result.is_err();
        result
    }

    /// 退出前兜底落盘：仅在有未落盘修改时写入（修复 L12，由 lib.rs 退出事件调用）。
    /// 刻意不等待后台加载：加载完成前 record 一律丢弃、dirty 恒为 false，
    /// 这里立即返回 Ok 不丢任何数据，且保证 ExitRequested 不会被加载阻塞。
    pub async fn flush_if_dirty(&self) -> Result<(), String> {
        if !self.write_state.read().await.dirty {
            return Ok(());
        }
        self.force_save().await
    }

    pub async fn remove_provider(&self, provider_id: &str) -> Result<(), String> {
        // 阶段四 perf：等加载完成再删，避免"空历史上删除(无效果)+强制落盘空文件，
        // 加载完成后被旧文件覆盖"导致被删供应商的历史复活。
        self.wait_until_loaded().await;
        {
            let mut stats = self.stats.write().await;
            stats.providers.remove(provider_id);
        }
        self.force_save().await
    }

    pub async fn get_snapshot(
        &self,
        range: StatsRange,
        settings: &AppSettings,
        enabled_provider_ids: &[String],
    ) -> UsageStatsSnapshot {
        // 行为契约：统计抽屉只由用户点击打开（WidgetContainer 底部按钮），
        // 打开早于加载完成时走前端既有 loading 态，加载完成后本次 IPC 返回完整数据。
        self.wait_until_loaded().await;
        let now = Utc::now();
        let stats = self.stats.read().await;
        build_snapshot(&stats, &range, settings, enabled_provider_ids, now)
    }

    pub async fn get_snapshot_json(&self) -> Result<serde_json::Value, String> {
        // 导出/WebDAV 快照必须等加载完成，否则会把空历史当成基线/导出内容。
        self.wait_until_loaded().await;
        let stats = self.stats.read().await;
        serde_json::to_value(&*stats).map_err(|error| format!("序列化用量历史失败: {}", error))
    }

    pub fn validate_snapshot_json(&self, snapshot: &serde_json::Value) -> Result<(), String> {
        serde_json::from_value::<UsageStatsFile>(snapshot.clone())
            .map(|_| ())
            .map_err(|error| format!("导入用量历史失败: {}", error))
    }

    pub async fn replace_all_from_json(&self, snapshot: serde_json::Value) -> Result<(), String> {
        // 导入必须在加载完成后执行，否则后台加载的换入会把导入结果覆盖回旧文件。
        self.wait_until_loaded().await;
        let mut snapshot = serde_json::from_value::<UsageStatsFile>(snapshot)
            .map_err(|error| format!("导入用量历史失败: {}", error))?;
        prune_stats_file(&mut snapshot, Utc::now());

        {
            let mut stats = self.stats.write().await;
            *stats = snapshot;
        }

        let result = self.save().await;
        {
            let mut state = self.write_state.write().await;
            state.last_save_at = Some(std::time::Instant::now());
            state.dirty = result.is_err();
        }

        result
    }
}

fn record_summary(stats: &mut UsageStatsFile, summary: &UsageSummary, now: DateTime<Utc>) {
    let captured_at = summary
        .last_updated
        .as_deref()
        .and_then(parse_timestamp)
        .unwrap_or(now)
        .to_rfc3339();

    let provider_history = stats
        .providers
        .entry(provider_id_to_string(&summary.provider_id))
        .or_default();
    provider_history.display_name = summary.display_name.clone();

    if let Some(usage) = summary.usage.as_ref() {
        let api_summary = provider_history
            .api_summary
            .get_or_insert_with(|| UsageSeriesHistory {
                label: "api".to_string(),
                currency: usage.currency.clone(),
                samples: Vec::new(),
            });
        api_summary.currency = usage.currency.clone();
        push_usage_sample(
            &mut api_summary.samples,
            UsageValueSample {
                captured_at: captured_at.clone(),
                total_used: usage.total_used,
                total_budget: usage.total_budget,
                remaining: usage.remaining,
                period_start: usage.period_start.clone(),
                period_end: usage.period_end.clone(),
            },
        );
    }

    for api_key in &summary.api_key_usages {
        if let Some(usage) = api_key.usage.as_ref() {
            let api_key_history = provider_history
                .api_keys
                .entry(api_key.key_id.clone())
                .or_insert_with(|| UsageSeriesHistory {
                    label: api_key.key_name.clone(),
                    currency: usage.currency.clone(),
                    samples: Vec::new(),
                });
            api_key_history.label = api_key.key_name.clone();
            api_key_history.currency = usage.currency.clone();
            push_usage_sample(
                &mut api_key_history.samples,
                UsageValueSample {
                    captured_at: captured_at.clone(),
                    total_used: usage.total_used,
                    total_budget: usage.total_budget,
                    remaining: usage.remaining,
                    period_start: usage.period_start.clone(),
                    period_end: usage.period_end.clone(),
                },
            );
        }
    }

    for subscription in &summary.subscriptions {
        if !provider_status_is_success(&subscription.usage.status) {
            continue;
        }

        let subscription_history = provider_history
            .subscriptions
            .entry(subscription.subscription_id.clone())
            .or_insert_with(|| SubscriptionHistory {
                subscription_name: subscription.subscription_name.clone(),
                windows: HashMap::new(),
                extra_usage: None,
            });
        subscription_history.subscription_name = subscription.subscription_name.clone();

        for window in &subscription.usage.windows {
            let window_history = subscription_history
                .windows
                .entry(window.label.clone())
                .or_insert_with(|| UtilizationSeriesHistory {
                    label: window.label.clone(),
                    samples: Vec::new(),
                });
            window_history.label = window.label.clone();
            push_utilization_sample(
                &mut window_history.samples,
                UtilizationSample {
                    captured_at: captured_at.clone(),
                    utilization: window.utilization,
                    resets_at: window.resets_at.clone(),
                },
            );
        }

        if let Some(extra_usage) = subscription.usage.extra_usage.as_ref() {
            if extra_usage.is_enabled {
                let extra_usage_history =
                    subscription_history
                        .extra_usage
                        .get_or_insert_with(|| ExtraUsageHistory {
                            label: "extraUsage".to_string(),
                            currency: "USD".to_string(),
                            samples: Vec::new(),
                        });
                push_extra_usage_sample(
                    &mut extra_usage_history.samples,
                    extra_usage,
                    captured_at.clone(),
                );
            }
        }
    }
}

fn push_usage_sample(samples: &mut Vec<UsageValueSample>, next: UsageValueSample) {
    let Some(last) = samples.last() else {
        samples.push(next);
        return;
    };

    let last_timestamp = parse_timestamp(&last.captured_at);
    let next_timestamp = parse_timestamp(&next.captured_at);
    let within_dedupe_window = match (last_timestamp, next_timestamp) {
        (Some(left), Some(right)) => (right - left) <= Duration::minutes(DEDUPE_WINDOW_MINUTES),
        _ => false,
    };

    if within_dedupe_window
        && nearly_equal(last.total_used, next.total_used)
        && optional_nearly_equal(last.total_budget, next.total_budget)
        && optional_nearly_equal(last.remaining, next.remaining)
        && last.period_start == next.period_start
        && last.period_end == next.period_end
    {
        return;
    }

    samples.push(next);
}

fn push_utilization_sample(samples: &mut Vec<UtilizationSample>, next: UtilizationSample) {
    let Some(last) = samples.last() else {
        samples.push(next);
        return;
    };

    let last_timestamp = parse_timestamp(&last.captured_at);
    let next_timestamp = parse_timestamp(&next.captured_at);
    let within_dedupe_window = match (last_timestamp, next_timestamp) {
        (Some(left), Some(right)) => (right - left) <= Duration::minutes(DEDUPE_WINDOW_MINUTES),
        _ => false,
    };

    if within_dedupe_window
        && nearly_equal(last.utilization, next.utilization)
        && last.resets_at == next.resets_at
    {
        return;
    }

    samples.push(next);
}

fn push_extra_usage_sample(
    samples: &mut Vec<ExtraUsageSample>,
    extra_usage: &ExtraUsage,
    captured_at: String,
) {
    let next = ExtraUsageSample {
        captured_at,
        used: extra_usage.used_usd.unwrap_or(0.0),
        limit: extra_usage.monthly_limit_usd,
        utilization: extra_usage.utilization,
        resets_at: extra_usage.resets_at.clone(),
    };

    let Some(last) = samples.last() else {
        samples.push(next);
        return;
    };

    let last_timestamp = parse_timestamp(&last.captured_at);
    let next_timestamp = parse_timestamp(&next.captured_at);
    let within_dedupe_window = match (last_timestamp, next_timestamp) {
        (Some(left), Some(right)) => (right - left) <= Duration::minutes(DEDUPE_WINDOW_MINUTES),
        _ => false,
    };

    if within_dedupe_window
        && nearly_equal(last.used, next.used)
        && optional_nearly_equal(last.limit, next.limit)
        && optional_nearly_equal(last.utilization, next.utilization)
        && last.resets_at == next.resets_at
    {
        return;
    }

    samples.push(next);
}

fn prune_stats_file(stats: &mut UsageStatsFile, now: DateTime<Utc>) {
    let cutoff = now - Duration::days(HISTORY_RETENTION_DAYS);
    prune_samples_in_file(stats, |timestamp| timestamp >= cutoff);
    enforce_sample_cap(stats, MAX_TOTAL_SAMPLES);
}

/// 按谓词裁剪全文件样本（保留谓词为 true 的样本），并清理裁空后的空序列/空供应商。
fn prune_samples_in_file(stats: &mut UsageStatsFile, keep: impl Fn(DateTime<Utc>) -> bool) {
    stats.providers.retain(|_, provider| {
        if let Some(api_summary) = provider.api_summary.as_mut() {
            api_summary
                .samples
                .retain(|sample| parse_timestamp(&sample.captured_at).is_some_and(&keep));
        }

        provider.api_keys.retain(|_, series| {
            series
                .samples
                .retain(|sample| parse_timestamp(&sample.captured_at).is_some_and(&keep));
            !series.samples.is_empty()
        });

        provider.subscriptions.retain(|_, subscription| {
            subscription.windows.retain(|_, series| {
                series
                    .samples
                    .retain(|sample| parse_timestamp(&sample.captured_at).is_some_and(&keep));
                !series.samples.is_empty()
            });

            if let Some(extra_usage) = subscription.extra_usage.as_mut() {
                extra_usage
                    .samples
                    .retain(|sample| parse_timestamp(&sample.captured_at).is_some_and(&keep));
            }

            let has_extra_usage = subscription
                .extra_usage
                .as_ref()
                .is_some_and(|extra_usage| !extra_usage.samples.is_empty());
            !subscription.windows.is_empty() || has_extra_usage
        });

        provider
            .api_summary
            .as_ref()
            .is_some_and(|api_summary| !api_summary.samples.is_empty())
            || !provider.api_keys.is_empty()
            || !provider.subscriptions.is_empty()
    });
}

/// 阶段四 perf：全文件样本数硬上限，超限时丢最旧。
///
/// 做法：收集全部样本时间戳降序排序，取第 max 条的时间为分界，
/// 一次性保留 >= 分界的样本。只在超限时才做 O(N log N) 排序，
/// 未超限的常规路径（每次刷新的 record prune）只做 O(序列数) 计数。
/// 分界时间点上的并列样本（同一次刷新写入的多序列同秒样本）会全部保留，
/// 实际保留数可能超出 max 一个刷新批次的量，属可接受的有界余量。
fn enforce_sample_cap(stats: &mut UsageStatsFile, max_total_samples: usize) {
    if count_total_samples(stats) <= max_total_samples {
        return;
    }

    let mut timestamps: Vec<DateTime<Utc>> = Vec::new();
    collect_sample_timestamps(stats, &mut timestamps);
    timestamps.sort_unstable_by(|left, right| right.cmp(left));

    let Some(cutoff) = timestamps.get(max_total_samples.saturating_sub(1)).copied() else {
        return;
    };

    prune_samples_in_file(stats, |timestamp| timestamp >= cutoff);
}

fn count_total_samples(stats: &UsageStatsFile) -> usize {
    let mut total = 0usize;
    for provider in stats.providers.values() {
        if let Some(api_summary) = provider.api_summary.as_ref() {
            total += api_summary.samples.len();
        }
        for series in provider.api_keys.values() {
            total += series.samples.len();
        }
        for subscription in provider.subscriptions.values() {
            for window in subscription.windows.values() {
                total += window.samples.len();
            }
            if let Some(extra_usage) = subscription.extra_usage.as_ref() {
                total += extra_usage.samples.len();
            }
        }
    }
    total
}

fn collect_sample_timestamps(stats: &UsageStatsFile, out: &mut Vec<DateTime<Utc>>) {
    for provider in stats.providers.values() {
        if let Some(api_summary) = provider.api_summary.as_ref() {
            out.extend(
                api_summary
                    .samples
                    .iter()
                    .filter_map(|sample| parse_timestamp(&sample.captured_at)),
            );
        }
        for series in provider.api_keys.values() {
            out.extend(
                series
                    .samples
                    .iter()
                    .filter_map(|sample| parse_timestamp(&sample.captured_at)),
            );
        }
        for subscription in provider.subscriptions.values() {
            for window in subscription.windows.values() {
                out.extend(
                    window
                        .samples
                        .iter()
                        .filter_map(|sample| parse_timestamp(&sample.captured_at)),
                );
            }
            if let Some(extra_usage) = subscription.extra_usage.as_ref() {
                out.extend(
                    extra_usage
                        .samples
                        .iter()
                        .filter_map(|sample| parse_timestamp(&sample.captured_at)),
                );
            }
        }
    }
}

fn build_snapshot(
    stats: &UsageStatsFile,
    range: &StatsRange,
    settings: &AppSettings,
    enabled_provider_ids: &[String],
    now: DateTime<Utc>,
) -> UsageStatsSnapshot {
    let enabled_provider_set: HashSet<&str> =
        enabled_provider_ids.iter().map(|id| id.as_str()).collect();
    let mut providers = Vec::new();

    for provider_id in enabled_provider_ids {
        let Some(provider_history) = stats.providers.get(provider_id) else {
            continue;
        };

        let api_summary = provider_history
            .api_summary
            .as_ref()
            .and_then(|series| build_api_stats_summary(series, range, now));

        let mut subscription_trends = Vec::new();
        for (subscription_id, subscription_history) in &provider_history.subscriptions {
            for window_history in subscription_history.windows.values() {
                if let Some(trend) = build_window_trend(
                    subscription_id,
                    &subscription_history.subscription_name,
                    window_history,
                    range,
                    now,
                ) {
                    subscription_trends.push(trend);
                }
            }

            if let Some(extra_usage_history) = subscription_history.extra_usage.as_ref() {
                if let Some(trend) = build_extra_usage_trend(
                    subscription_id,
                    &subscription_history.subscription_name,
                    extra_usage_history,
                    range,
                    now,
                ) {
                    subscription_trends.push(trend);
                }
            }
        }

        let last_sample_at = provider_latest_sample_at(provider_history);
        if api_summary.is_none() && subscription_trends.is_empty() && last_sample_at.is_none() {
            continue;
        }

        providers.push(ProviderStatsSnapshot {
            provider_id: provider_id.clone(),
            display_name: provider_history.display_name.clone(),
            api_summary,
            subscription_trends,
            last_sample_at,
        });
    }

    let latest_sample_at = providers
        .iter()
        .filter_map(|provider| provider.last_sample_at.as_deref())
        .filter_map(parse_timestamp)
        .max();

    let mut health_notices = Vec::new();
    if !enabled_provider_ids.is_empty()
        && should_suggest_auto_refresh(settings, enabled_provider_ids)
    {
        health_notices.push(StatsHealthNotice {
            code: StatsHealthNoticeCode::EnableAutoRefresh,
            level: StatsHealthNoticeLevel::Info,
        });
    }

    if !enabled_provider_set.is_empty() && providers.is_empty() {
        health_notices.push(StatsHealthNotice {
            code: StatsHealthNoticeCode::StartingToCollect,
            level: StatsHealthNoticeLevel::Info,
        });
    } else {
        if latest_sample_at
            .is_some_and(|sample_time| now - sample_time > Duration::hours(STALE_SAMPLE_HOURS))
        {
            health_notices.push(StatsHealthNotice {
                code: StatsHealthNoticeCode::StaleData,
                level: StatsHealthNoticeLevel::Warning,
            });
        }

        if !providers.is_empty()
            && !providers_have_forecast_ready_samples(stats, enabled_provider_ids, now)
        {
            health_notices.push(StatsHealthNotice {
                code: StatsHealthNoticeCode::InsufficientSamples,
                level: StatsHealthNoticeLevel::Info,
            });
        }
    }

    UsageStatsSnapshot {
        range: range.clone(),
        generated_at: now.to_rfc3339(),
        health_notices,
        providers,
    }
}

fn build_api_stats_summary(
    series: &UsageSeriesHistory,
    range: &StatsRange,
    now: DateTime<Utc>,
) -> Option<ApiStatsSummary> {
    let latest = series.samples.last()?;
    let recent_velocity = compute_usage_velocity(&series.samples, now);
    let latest_timestamp = parse_timestamp(&latest.captured_at).unwrap_or(now);

    Some(ApiStatsSummary {
        current_total_used: latest.total_used,
        range_used: compute_usage_range_delta(&series.samples, range, now),
        current_remaining: latest.remaining,
        currency: series.currency.clone(),
        recent_velocity,
        forecast: build_api_forecast(latest, recent_velocity, latest_timestamp),
    })
}

fn build_window_trend(
    subscription_id: &str,
    subscription_name: &str,
    history: &UtilizationSeriesHistory,
    range: &StatsRange,
    now: DateTime<Utc>,
) -> Option<SubscriptionTrendSummary> {
    let latest = history.samples.last()?;
    let recent_velocity = compute_utilization_velocity(&history.samples, now);
    let latest_timestamp = parse_timestamp(&latest.captured_at).unwrap_or(now);

    Some(SubscriptionTrendSummary {
        subscription_id: subscription_id.to_string(),
        subscription_name: subscription_name.to_string(),
        kind: SubscriptionTrendKind::Window,
        label: history.label.clone(),
        current_utilization: latest.utilization,
        range_delta: compute_utilization_range_delta(&history.samples, range, now),
        recent_velocity,
        forecast: build_utilization_forecast(
            latest.utilization,
            recent_velocity,
            latest.resets_at.clone(),
            latest_timestamp,
        ),
        resets_at: latest.resets_at.clone(),
        current_used: None,
        current_limit: None,
        currency: None,
    })
}

fn build_extra_usage_trend(
    subscription_id: &str,
    subscription_name: &str,
    history: &ExtraUsageHistory,
    range: &StatsRange,
    now: DateTime<Utc>,
) -> Option<SubscriptionTrendSummary> {
    let latest = history.samples.last()?;
    let current_utilization = latest.utilization.unwrap_or_else(|| {
        latest
            .limit
            .filter(|limit| *limit > 0.0)
            .map(|limit| (latest.used / limit * 100.0).clamp(0.0, 100.0))
            .unwrap_or(0.0)
    });
    let usage_velocity = compute_extra_usage_velocity(&history.samples, now);
    let recent_velocity = usage_velocity.and_then(|velocity| {
        latest
            .limit
            .filter(|limit| *limit > 0.0)
            .map(|limit| (velocity / limit) * 100.0)
    });
    let latest_timestamp = parse_timestamp(&latest.captured_at).unwrap_or(now);

    Some(SubscriptionTrendSummary {
        subscription_id: subscription_id.to_string(),
        subscription_name: subscription_name.to_string(),
        kind: SubscriptionTrendKind::ExtraUsage,
        label: history.label.clone(),
        current_utilization,
        range_delta: compute_extra_usage_range_delta(&history.samples, range, now)
            .and_then(|used_delta| {
                latest
                    .limit
                    .filter(|limit| *limit > 0.0)
                    .map(|limit| (used_delta / limit) * 100.0)
            })
            .unwrap_or(0.0),
        recent_velocity,
        forecast: build_extra_usage_forecast(latest, usage_velocity, latest_timestamp),
        resets_at: latest.resets_at.clone(),
        current_used: Some(latest.used),
        current_limit: latest.limit,
        currency: Some(history.currency.clone()),
    })
}

fn build_api_forecast(
    latest: &UsageValueSample,
    recent_velocity: Option<f64>,
    latest_timestamp: DateTime<Utc>,
) -> UsageForecast {
    let Some(remaining) = latest.remaining else {
        return UsageForecast {
            status: UsageForecastStatus::NotApplicable,
            estimated_at: None,
            hours_remaining: None,
        };
    };

    if remaining <= 0.0 {
        return UsageForecast {
            status: UsageForecastStatus::Available,
            estimated_at: Some(latest_timestamp.to_rfc3339()),
            hours_remaining: Some(0.0),
        };
    }

    let Some(velocity) = recent_velocity else {
        return UsageForecast {
            status: UsageForecastStatus::InsufficientData,
            estimated_at: None,
            hours_remaining: None,
        };
    };

    if velocity <= FLOAT_EPSILON {
        return UsageForecast {
            status: UsageForecastStatus::InsufficientData,
            estimated_at: None,
            hours_remaining: None,
        };
    }

    let hours_remaining = remaining / velocity;
    UsageForecast {
        status: UsageForecastStatus::Available,
        estimated_at: Some(estimate_time(latest_timestamp, hours_remaining)),
        hours_remaining: Some(hours_remaining.max(0.0)),
    }
}

fn build_utilization_forecast(
    current_utilization: f64,
    recent_velocity: Option<f64>,
    resets_at: Option<String>,
    latest_timestamp: DateTime<Utc>,
) -> UsageForecast {
    if current_utilization >= 100.0 {
        return UsageForecast {
            status: UsageForecastStatus::Available,
            estimated_at: Some(latest_timestamp.to_rfc3339()),
            hours_remaining: Some(0.0),
        };
    }

    let Some(velocity) = recent_velocity else {
        return UsageForecast {
            status: UsageForecastStatus::InsufficientData,
            estimated_at: None,
            hours_remaining: None,
        };
    };

    if velocity <= FLOAT_EPSILON {
        return UsageForecast {
            status: UsageForecastStatus::InsufficientData,
            estimated_at: None,
            hours_remaining: None,
        };
    }

    let hours_remaining = (100.0 - current_utilization).max(0.0) / velocity;
    let estimated_at = estimate_time(latest_timestamp, hours_remaining);

    if let Some(reset_time) = resets_at.as_deref().and_then(parse_timestamp) {
        if let Some(estimate_time) = parse_timestamp(&estimated_at) {
            if estimate_time > reset_time {
                return UsageForecast {
                    status: UsageForecastStatus::UnlikelyBeforeReset,
                    estimated_at: Some(estimated_at),
                    hours_remaining: Some(hours_remaining.max(0.0)),
                };
            }
        }
    }

    UsageForecast {
        status: UsageForecastStatus::Available,
        estimated_at: Some(estimated_at),
        hours_remaining: Some(hours_remaining.max(0.0)),
    }
}

fn build_extra_usage_forecast(
    latest: &ExtraUsageSample,
    recent_velocity: Option<f64>,
    latest_timestamp: DateTime<Utc>,
) -> UsageForecast {
    let Some(limit) = latest.limit else {
        return UsageForecast {
            status: UsageForecastStatus::NotApplicable,
            estimated_at: None,
            hours_remaining: None,
        };
    };

    let remaining = (limit - latest.used).max(0.0);
    if remaining <= 0.0 {
        return UsageForecast {
            status: UsageForecastStatus::Available,
            estimated_at: Some(latest_timestamp.to_rfc3339()),
            hours_remaining: Some(0.0),
        };
    }

    let Some(velocity) = recent_velocity else {
        return UsageForecast {
            status: UsageForecastStatus::InsufficientData,
            estimated_at: None,
            hours_remaining: None,
        };
    };

    if velocity <= FLOAT_EPSILON {
        return UsageForecast {
            status: UsageForecastStatus::InsufficientData,
            estimated_at: None,
            hours_remaining: None,
        };
    }

    let hours_remaining = remaining / velocity;
    let estimated_at = estimate_time(latest_timestamp, hours_remaining);
    if let Some(reset_time) = latest.resets_at.as_deref().and_then(parse_timestamp) {
        if let Some(estimate_time) = parse_timestamp(&estimated_at) {
            if estimate_time > reset_time {
                return UsageForecast {
                    status: UsageForecastStatus::UnlikelyBeforeReset,
                    estimated_at: Some(estimated_at),
                    hours_remaining: Some(hours_remaining.max(0.0)),
                };
            }
        }
    }

    UsageForecast {
        status: UsageForecastStatus::Available,
        estimated_at: Some(estimated_at),
        hours_remaining: Some(hours_remaining.max(0.0)),
    }
}

fn compute_usage_range_delta(
    samples: &[UsageValueSample],
    range: &StatsRange,
    now: DateTime<Utc>,
) -> f64 {
    let relevant = filter_usage_samples_in_range(samples, range, now);
    accumulate_usage_delta(&relevant)
}

fn compute_utilization_range_delta(
    samples: &[UtilizationSample],
    range: &StatsRange,
    now: DateTime<Utc>,
) -> f64 {
    let relevant = filter_utilization_samples_in_range(samples, range, now);
    accumulate_utilization_delta(&relevant)
}

fn compute_extra_usage_range_delta(
    samples: &[ExtraUsageSample],
    range: &StatsRange,
    now: DateTime<Utc>,
) -> Option<f64> {
    let relevant = filter_extra_usage_samples_in_range(samples, range, now);
    Some(accumulate_extra_usage_delta(&relevant))
}

fn compute_usage_velocity(samples: &[UsageValueSample], now: DateTime<Utc>) -> Option<f64> {
    let recent = filter_recent_usage_samples(samples, now);
    if recent.len() < FORECAST_MIN_SAMPLES {
        return None;
    }

    let first = parse_timestamp(&recent.first()?.captured_at)?;
    let last = parse_timestamp(&recent.last()?.captured_at)?;
    let elapsed_seconds = (last - first).num_seconds();
    if elapsed_seconds <= 0 {
        return None;
    }

    let delta = accumulate_usage_delta(&recent);
    if delta <= FLOAT_EPSILON {
        return None;
    }

    Some(delta / (elapsed_seconds as f64 / 3600.0))
}

fn compute_utilization_velocity(samples: &[UtilizationSample], now: DateTime<Utc>) -> Option<f64> {
    let recent = filter_recent_utilization_samples(samples, now);
    if recent.len() < FORECAST_MIN_SAMPLES {
        return None;
    }

    let first = parse_timestamp(&recent.first()?.captured_at)?;
    let last = parse_timestamp(&recent.last()?.captured_at)?;
    let elapsed_seconds = (last - first).num_seconds();
    if elapsed_seconds <= 0 {
        return None;
    }

    let delta = accumulate_utilization_delta(&recent);
    if delta <= FLOAT_EPSILON {
        return None;
    }

    Some(delta / (elapsed_seconds as f64 / 3600.0))
}

fn compute_extra_usage_velocity(samples: &[ExtraUsageSample], now: DateTime<Utc>) -> Option<f64> {
    let recent = filter_recent_extra_usage_samples(samples, now);
    if recent.len() < FORECAST_MIN_SAMPLES {
        return None;
    }

    let first = parse_timestamp(&recent.first()?.captured_at)?;
    let last = parse_timestamp(&recent.last()?.captured_at)?;
    let elapsed_seconds = (last - first).num_seconds();
    if elapsed_seconds <= 0 {
        return None;
    }

    let delta = accumulate_extra_usage_delta(&recent);
    if delta <= FLOAT_EPSILON {
        return None;
    }

    Some(delta / (elapsed_seconds as f64 / 3600.0))
}

fn filter_usage_samples_in_range(
    samples: &[UsageValueSample],
    range: &StatsRange,
    now: DateTime<Utc>,
) -> Vec<UsageValueSample> {
    samples
        .iter()
        .filter(|sample| sample_in_current_range(&sample.captured_at, range, now))
        .cloned()
        .collect()
}

fn filter_utilization_samples_in_range(
    samples: &[UtilizationSample],
    range: &StatsRange,
    now: DateTime<Utc>,
) -> Vec<UtilizationSample> {
    samples
        .iter()
        .filter(|sample| sample_in_current_range(&sample.captured_at, range, now))
        .cloned()
        .collect()
}

fn filter_extra_usage_samples_in_range(
    samples: &[ExtraUsageSample],
    range: &StatsRange,
    now: DateTime<Utc>,
) -> Vec<ExtraUsageSample> {
    samples
        .iter()
        .filter(|sample| sample_in_current_range(&sample.captured_at, range, now))
        .cloned()
        .collect()
}

fn filter_recent_usage_samples(
    samples: &[UsageValueSample],
    now: DateTime<Utc>,
) -> Vec<UsageValueSample> {
    let cutoff = now - Duration::hours(FORECAST_LOOKBACK_HOURS);
    samples
        .iter()
        .filter(|sample| parse_timestamp(&sample.captured_at).is_some_and(|dt| dt >= cutoff))
        .cloned()
        .collect()
}

fn filter_recent_utilization_samples(
    samples: &[UtilizationSample],
    now: DateTime<Utc>,
) -> Vec<UtilizationSample> {
    let cutoff = now - Duration::hours(FORECAST_LOOKBACK_HOURS);
    samples
        .iter()
        .filter(|sample| parse_timestamp(&sample.captured_at).is_some_and(|dt| dt >= cutoff))
        .cloned()
        .collect()
}

fn filter_recent_extra_usage_samples(
    samples: &[ExtraUsageSample],
    now: DateTime<Utc>,
) -> Vec<ExtraUsageSample> {
    let cutoff = now - Duration::hours(FORECAST_LOOKBACK_HOURS);
    samples
        .iter()
        .filter(|sample| parse_timestamp(&sample.captured_at).is_some_and(|dt| dt >= cutoff))
        .cloned()
        .collect()
}

fn accumulate_usage_delta(samples: &[UsageValueSample]) -> f64 {
    let mut total = 0.0;
    for pair in samples.windows(2) {
        let previous = &pair[0];
        let current = &pair[1];

        if usage_series_reset(previous, current) {
            continue;
        }

        let delta = current.total_used - previous.total_used;
        if delta > 0.0 {
            total += delta;
        }
    }
    total
}

fn accumulate_utilization_delta(samples: &[UtilizationSample]) -> f64 {
    let mut total = 0.0;
    for pair in samples.windows(2) {
        let previous = &pair[0];
        let current = &pair[1];

        if utilization_series_reset(previous, current) {
            continue;
        }

        let delta = current.utilization - previous.utilization;
        if delta > 0.0 {
            total += delta;
        }
    }
    total
}

fn accumulate_extra_usage_delta(samples: &[ExtraUsageSample]) -> f64 {
    let mut total = 0.0;
    for pair in samples.windows(2) {
        let previous = &pair[0];
        let current = &pair[1];

        if extra_usage_series_reset(previous, current) {
            continue;
        }

        let delta = current.used - previous.used;
        if delta > 0.0 {
            total += delta;
        }
    }
    total
}

fn usage_series_reset(previous: &UsageValueSample, current: &UsageValueSample) -> bool {
    previous.period_start != current.period_start
        || previous.period_end != current.period_end
        || current.total_used + FLOAT_EPSILON < previous.total_used
}

fn utilization_series_reset(previous: &UtilizationSample, current: &UtilizationSample) -> bool {
    previous.resets_at != current.resets_at
        || current.utilization + FLOAT_EPSILON < previous.utilization
}

fn extra_usage_series_reset(previous: &ExtraUsageSample, current: &ExtraUsageSample) -> bool {
    previous.resets_at != current.resets_at
        || !optional_nearly_equal(previous.limit, current.limit)
        || current.used + FLOAT_EPSILON < previous.used
}

fn provider_latest_sample_at(history: &ProviderStatsHistory) -> Option<String> {
    let mut latest: Option<DateTime<Utc>> = history
        .api_summary
        .as_ref()
        .and_then(|series| series.samples.last())
        .and_then(|sample| parse_timestamp(&sample.captured_at));

    for api_key_history in history.api_keys.values() {
        latest = max_timestamp(
            latest,
            api_key_history
                .samples
                .last()
                .and_then(|sample| parse_timestamp(&sample.captured_at)),
        );
    }

    for subscription in history.subscriptions.values() {
        for window in subscription.windows.values() {
            latest = max_timestamp(
                latest,
                window
                    .samples
                    .last()
                    .and_then(|sample| parse_timestamp(&sample.captured_at)),
            );
        }

        if let Some(extra_usage) = subscription.extra_usage.as_ref() {
            latest = max_timestamp(
                latest,
                extra_usage
                    .samples
                    .last()
                    .and_then(|sample| parse_timestamp(&sample.captured_at)),
            );
        }
    }

    latest.map(|timestamp| timestamp.to_rfc3339())
}

fn providers_have_forecast_ready_samples(
    stats: &UsageStatsFile,
    enabled_provider_ids: &[String],
    now: DateTime<Utc>,
) -> bool {
    for provider_id in enabled_provider_ids {
        let Some(provider_history) = stats.providers.get(provider_id) else {
            continue;
        };

        if provider_history.api_summary.as_ref().is_some_and(|series| {
            filter_recent_usage_samples(&series.samples, now).len() >= FORECAST_MIN_SAMPLES
        }) {
            return true;
        }

        for subscription in provider_history.subscriptions.values() {
            if subscription.windows.values().any(|series| {
                filter_recent_utilization_samples(&series.samples, now).len()
                    >= FORECAST_MIN_SAMPLES
            }) {
                return true;
            }

            if subscription.extra_usage.as_ref().is_some_and(|series| {
                filter_recent_extra_usage_samples(&series.samples, now).len()
                    >= FORECAST_MIN_SAMPLES
            }) {
                return true;
            }
        }
    }

    false
}

fn should_suggest_auto_refresh(settings: &AppSettings, enabled_provider_ids: &[String]) -> bool {
    if settings.polling_mode == PollingMode::Manual {
        return true;
    }

    !enabled_provider_ids.is_empty()
        && enabled_provider_ids.iter().all(|provider_id| {
            if settings.provider_polling_overrides_enabled {
                if let Some(override_settings) =
                    settings.provider_polling_overrides.get(provider_id)
                {
                    return override_settings.polling_mode == PollingMode::Manual;
                }
            }

            settings.polling_mode == PollingMode::Manual
        })
}

fn sample_in_current_range(captured_at: &str, range: &StatsRange, now: DateTime<Utc>) -> bool {
    let Some(timestamp) = parse_timestamp(captured_at) else {
        return false;
    };

    let local_timestamp = timestamp.with_timezone(&Local);
    let local_now = now.with_timezone(&Local);

    match range {
        StatsRange::Day => local_timestamp.date_naive() == local_now.date_naive(),
        StatsRange::Month => {
            local_timestamp.year() == local_now.year()
                && local_timestamp.month() == local_now.month()
        }
    }
}

fn parse_timestamp(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|timestamp| timestamp.with_timezone(&Utc))
}

fn estimate_time(start: DateTime<Utc>, hours_remaining: f64) -> String {
    let seconds = (hours_remaining.max(0.0) * 3600.0).round() as i64;
    (start + Duration::seconds(seconds)).to_rfc3339()
}

fn nearly_equal(left: f64, right: f64) -> bool {
    (left - right).abs() <= FLOAT_EPSILON
}

fn optional_nearly_equal(left: Option<f64>, right: Option<f64>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => nearly_equal(left, right),
        (None, None) => true,
        _ => false,
    }
}

fn max_timestamp(
    current: Option<DateTime<Utc>>,
    next: Option<DateTime<Utc>>,
) -> Option<DateTime<Utc>> {
    match (current, next) {
        (Some(left), Some(right)) => Some(left.max(right)),
        (Some(left), None) => Some(left),
        (None, Some(right)) => Some(right),
        (None, None) => None,
    }
}

fn provider_id_to_string(provider_id: &crate::providers::types::ProviderId) -> String {
    // ProviderId 已是 String 类型别名，直接克隆即可
    provider_id.clone()
}

fn provider_status_is_success(status: &crate::providers::types::ProviderStatus) -> bool {
    matches!(status, crate::providers::types::ProviderStatus::Success)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn fixed_time() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 4, 5, 12, 0, 0).unwrap()
    }

    fn usage_sample(hours_offset: i64, total_used: f64, period_start: &str) -> UsageValueSample {
        UsageValueSample {
            captured_at: (fixed_time() + Duration::hours(hours_offset)).to_rfc3339(),
            total_used,
            total_budget: Some(100.0),
            remaining: Some((100.0 - total_used).max(0.0)),
            period_start: Some(period_start.to_string()),
            period_end: Some("2026-04-30".to_string()),
        }
    }

    fn utilization_sample(
        hours_offset: i64,
        utilization: f64,
        resets_at: &str,
    ) -> UtilizationSample {
        UtilizationSample {
            captured_at: (fixed_time() + Duration::hours(hours_offset)).to_rfc3339(),
            utilization,
            resets_at: Some(resets_at.to_string()),
        }
    }

    #[test]
    fn prune_stats_removes_old_samples() {
        let mut stats = UsageStatsFile::default();
        stats.providers.insert(
            "openai".to_string(),
            ProviderStatsHistory {
                display_name: "OpenAI".to_string(),
                api_summary: Some(UsageSeriesHistory {
                    label: "api".to_string(),
                    currency: "USD".to_string(),
                    samples: vec![
                        UsageValueSample {
                            captured_at: (fixed_time() - Duration::days(181)).to_rfc3339(),
                            total_used: 1.0,
                            total_budget: Some(10.0),
                            remaining: Some(9.0),
                            period_start: Some("2025-10-01".to_string()),
                            period_end: Some("2025-10-31".to_string()),
                        },
                        usage_sample(0, 2.0, "2026-04-01"),
                    ],
                }),
                api_keys: HashMap::new(),
                subscriptions: HashMap::new(),
            },
        );

        prune_stats_file(&mut stats, fixed_time());
        let remaining_samples = &stats.providers["openai"]
            .api_summary
            .as_ref()
            .unwrap()
            .samples;

        assert_eq!(remaining_samples.len(), 1);
        assert!(nearly_equal(remaining_samples[0].total_used, 2.0));
    }

    #[test]
    fn dedupe_skips_identical_samples_in_short_window() {
        let mut samples = vec![usage_sample(0, 10.0, "2026-04-01")];
        push_usage_sample(
            &mut samples,
            UsageValueSample {
                captured_at: (fixed_time() + Duration::minutes(4)).to_rfc3339(),
                total_used: 10.0,
                total_budget: Some(100.0),
                remaining: Some(90.0),
                period_start: Some("2026-04-01".to_string()),
                period_end: Some("2026-04-30".to_string()),
            },
        );

        assert_eq!(samples.len(), 1);
    }

    #[test]
    fn usage_range_delta_treats_counter_drop_as_reset() {
        let samples = vec![
            usage_sample(-3, 10.0, "2026-04-01"),
            usage_sample(-2, 18.0, "2026-04-01"),
            usage_sample(-1, 3.0, "2026-04-05"),
            usage_sample(0, 8.0, "2026-04-05"),
        ];

        let delta = compute_usage_range_delta(&samples, &StatsRange::Day, fixed_time());
        assert!(nearly_equal(delta, 13.0));
    }

    #[test]
    fn forecast_returns_unlikely_before_reset_when_eta_exceeds_reset() {
        let samples = vec![
            utilization_sample(-5, 20.0, "2026-04-05T13:00:00Z"),
            utilization_sample(-3, 25.0, "2026-04-05T13:00:00Z"),
            utilization_sample(-1, 30.0, "2026-04-05T13:00:00Z"),
        ];

        let latest = samples.last().unwrap();
        let recent_velocity = compute_utilization_velocity(&samples, fixed_time());
        let forecast = build_utilization_forecast(
            latest.utilization,
            recent_velocity,
            latest.resets_at.clone(),
            parse_timestamp(&latest.captured_at).unwrap(),
        );

        assert_eq!(forecast.status, UsageForecastStatus::UnlikelyBeforeReset);
    }

    #[test]
    fn api_forecast_requires_enough_samples() {
        let samples = vec![
            usage_sample(-1, 10.0, "2026-04-01"),
            usage_sample(0, 12.0, "2026-04-01"),
        ];

        assert!(compute_usage_velocity(&samples, fixed_time()).is_none());
    }

    #[test]
    fn sample_cap_drops_oldest_samples() {
        // 8 条样本时间递增（offset -8..-1），上限 5 → 保留最新 5 条、丢最旧 3 条
        let mut stats = UsageStatsFile::default();
        let samples: Vec<UsageValueSample> = (0..8)
            .map(|index| usage_sample(index - 8, index as f64, "2026-04-01"))
            .collect();
        stats.providers.insert(
            "openai".to_string(),
            ProviderStatsHistory {
                display_name: "OpenAI".to_string(),
                api_summary: Some(UsageSeriesHistory {
                    label: "api".to_string(),
                    currency: "USD".to_string(),
                    samples,
                }),
                api_keys: HashMap::new(),
                subscriptions: HashMap::new(),
            },
        );

        enforce_sample_cap(&mut stats, 5);
        let remaining = &stats.providers["openai"]
            .api_summary
            .as_ref()
            .unwrap()
            .samples;

        assert_eq!(remaining.len(), 5);
        assert!(nearly_equal(remaining[0].total_used, 3.0));
        assert!(nearly_equal(remaining[4].total_used, 7.0));

        // 未超限时不做任何裁剪
        enforce_sample_cap(&mut stats, 50);
        let remaining = &stats.providers["openai"]
            .api_summary
            .as_ref()
            .unwrap()
            .samples;
        assert_eq!(remaining.len(), 5);
    }

    fn test_summary(provider_id: &str, total_used: f64) -> UsageSummary {
        UsageSummary {
            provider_id: provider_id.to_string(),
            display_name: "OpenAI".to_string(),
            enabled: true,
            status: crate::providers::types::ProviderStatus::Success,
            api_key_usages: Vec::new(),
            usage: Some(crate::providers::types::UsageData {
                total_used,
                total_budget: Some(100.0),
                remaining: Some(100.0 - total_used),
                currency: "USD".to_string(),
                period_start: Some("2026-04-01".to_string()),
                period_end: Some("2026-04-30".to_string()),
                windows: Vec::new(),
                plan_name: None,
            }),
            subscriptions: Vec::new(),
            rate_limit: None,
            last_updated: None,
            error_message: None,
        }
    }

    fn temp_stats_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("peeka_stats_test_{}_{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 行为契约（阶段四 perf）：加载未就绪时 record 丢弃样本（不进内存不落盘），
    /// 读取路径等加载完成后返回磁盘数据；加载完成后 record 正常记录。
    #[tokio::test]
    async fn record_before_load_is_discarded_and_served_after_load() {
        let data_dir = temp_stats_dir("record_pending");
        let store = UsageStatsStore::new(data_dir.clone());

        // 加载未就绪：record 被丢弃
        store
            .record_summaries(&[test_summary("openai", 10.0)])
            .await
            .unwrap();
        store.load_from_disk().await; // 磁盘从未被 record 写过 → 空历史

        let json = store.get_snapshot_json().await.unwrap();
        assert!(
            json["providers"].as_object().unwrap().is_empty(),
            "加载完成前的样本应被丢弃而不是写进空历史"
        );

        // 加载完成后：record 正常进内存并落盘
        store
            .record_summaries(&[test_summary("openai", 10.0)])
            .await
            .unwrap();
        let json = store.get_snapshot_json().await.unwrap();
        assert!(
            json["providers"]["openai"].is_object(),
            "加载完成后的 record 必须正常记录"
        );

        let _ = std::fs::remove_dir_all(&data_dir);
    }

    /// 行为契约（阶段四 perf）：new() 不同步读盘（内存为空），读取路径在后台加载
    /// 完成前等待、完成后拿到磁盘数据（换入生效）。
    #[tokio::test]
    async fn snapshot_waits_for_pending_background_load() {
        let data_dir = temp_stats_dir("bg_load");
        let mut stats = UsageStatsFile::default();
        stats.providers.insert(
            "kimi".to_string(),
            ProviderStatsHistory {
                display_name: "Kimi".to_string(),
                api_summary: Some(UsageSeriesHistory {
                    label: "api".to_string(),
                    currency: "CNY".to_string(),
                    // 必须用当前时间附近的样本：180 天保留策略会把更旧的裁掉
                    samples: vec![UsageValueSample {
                        captured_at: Utc::now().to_rfc3339(),
                        total_used: 12.0,
                        total_budget: Some(100.0),
                        remaining: Some(88.0),
                        period_start: Some("2026-11-01".to_string()),
                        period_end: Some("2026-11-30".to_string()),
                    }],
                }),
                api_keys: HashMap::new(),
                subscriptions: HashMap::new(),
            },
        );
        let stats_path = data_dir.join("usage_stats.json");
        std::fs::write(&stats_path, serde_json::to_string(&stats).unwrap()).unwrap();

        let store = std::sync::Arc::new(UsageStatsStore::new(data_dir.clone()));
        // 模拟 start_background_load：加载在另一任务里异步完成
        let background = {
            let store = std::sync::Arc::clone(&store);
            tokio::spawn(async move {
                store.load_from_disk().await;
            })
        };

        // 不等 load_from_disk 直接读：get_snapshot_json 必须等待加载完成而非返回空数据
        let json = store.get_snapshot_json().await.unwrap();
        assert!(
            json["providers"]["kimi"].is_object(),
            "读取路径必须等后台加载完成后返回磁盘数据"
        );

        background.await.unwrap();
        let _ = std::fs::remove_dir_all(&data_dir);
    }
}
