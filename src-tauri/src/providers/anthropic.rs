use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;

use super::traits::{ProviderError, UsageProvider};
use super::types::*;

pub struct AnthropicProvider {
    client: Client,
}

impl AnthropicProvider {
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

/// 花费报告响应
#[derive(Debug, Deserialize)]
struct CostReportResponse {
    data: Vec<CostEntry>,
}

#[derive(Debug, Deserialize)]
struct CostEntry {
    #[serde(default)]
    cost_cents: f64,
}

#[async_trait]
impl UsageProvider for AnthropicProvider {
    fn id(&self) -> String {
        "anthropic".to_string()
    }

    fn display_name(&self) -> &str {
        "Anthropic"
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            has_balance: false,
            has_usage: true,
            has_rate_limit: true,
            has_subscription: true,
        }
    }

    async fn fetch_usage(&self, api_key: &str) -> Result<UsageData, ProviderError> {
        let now = chrono::Utc::now();
        let start_date = now.format("%Y-%m-01").to_string();
        let end_date = now.format("%Y-%m-%d").to_string();

        let url = format!(
            "https://api.anthropic.com/v1/organizations/cost_report?start_date={}&end_date={}",
            start_date, end_date
        );

        let resp = self
            .client
            .get(&url)
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01")
            .send()
            .await?;

        if resp.status().as_u16() == 401 || resp.status().as_u16() == 403 {
            return Err(ProviderError::AuthError(
                "Admin API Key 无效或权限不足（需要 sk-ant-admin... 格式）".into(),
            ));
        }

        if !resp.status().is_success() {
            return Err(ProviderError::RequestError(format!(
                "HTTP {}",
                resp.status()
            )));
        }

        let report: CostReportResponse = resp
            .json()
            .await
            .map_err(|e| ProviderError::ParseError(e.to_string()))?;

        let total_used: f64 = report.data.iter().map(|e| e.cost_cents).sum::<f64>() / 100.0;

        Ok(UsageData {
            total_used,
            total_budget: None,
            remaining: None,
            currency: "USD".to_string(),
            period_start: Some(start_date),
            period_end: Some(end_date),
            windows: Vec::new(),
        })
    }

    async fn fetch_rate_limits(
        &self,
        _api_key: &str,
    ) -> Result<Option<RateLimitData>, ProviderError> {
        // 修复 M7：不再为展示 rate limit badge 主动 POST /v1/messages 探测。
        //
        // 背景：之前每次刷新（默认 5 分钟轮询 ≈ 288 次/天/key）都会额外发一个
        // max_tokens=1 的真实 Messages 请求，只为读取响应头里的
        // `anthropic-ratelimit-*` 系列字段——这是一次真实计费调用，
        // 既产生费用又污染用量统计。
        //
        // 为什么不改成从上面的 cost_report GET 响应头里解析：
        // `anthropic-ratelimit-*` 头是 Anthropic 官方文档为 Messages API
        // 定义的响应头（见 https://docs.claude.com/en/api/rate-limits），
        // cost_report 属于 Admin API，官方并未承诺返回 Messages 的限流头；
        // 即使返回，语义也是 Admin API 自身的限流，与 badge 想展示的
        // Messages API 限流不符，展示出来会误导用户。
        //
        // 结论：不为展示 badge 付出计费代价，rate limit 直接返回 None，
        // 前端拿不到数据时自然不渲染 badge。原先 Admin key 401/403 被
        // 调用方 `.ok()` 静默吞掉的问题也随探测移除而消失。
        Ok(None)
    }

    async fn validate_key(&self, api_key: &str) -> Result<bool, ProviderError> {
        // 使用 cost_report 端点验证 admin key
        let resp = self
            .client
            .get("https://api.anthropic.com/v1/organizations/cost_report?start_date=2024-01-01&end_date=2024-01-02")
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01")
            .send()
            .await?;

        Ok(resp.status().is_success())
    }
}
