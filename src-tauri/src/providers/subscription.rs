use reqwest::Client;
use serde::Deserialize;

use super::types::*;

/// 订阅用量查询器
pub struct SubscriptionFetcher {
    client: Client,
}

// ===== Anthropic 订阅 =====

#[derive(Debug, Deserialize)]
struct AnthropicOAuthUsageResponse {
    five_hour: Option<AnthropicUsageWindow>,
    seven_day: Option<AnthropicUsageWindow>,
    seven_day_sonnet: Option<AnthropicUsageWindow>,
    seven_day_opus: Option<AnthropicUsageWindow>,
    extra_usage: Option<AnthropicExtraUsage>,
}

#[derive(Debug, Deserialize)]
struct AnthropicUsageWindow {
    utilization: f64,
    resets_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AnthropicExtraUsage {
    is_enabled: bool,
    monthly_limit: Option<f64>,
    used_credits: Option<f64>,
    utilization: Option<f64>,
    /// 部分版本的 OAuth usage 响应会带 extra_usage 的重置时间；
    /// 不带时由调用方回退为本地按账期推算（见 fetch_anthropic_oauth）
    resets_at: Option<String>,
}

// ===== OpenAI 订阅 =====

#[derive(Debug, Deserialize)]
struct OpenAIWhamUsageResponse {
    plan_type: Option<String>,
    rate_limit: Option<OpenAIRateLimitInfo>,
}

#[derive(Debug, Deserialize)]
struct OpenAIRateLimitInfo {
    primary_window: Option<OpenAIUsageWindow>,
    secondary_window: Option<OpenAIUsageWindow>,
}

#[derive(Debug, Deserialize)]
struct OpenAIUsageWindow {
    used_percent: Option<f64>,
    limit_window_seconds: Option<u64>,
    reset_at: Option<u64>,
}

impl SubscriptionFetcher {
    pub fn new() -> Self {
        Self {
            // 必须带超时：网络挂起时避免刷新永久卡死
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .connect_timeout(std::time::Duration::from_secs(10))
                .build()
                .expect("无法创建 HTTP 客户端"),
        }
    }

    /// 统一订阅查询入口，按 provider 字符串分发
    ///
    /// provider 取值：内置模板 QueryType::Subscription 的 provider 字段，
    /// 如 "anthropic_oauth" / "openai_wham"。
    ///
    /// `account_id` 仅 openai_wham 使用：作为 `ChatGPT-Account-Id` header 发送，
    /// 用于多账号场景。来源通常是 `~/.codex/auth.json` 的 `tokens.account_id`，
    /// 由调用方通过 oauth_detect 解析后传入；None 时不附带该 header。
    ///
    /// `key_store` 仅 gemini 使用：缓存刷新后的 access_token（修复 M8），
    /// 其它 provider 传 None 即可。
    pub async fn fetch(
        &self,
        provider: &str,
        oauth_token: &str,
        account_id: Option<&str>,
        key_store: Option<&crate::config::encryption::KeyStore>,
    ) -> SubscriptionUsage {
        match provider {
            "anthropic_oauth" => self.fetch_anthropic_oauth(oauth_token).await,
            "openai_wham" => self.fetch_openai_wham(oauth_token, account_id).await,
            // Gemini：oauth_token 参数传入的是 ~/.gemini/oauth_creds.json 的完整 JSON
            // （含 access_token + refresh_token + expiry），由 gemini 模块解析并支持自动刷新。
            // account_id 参数对 Gemini 不适用（Gemini 不需要 ChatGPT-Account-Id）。
            // key_store 用于缓存刷新后的 token（修复 M8），避免每次轮询都打 token 端点。
            "gemini" => {
                super::gemini::fetch_gemini_quota(&self.client, oauth_token, key_store).await
            }
            _ => SubscriptionUsage {
                plan_name: None,
                windows: vec![],
                extra_usage: None,
                status: ProviderStatus::Error,
                error_message: Some(format!("不支持的订阅供应商: {}", provider)),
            },
        }
    }

