use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;

use super::traits::{ProviderError, UsageProvider};
use super::types::*;

pub struct OpenRouterProvider {
    client: Client,
}

impl OpenRouterProvider {
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
}

/// /api/v1/credits 响应
#[derive(Debug, Deserialize)]
struct CreditsResponse {
    data: CreditsData,
}

#[derive(Debug, Deserialize)]
struct CreditsData {
    total_credits: f64,
    total_usage: f64,
}

/// /api/v1/key 响应
#[derive(Debug, Deserialize)]
struct KeyInfoResponse {
    data: KeyInfoData,
}

#[derive(Debug, Deserialize)]
struct KeyInfoData {
    usage: f64,
    limit: Option<f64>,
    rate_limit: Option<KeyRateLimit>,
}

#[derive(Debug, Deserialize)]
struct KeyRateLimit {
    /// 每个限流窗口允许的请求次数（窗口长度见 interval）
    requests: Option<u64>,
    /// 限流窗口，如 "10s"（语义：每 interval 最多 requests 次）
    interval: Option<String>,
}

/// 把 OpenRouter rate_limit 的 interval 字符串（如 "10s" / "1m" / "1h"）解析为秒数。
/// 缺省或解析失败按 60 秒处理（即把 requests 理解为"每分钟 N 次"）。
fn rate_limit_interval_seconds(interval: Option<&str>) -> u64 {
    let Some(raw) = interval else {
        return 60;
    };
    let trimmed = raw.trim();
    let split_at = trimmed
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(trimmed.len());
    let (digits, unit) = trimmed.split_at(split_at);
    let value: u64 = digits.parse().unwrap_or(60);
    let seconds = match unit {
        "s" => value,
        "m" => value.saturating_mul(60),
        "h" => value.saturating_mul(3600),
        "d" => value.saturating_mul(86400),
        _ => 60,
    };
    seconds.max(1)
}

#[async_trait]
impl UsageProvider for OpenRouterProvider {
    fn id(&self) -> String {
        "openrouter".to_string()
    }

    fn display_name(&self) -> &str {
        "OpenRouter"
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            has_balance: true,
            has_usage: true,
            has_rate_limit: true,
            has_subscription: false,
        }
    }

    async fn fetch_usage(&self, api_key: &str) -> Result<UsageData, ProviderError> {
        // 获取 credits 信息
        let credits_resp = self
            .client
            .get("https://openrouter.ai/api/v1/credits")
            .bearer_auth(api_key)
            .send()
            .await?;

        if credits_resp.status().as_u16() == 401 {
            return Err(ProviderError::AuthError("API Key 无效".into()));
        }

        if credits_resp.status().is_success() {
            let credits: CreditsResponse = credits_resp
                .json()
                .await
                .map_err(|e| ProviderError::ParseError(e.to_string()))?;

            return Ok(UsageData {
                total_used: credits.data.total_usage,
                total_budget: Some(credits.data.total_credits),
                remaining: Some((credits.data.total_credits - credits.data.total_usage).max(0.0)),
                currency: "USD".to_string(),
                period_start: None,
                period_end: None,
                windows: Vec::new(),
            });
        }

        // 回退到 /api/v1/key
        let key_resp = self
            .client
            .get("https://openrouter.ai/api/v1/key")
            .bearer_auth(api_key)
            .send()
            .await?;

        if !key_resp.status().is_success() {
            return Err(ProviderError::RequestError(format!(
                "HTTP {}",
                key_resp.status()
            )));
        }

        let key_info: KeyInfoResponse = key_resp
            .json()
            .await
            .map_err(|e| ProviderError::ParseError(e.to_string()))?;

        Ok(UsageData {
            total_used: key_info.data.usage,
            total_budget: key_info.data.limit,
            remaining: key_info
                .data
                .limit
                .map(|l| (l - key_info.data.usage).max(0.0)),
            currency: "USD".to_string(),
            period_start: None,
            period_end: None,
            windows: Vec::new(),
        })
    }

    async fn fetch_rate_limits(
        &self,
        api_key: &str,
    ) -> Result<Option<RateLimitData>, ProviderError> {
        let resp = self
            .client
            .get("https://openrouter.ai/api/v1/key")
            .bearer_auth(api_key)
            .send()
            .await?;

        if !resp.status().is_success() {
            return Ok(None);
        }

        let key_info: KeyInfoResponse = resp
            .json()
            .await
            .map_err(|e| ProviderError::ParseError(e.to_string()))?;

        if let Some(rl) = key_info.data.rate_limit {
            // 修复 L5：OpenRouter 的 rate_limit 语义是"每个 interval 窗口最多 requests 次"
            // （如 {requests: 200, interval: "10s"}），没有"当前已用/剩余"字段。
            // 原实现把 remaining 和 limit 都填 requests，徽章恒显示无信息的 "RPM: N/N"。
            // 现换算为每分钟上限填入 limit，当前速率留 None（API 不提供，不编造）。
            let requests_per_minute_limit = rl.requests.map(|requests| {
                let window_seconds = rate_limit_interval_seconds(rl.interval.as_deref());
                // 向上取整：(requests * 60 + window - 1) / window
                requests
                    .saturating_mul(60)
                    .saturating_add(window_seconds - 1)
                    / window_seconds
            });
            Ok(Some(RateLimitData {
                requests_per_minute: None,
                requests_per_minute_limit,
                tokens_per_minute: None,
                tokens_per_minute_limit: None,
            }))
        } else {
            Ok(None)
        }
    }

    async fn validate_key(&self, api_key: &str) -> Result<bool, ProviderError> {
        let resp = self
            .client
            .get("https://openrouter.ai/api/v1/key")
            .bearer_auth(api_key)
            .send()
            .await?;

        Ok(resp.status().is_success())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rate_limit_interval_seconds() {
        assert_eq!(rate_limit_interval_seconds(Some("10s")), 10);
        assert_eq!(rate_limit_interval_seconds(Some("1m")), 60);
        assert_eq!(rate_limit_interval_seconds(Some("2h")), 7200);
        assert_eq!(rate_limit_interval_seconds(Some("1d")), 86400);
        // 缺省 / 非法值按每分钟理解；0 窗口钳制为 1 秒防除零
        assert_eq!(rate_limit_interval_seconds(None), 60);
        assert_eq!(rate_limit_interval_seconds(Some("garbage")), 60);
        assert_eq!(rate_limit_interval_seconds(Some("0s")), 1);
    }
}
