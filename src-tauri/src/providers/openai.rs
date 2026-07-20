use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;

use super::traits::{ProviderError, UsageProvider};
use super::types::*;

pub struct OpenAIProvider {
    client: Client,
}

impl OpenAIProvider {
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

    /// 调用 /v1/organization/costs 累计指定时间范围内的费用总额（单位：美元）。
    ///
    /// 修复 M6 单位问题：`amount.value` 是**美元小数**而不是美分，不再除以 100。
    /// 依据：OpenAI 官方 OpenAPI 规范（github.com/openai/openai-openapi）中
    /// `CostsResult.amount` 的描述为 "The monetary value in its associated
    /// currency"（currency 为小写 ISO-4217，如 "usd"），且官方示例为
    /// `"amount": {"value": 0.06, "currency": "usd"}`，即 0.06 美元。
    ///
    /// 修复 M6 分页问题：响应按 bucket 分页（`has_more` + `next_page` 游标，
    /// 游标通过 `page` 查询参数回传）。旧代码只取第一页且未传 limit
    /// （默认仅 7 个 bucket），月初第 8 天起的费用就会被漏计。
    /// 这里显式传 `limit=180`（官方上限）并沿游标翻页累计，
    /// 页数封顶 [`COSTS_MAX_PAGES`] 防死循环。
    async fn fetch_monthly_costs(
        &self,
        api_key: &str,
        start_timestamp: i64,
        end_timestamp: i64,
    ) -> Result<f64, ProviderError> {
        let mut total_used = 0.0;
        let mut page_cursor: Option<String> = None;

        for page_index in 0..COSTS_MAX_PAGES {
            let mut request = self
                .client
                .get("https://api.openai.com/v1/organization/costs")
                .bearer_auth(api_key)
                .query(&[
                    ("start_time", start_timestamp.to_string()),
                    ("end_time", end_timestamp.to_string()),
                    ("group_by", "line_item".to_string()),
                    ("limit", COSTS_PAGE_LIMIT.to_string()),
                ]);
            if let Some(cursor) = page_cursor.as_deref() {
                // reqwest 的 .query() 会对游标做 percent-encoding
                request = request.query(&[("page", cursor)]);
            }

            let resp = request.send().await?;

            if resp.status().as_u16() == 401 || resp.status().as_u16() == 403 {
                return Err(ProviderError::AuthError("API Key 无效或权限不足".into()));
            }

            if !resp.status().is_success() {
                if page_index == 0 {
                    // 保持原有语义：首页非 2xx（除鉴权外）按 0 处理，
                    // 让后付费账户继续走 subscription 限额展示
                    return Ok(0.0);
                }
                // 后续页失败不能静默漏计（那正是 M6 要修的问题），
                // 返回错误让前端展示异常，而不是一个偏低的金额
                return Err(ProviderError::RequestError(format!(
                    "HTTP {}",
                    resp.status()
                )));
            }

            let costs: CostsResponse = resp
                .json()
                .await
                .map_err(|e| ProviderError::ParseError(e.to_string()))?;

            total_used += costs
                .data
                .iter()
                .flat_map(|bucket| bucket.results.iter())
                .map(|result| result.amount.value)
                .sum::<f64>();

            if costs.has_more {
                if let Some(next) = costs.next_page.filter(|next| !next.is_empty()) {
                    page_cursor = Some(next);
                    continue;
                }
            }
            break;
        }

        Ok(total_used)
    }
}