    /// Anthropic OAuth 订阅查询（原 fetch_anthropic）
    async fn fetch_anthropic_oauth(&self, oauth_token: &str) -> SubscriptionUsage {
        let resp = self
            .client
            .get("https://api.anthropic.com/api/oauth/usage")
            .header("Authorization", format!("Bearer {}", oauth_token))
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .header("anthropic-beta", "oauth-2025-04-20")
            .send()
            .await;

        let resp = match resp {
            Ok(r) => r,
            Err(e) => {
                return SubscriptionUsage {
                    plan_name: None,
                    windows: vec![],
                    extra_usage: None,
                    status: ProviderStatus::Error,
                    error_message: Some(format!("请求失败: {}", e)),
                };
            }
        };

        if resp.status().as_u16() == 401 || resp.status().as_u16() == 403 {
            return SubscriptionUsage {
                plan_name: None,
                windows: vec![],
                extra_usage: None,
                status: ProviderStatus::Error,
                error_message: Some("OAuth Token 无效或已过期".into()),
            };
        }

        if resp.status().as_u16() == 429 {
            return SubscriptionUsage {
                plan_name: None,
                windows: vec![],
                extra_usage: None,
                status: ProviderStatus::Error,
                error_message: Some("请求过于频繁，请稍后再试".into()),
            };
        }

        if !resp.status().is_success() {
            return SubscriptionUsage {
                plan_name: None,
                windows: vec![],
                extra_usage: None,
                status: ProviderStatus::Error,
                error_message: Some(format!("HTTP {}", resp.status())),
            };
        }

        let data: AnthropicOAuthUsageResponse = match resp.json().await {
            Ok(d) => d,
            Err(e) => {
                return SubscriptionUsage {
                    plan_name: None,
                    windows: vec![],
                    extra_usage: None,
                    status: ProviderStatus::Error,
                    error_message: Some(format!("解析失败: {}", e)),
                };
            }
        };

        let mut windows = Vec::new();

        if let Some(w) = data.five_hour {
            windows.push(SubscriptionWindow {
                label: window_labels::FIVE_HOUR.into(),
                utilization: w.utilization,
                resets_at: w.resets_at,
            });
        }

        if let Some(w) = data.seven_day {
            windows.push(SubscriptionWindow {
                label: window_labels::SEVEN_DAY.into(),
                utilization: w.utilization,
                resets_at: w.resets_at,
            });
        }

        if let Some(w) = data.seven_day_sonnet {
            windows.push(SubscriptionWindow {
                label: window_labels::SEVEN_DAY_SONNET.into(),
                utilization: w.utilization,
                resets_at: w.resets_at,
            });
        }

        if let Some(w) = data.seven_day_opus {
            windows.push(SubscriptionWindow {
                label: window_labels::SEVEN_DAY_OPUS.into(),
                utilization: w.utilization,
                resets_at: w.resets_at,
            });
        }

        let extra_usage = data.extra_usage.map(|e| {
            let monthly_limit_usd = e.monthly_limit.map(|c| (c / 100.0 * 100.0).round() / 100.0);
            let used_usd = e.used_credits.map(|c| (c / 100.0 * 100.0).round() / 100.0);
            let utilization = e.utilization.map(|u| u * 100.0);
            // 修复 L17：优先使用 API 返回的 resets_at；响应不带该字段时回退本地推算
            // （extra usage 按自然月账期重置，取下月 1 号 0 点 UTC）。
            // 该回退是账期语义的推算值而非 API 实测值，但语义诚实：
            // extra usage 确实按月重置，不会误导前端。
            let resets_at = e
                .resets_at
                .filter(|s| !s.is_empty())
                .or_else(|| Some(next_month_iso()));
            ExtraUsage {
                is_enabled: e.is_enabled,
                monthly_limit_usd,
                used_usd,
                utilization,
                resets_at,
            }
        });

        SubscriptionUsage {
            plan_name: Some("Claude Pro/Max".into()),
            windows,
            extra_usage,
            status: ProviderStatus::Success,
            error_message: None,
        }
    }

    /// OpenAI Wham 订阅查询（原 fetch_openai，ChatGPT Plus/Pro/Team）
    ///
    /// `account_id` 用于多账号场景：作为 `ChatGPT-Account-Id` header 发送。
    /// 当 `account_id` 为 None 时，会尝试从 `~/.codex/auth.json` 的
    /// `tokens.account_id` 自动读取（与 OAuth 自动检测复用同一文件）。
    /// 读取失败则不附带该 header（单账号场景后端通常可正常返回）。
    async fn fetch_openai_wham(
        &self,
        oauth_token: &str,
        account_id: Option<&str>,
    ) -> SubscriptionUsage {
        // 调用方未传 account_id 时，回退到自动检测
        let resolved_account_id = match account_id {
            Some(aid) => Some(aid.to_string()),
            None => super::oauth_detect::detect_openai()
                .and_then(|d| d.account_id)
                .filter(|s| !s.is_empty()),
        };

        let mut req = self
            .client
            .get("https://chatgpt.com/backend-api/wham/usage")
            .header("Authorization", format!("Bearer {}", oauth_token))
            .header("User-Agent", "codex-cli");

        if let Some(aid) = resolved_account_id.as_deref() {
            req = req.header("ChatGPT-Account-Id", aid);
        }

        let resp = req.send().await;

        let resp = match resp {
            Ok(r) => r,
            Err(e) => {
                return SubscriptionUsage {
                    plan_name: None,
                    windows: vec![],
                    extra_usage: None,
                    status: ProviderStatus::Error,
                    error_message: Some(format!("请求失败: {}", e)),
                };
            }
        };

        if resp.status().as_u16() == 401 || resp.status().as_u16() == 403 {
            return SubscriptionUsage {
                plan_name: None,
                windows: vec![],
                extra_usage: None,
                status: ProviderStatus::Error,
                error_message: Some("OAuth Token 无效或已过期".into()),
            };
        }

        if !resp.status().is_success() {
            return SubscriptionUsage {
                plan_name: None,
                windows: vec![],
                extra_usage: None,
                status: ProviderStatus::Error,
                error_message: Some(format!("HTTP {}", resp.status())),
            };
        }

        let data: OpenAIWhamUsageResponse = match resp.json().await {
            Ok(d) => d,
            Err(e) => {
                return SubscriptionUsage {
                    plan_name: None,
                    windows: vec![],
                    extra_usage: None,
                    status: ProviderStatus::Error,
                    error_message: Some(format!("解析失败: {}", e)),
                };
            }
        };

        let plan_name = data.plan_type.map(|p| match p.as_str() {
            "plus" => "ChatGPT Plus".into(),
            "pro" => "ChatGPT Pro".into(),
            "team" => "ChatGPT Team".into(),
            "enterprise" => "ChatGPT Enterprise".into(),
            other => other.to_string(),
        });

        let mut windows = Vec::new();

        if let Some(ref rl) = data.rate_limit {
            if let Some(ref w) = rl.primary_window {
                // 修复 L7：label 用机器常量（前端经 windowLabels 映射各语言文案），
                // 不再硬编码中文「5小时」「7天」「主窗口」
                let label = openai_window_label(w.limit_window_seconds, window_labels::PRIMARY);
                windows.push(SubscriptionWindow {
                    label,
                    utilization: w.used_percent.unwrap_or(0.0),
                    resets_at: w.reset_at.map(|ts| {
                        chrono::DateTime::from_timestamp(ts as i64, 0)
                            .map(|dt| dt.to_rfc3339())
                            .unwrap_or_default()
                    }),
                });
            }
            if let Some(ref w) = rl.secondary_window {
                let label = openai_window_label(w.limit_window_seconds, window_labels::SECONDARY);
                windows.push(SubscriptionWindow {
                    label,
                    utilization: w.used_percent.unwrap_or(0.0),
                    resets_at: w.reset_at.map(|ts| {
                        chrono::DateTime::from_timestamp(ts as i64, 0)
                            .map(|dt| dt.to_rfc3339())
                            .unwrap_or_default()
                    }),
                });
            }
        }

        SubscriptionUsage {
            plan_name,
            windows,
            extra_usage: None,
            status: ProviderStatus::Success,
            error_message: None,
        }
    }
}

/// 计算下个月月初的 ISO 8601 字符串（extra_usage 的重置时间）
fn next_month_iso() -> String {
    use chrono::{Datelike, TimeZone, Utc};
    let now = Utc::now();
    let (year, month) = if now.month() == 12 {
        (now.year() + 1, 1)
    } else {
        (now.year(), now.month() + 1)
    };
    Utc.with_ymd_and_hms(year, month, 1, 0, 0, 0)
        .single()
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_default()
}

/// OpenAI wham 订阅窗口的机器常量标签（修复 L7）
///
/// ChatGPT 后端的 limit_window_seconds：主窗口通常 18000（5 小时）、
/// 次窗口通常 604800（7 天）。能按时长归类就用 five_hour / seven_day；
/// 无法归类（新版窗口时长）时用调用方给的兜底常量（primary / secondary），
/// 前端通过 windowLabels 映射成各语言文案。
fn openai_window_label(limit_window_seconds: Option<u64>, fallback: &str) -> String {
    const FIVE_HOUR_SECONDS: u64 = 5 * 3600;
    const SEVEN_DAY_SECONDS: u64 = 7 * 86400;
    match limit_window_seconds {
        Some(FIVE_HOUR_SECONDS) => window_labels::FIVE_HOUR.to_string(),
        Some(SEVEN_DAY_SECONDS) => window_labels::SEVEN_DAY.to_string(),
        _ => fallback.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_openai_window_label_known_durations() {
        assert_eq!(
            openai_window_label(Some(18000), window_labels::PRIMARY),
            "five_hour"
        );
        assert_eq!(
            openai_window_label(Some(604800), window_labels::SECONDARY),
            "seven_day"
        );
    }

    #[test]
    fn test_openai_window_label_fallback() {
        // 无法按时长归类时用兜底常量；缺失时长同样兜底
        assert_eq!(
            openai_window_label(Some(3600), window_labels::PRIMARY),
            "primary"
        );
        assert_eq!(
            openai_window_label(None, window_labels::SECONDARY),
            "secondary"
        );
    }
}