/// /v1/organization/costs 响应
#[derive(Debug, Deserialize)]
struct CostsResponse {
    #[serde(default)]
    data: Vec<CostBucket>,
    /// 是否还有下一页 bucket（分页字段，修复 M6）
    #[serde(default)]
    has_more: bool,
    /// 下一页游标，作为 `page` 查询参数回传（分页字段，修复 M6）
    #[serde(default)]
    next_page: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CostBucket {
    results: Vec<CostResult>,
}

#[derive(Debug, Deserialize)]
struct CostResult {
    amount: CostAmount,
}

#[derive(Debug, Deserialize)]
struct CostAmount {
    value: f64,
}

/// costs API 单页 bucket 数上限（官方允许 1-180，默认 7；
/// 不显式传 limit 时月初第 8 天起就会漏计，修复 M6）
const COSTS_PAGE_LIMIT: u32 = 180;
/// costs API 翻页次数上限，防 next_page 游标异常导致死循环
const COSTS_MAX_PAGES: u32 = 20;

/// /v1/dashboard/billing/subscription 响应
#[derive(Debug, Deserialize)]
struct SubscriptionResponse {
    hard_limit_usd: Option<f64>,
}

/// /v1/dashboard/billing/credit_grants 响应
#[derive(Debug, Deserialize)]
struct CreditGrantsResponse {
    total_granted: Option<f64>,
    total_used: Option<f64>,
    total_available: Option<f64>,
}

#[async_trait]
impl UsageProvider for OpenAIProvider {
    fn id(&self) -> String {
        "openai".to_string()
    }

    fn display_name(&self) -> &str {
        "OpenAI"
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            has_balance: true,
            has_usage: true,
            has_rate_limit: false,
            has_subscription: true,
        }
    }

    async fn fetch_usage(&self, api_key: &str) -> Result<UsageData, ProviderError> {
        // 先尝试获取 credit grants（预付费账户）
        let credits_result = self
            .client
            .get("https://api.openai.com/v1/dashboard/billing/credit_grants")
            .bearer_auth(api_key)
            .send()
            .await;

        if let Ok(resp) = credits_result {
            if resp.status().is_success() {
                if let Ok(credits) = resp.json::<CreditGrantsResponse>().await {
                    if let (Some(granted), Some(used), Some(available)) = (
                        credits.total_granted,
                        credits.total_used,
                        credits.total_available,
                    ) {
                        if granted > 0.0 {
                            return Ok(UsageData {
                                total_used: used,
                                total_budget: Some(granted),
                                remaining: Some(available),
                                currency: "USD".to_string(),
                                period_start: None,
                                period_end: None,
                                windows: Vec::new(),
                            });
                        }
                    }
                }
            }
        }

        // 后付费账户：获取本月用量
        let now = chrono::Utc::now();
        let start_of_month = now.format("%Y-%m-01").to_string();
        let end_date = now.format("%Y-%m-%d").to_string();

        let start_timestamp = chrono::NaiveDate::parse_from_str(&start_of_month, "%Y-%m-%d")
            .map(|d| d.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp())
            .unwrap_or(0);
        let end_timestamp = now.timestamp();

        // 获取用量（costs API，含分页累计，修复 M6）
        let total_used = self
            .fetch_monthly_costs(api_key, start_timestamp, end_timestamp)
            .await?;

        // 尝试获取 subscription 限额
        let budget = match self
            .client
            .get("https://api.openai.com/v1/dashboard/billing/subscription")
            .bearer_auth(api_key)
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => resp
                .json::<SubscriptionResponse>()
                .await
                .ok()
                .and_then(|s| s.hard_limit_usd),
            _ => None,
        };

        Ok(UsageData {
            total_used,
            total_budget: budget,
            remaining: budget.map(|b| (b - total_used).max(0.0)),
            currency: "USD".to_string(),
            period_start: Some(start_of_month),
            period_end: Some(end_date),
            windows: Vec::new(),
        })
    }

    async fn fetch_rate_limits(
        &self,
        _api_key: &str,
    ) -> Result<Option<RateLimitData>, ProviderError> {
        // OpenAI 速率限制在响应 header 中返回，需要实际请求才能获取
        Ok(None)
    }

    async fn validate_key(&self, api_key: &str) -> Result<bool, ProviderError> {
        let resp = self
            .client
            .get("https://api.openai.com/v1/models")
            .bearer_auth(api_key)
            .send()
            .await?;

        Ok(resp.status().is_success())
    }
}
