use reqwest::Client;
use serde_json::Value;

use super::traits::ProviderError;
use super::types::UsageData;

/// 执行 CodingPlan 查询
///
/// CodingPlan 类供应商返回百分比型多窗口用量（如 Kimi 的 5 小时/周限额）。
/// 统一转成 UsageData（currency="%"，total_budget=100，total_used=最高窗口 utilization）。
///
/// 注意：CodingPlan 本质是百分比型，和 Subscription 类似但用 API Key 查询。
/// 当前返回 UsageData（单值），多窗口信息通过 total_used 取最高 utilization。
/// 完整多窗口支持需要扩展 UsageData 结构（阶段 2.5），当前先保证能查到数据。
///
/// 支持的 provider：kimi / glm / minimax
pub async fn execute_coding_plan_query(
    client: &Client,
    provider: &str,
    api_key: &str,
) -> Result<UsageData, ProviderError> {
    match provider {
        "kimi" => fetch_kimi(client, api_key).await,
        "glm" => fetch_glm(client, api_key).await,
        "minimax" => fetch_minimax(client, api_key).await,
        _ => Err(ProviderError::RequestError(format!(
            "不支持的 CodingPlan 供应商: {}",
            provider
        ))),
    }
}

/// 把 utilization (0-100) 组装成百分比型 UsageData
///
/// 取所有窗口中最高的 utilization 作为 total_used，便于前端进度条展示。
fn build_percent_usage(utilizations: Vec<f64>) -> UsageData {
    let total_used = utilizations
        .iter()
        .cloned()
        .fold(0.0_f64, f64::max)
        .clamp(0.0, 100.0);
    UsageData {
        total_used,
        total_budget: Some(100.0),
        remaining: Some((100.0 - total_used).clamp(0.0, 100.0)),
        currency: "%".to_string(),
        period_start: None,
        period_end: None,
    }
}

/// 统一的 HTTP 状态码检查（抄 balance.rs）
fn check_status(status: reqwest::StatusCode) -> Result<(), ProviderError> {
    let code = status.as_u16();
    if code == 401 || code == 403 {
        return Err(ProviderError::AuthError(format!(
            "认证失败 (HTTP {})",
            code
        )));
    }
    if code == 429 {
        return Err(ProviderError::RateLimited("请求过于频繁".to_string()));
    }
    if !status.is_success() {
        return Err(ProviderError::RequestError(format!("HTTP {}", status)));
    }
    Ok(())
}

/// bytes-then-parse 模式（抄 balance.rs，区分读体错和解析错）
///
/// RequestBuilder 由调用方通过 `client.get(url)` 构造（已绑定 client），
/// 这里只负责发送 + 状态检查 + 解析。
async fn fetch_json(req: reqwest::RequestBuilder) -> Result<Value, ProviderError> {
    let resp = req.send().await.map_err(|e| {
        if let Some(status) = e.status() {
            let code = status.as_u16();
            if code == 401 || code == 403 {
                return ProviderError::AuthError(format!("认证失败 (HTTP {})", code));
            }
            if code == 429 {
                return ProviderError::RateLimited("请求过于频繁".to_string());
            }
        }
        ProviderError::RequestError(e.to_string())
    })?;

    check_status(resp.status())?;

    let body_bytes = resp
        .bytes()
        .await
        .map_err(|e| ProviderError::RequestError(format!("读取响应体失败: {}", e)))?;

    let json: Value = serde_json::from_slice(&body_bytes)
        .map_err(|e| ProviderError::ParseError(format!("解析 JSON 失败: {}", e)))?;

    Ok(json)
}

// ===== Kimi（月之暗面）=====
//
// GET https://api.kimi.com/coding/v1/usages
// Authorization: Bearer {key}
//
// 响应：
// {
//   "limits": [
//     { "detail": { "limit": 100, "remaining": 30, "resetTime": "2026-07-17T10:00:00Z" } }
//   ],
//   "usage": { "limit": 1000, "remaining": 800, "resetTime": "2026-07-20T00:00:00Z" }
// }
//
// 映射：
// - limits[0].detail -> five_hour 窗口（utilization = (limit-remaining)/limit*100）
// - usage -> weekly_limit 窗口（utilization = (limit-remaining)/limit*100）
async fn fetch_kimi(client: &Client, api_key: &str) -> Result<UsageData, ProviderError> {
    let req = client
        .get("https://api.kimi.com/coding/v1/usages")
        .bearer_auth(api_key)
        .header("Accept", "application/json");

    let json = fetch_json(req).await?;

    let mut utilizations: Vec<f64> = Vec::new();

    // limits[0].detail -> five_hour
    if let Some(detail) = json
        .get("limits")
        .and_then(|v| v.get(0))
        .and_then(|v| v.get("detail"))
    {
        if let Some(u) = utilization_from_limit_remaining(detail) {
            utilizations.push(u);
        }
    }

    // usage -> weekly_limit
    if let Some(usage) = json.get("usage") {
        if let Some(u) = utilization_from_limit_remaining(usage) {
            utilizations.push(u);
        }
    }

    if utilizations.is_empty() {
        return Err(ProviderError::ParseError(
            "Kimi 响应中未找到有效的 limits[0].detail 或 usage 字段".into(),
        ));
    }

    Ok(build_percent_usage(utilizations))
}

/// 从 { limit, remaining } 对象计算 utilization 百分比 (0-100)
///
/// utilization = (limit - remaining) / limit * 100
/// 若 limit 为 0 或字段缺失，返回 None。
fn utilization_from_limit_remaining(obj: &Value) -> Option<f64> {
    let limit = obj.get("limit").and_then(|v| v.as_f64())?;
    let remaining = obj.get("remaining").and_then(|v| v.as_f64())?;
    if limit <= 0.0 {
        return None;
    }
    let used = (limit - remaining).max(0.0);
    Some((used / limit * 100.0).clamp(0.0, 100.0))
}

// ===== GLM（智谱，个人版）=====
//
// GET https://open.bigmodel.cn/api/monitor/usage/quota/limit
// Authorization: {key}      （裸 key，无 Bearer 前缀！）
// Accept-Language: en-US,en
//
// 响应：
// {
//   "data": {
//     "limits": [
//       { "type": "TOKENS_LIMIT", "unit": 3, "percentage": 80 },
//       { "type": "TOKENS_LIMIT", "unit": 6, "percentage": 50 }
//     ],
//     "level": "VIP"
//   }
// }
//
// 映射：
// - unit==3 -> five_hour（utilization = percentage）
// - unit==6 -> weekly_limit（utilization = percentage）
async fn fetch_glm(client: &Client, api_key: &str) -> Result<UsageData, ProviderError> {
    let req = client
        .get("https://open.bigmodel.cn/api/monitor/usage/quota/limit")
        // GLM 用裸 key（无 Bearer 前缀）
        .header("Authorization", api_key)
        .header("Accept", "application/json")
        .header("Accept-Language", "en-US,en");

    let json = fetch_json(req).await?;

    let mut utilizations: Vec<f64> = Vec::new();

    // data.limits[] 中找 unit==3 和 unit==6
    if let Some(limits) = json
        .get("data")
        .and_then(|v| v.get("limits"))
        .and_then(|v| v.as_array())
    {
        for item in limits {
            let unit = item.get("unit").and_then(|v| v.as_u64());
            let percentage = item.get("percentage").and_then(|v| v.as_f64());
            if let (Some(u), Some(p)) = (unit, percentage) {
                // unit==3 -> five_hour, unit==6 -> weekly_limit
                if u == 3 || u == 6 {
                    utilizations.push(p.clamp(0.0, 100.0));
                }
            }
        }
    }

    if utilizations.is_empty() {
        return Err(ProviderError::ParseError(
            "GLM 响应中未找到 unit==3 或 unit==6 的 limits 条目".into(),
        ));
    }

    Ok(build_percent_usage(utilizations))
}

// ===== MiniMax =====
//
// GET https://api.minimaxi.com/v1/api/openplatform/coding_plan/remains
// Authorization: Bearer {key}
//
// 响应：
// {
//   "model_remains": [
//     {
//       "model_name": "general",
//       "current_interval_remaining_percent": 70,
//       "current_weekly_status": 1,
//       "current_weekly_remaining_percent": 60
//     }
//   ]
// }
//
// 映射（取 model_name=="general" 的条目）：
// - current_interval_remaining_percent -> five_hour（utilization = 100 - remain）
// - current_weekly_status==1 时 current_weekly_remaining_percent -> weekly_limit
//   （utilization = 100 - remain）
async fn fetch_minimax(client: &Client, api_key: &str) -> Result<UsageData, ProviderError> {
    let req = client
        .get("https://api.minimaxi.com/v1/api/openplatform/coding_plan/remains")
        .bearer_auth(api_key)
        .header("Accept", "application/json");

    let json = fetch_json(req).await?;

    let mut utilizations: Vec<f64> = Vec::new();

    // model_remains[] 中找 model_name=="general"
    if let Some(arr) = json.get("model_remains").and_then(|v| v.as_array()) {
        for item in arr {
            let is_general = item
                .get("model_name")
                .and_then(|v| v.as_str())
                .map(|s| s == "general")
                .unwrap_or(false);
            if !is_general {
                continue;
            }

            // five_hour: utilization = 100 - current_interval_remaining_percent
            if let Some(remain) = item
                .get("current_interval_remaining_percent")
                .and_then(|v| v.as_f64())
            {
                utilizations.push((100.0 - remain).clamp(0.0, 100.0));
            }

            // weekly_limit: 仅当 current_weekly_status==1 时取
            let weekly_status = item.get("current_weekly_status").and_then(|v| v.as_u64());
            if weekly_status == Some(1) {
                if let Some(remain) = item
                    .get("current_weekly_remaining_percent")
                    .and_then(|v| v.as_f64())
                {
                    utilizations.push((100.0 - remain).clamp(0.0, 100.0));
                }
            }
        }
    }

    if utilizations.is_empty() {
        return Err(ProviderError::ParseError(
            "MiniMax 响应中未找到 model_name==\"general\" 的有效用量条目".into(),
        ));
    }

    Ok(build_percent_usage(utilizations))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_utilization_from_limit_remaining() {
        let v = serde_json::json!({ "limit": 100, "remaining": 30 });
        assert!((utilization_from_limit_remaining(&v).unwrap() - 70.0).abs() < 1e-6);

        let v = serde_json::json!({ "limit": 1000, "remaining": 800 });
        assert!((utilization_from_limit_remaining(&v).unwrap() - 20.0).abs() < 1e-6);

        // limit=0 -> None
        let v = serde_json::json!({ "limit": 0, "remaining": 0 });
        assert!(utilization_from_limit_remaining(&v).is_none());

        // remaining > limit -> clamped to 0
        let v = serde_json::json!({ "limit": 100, "remaining": 150 });
        assert!((utilization_from_limit_remaining(&v).unwrap() - 0.0).abs() < 1e-6);

        // missing fields -> None
        let v = serde_json::json!({ "limit": 100 });
        assert!(utilization_from_limit_remaining(&v).is_none());
    }

    #[test]
    fn test_build_percent_usage_takes_max() {
        let usage = build_percent_usage(vec![30.0, 80.0, 50.0]);
        assert!((usage.total_used - 80.0).abs() < 1e-6);
        assert_eq!(usage.total_budget, Some(100.0));
        assert!((usage.remaining.unwrap() - 20.0).abs() < 1e-6);
        assert_eq!(usage.currency, "%");
    }

    #[test]
    fn test_build_percent_usage_clamps() {
        let usage = build_percent_usage(vec![150.0]);
        assert!((usage.total_used - 100.0).abs() < 1e-6);
        assert!((usage.remaining.unwrap() - 0.0).abs() < 1e-6);
    }

    #[test]
    fn test_build_percent_usage_empty() {
        let usage = build_percent_usage(vec![]);
        assert!((usage.total_used - 0.0).abs() < 1e-6);
        assert_eq!(usage.remaining, Some(100.0));
    }

    #[test]
    fn test_glm_json_mapping() {
        // 模拟 GLM 响应，验证 unit==3 / unit==6 提取逻辑
        let json = serde_json::json!({
            "data": {
                "limits": [
                    { "type": "TOKENS_LIMIT", "unit": 3, "percentage": 80 },
                    { "type": "TOKENS_LIMIT", "unit": 6, "percentage": 50 },
                    { "type": "TOKENS_LIMIT", "unit": 9, "percentage": 99 } // 应被忽略
                ],
                "level": "VIP"
            }
        });

        let mut utilizations: Vec<f64> = Vec::new();
        if let Some(limits) = json
            .get("data")
            .and_then(|v| v.get("limits"))
            .and_then(|v| v.as_array())
        {
            for item in limits {
                let unit = item.get("unit").and_then(|v| v.as_u64());
                let percentage = item.get("percentage").and_then(|v| v.as_f64());
                if let (Some(u), Some(p)) = (unit, percentage) {
                    if u == 3 || u == 6 {
                        utilizations.push(p);
                    }
                }
            }
        }
        assert_eq!(utilizations, vec![80.0, 50.0]);
    }

    #[test]
    fn test_minimax_json_mapping() {
        // 模拟 MiniMax 响应，验证 general + weekly_status==1 逻辑
        let json = serde_json::json!({
            "model_remains": [
                {
                    "model_name": "other",
                    "current_interval_remaining_percent": 10,
                    "current_weekly_status": 1,
                    "current_weekly_remaining_percent": 20
                },
                {
                    "model_name": "general",
                    "current_interval_remaining_percent": 70,
                    "current_weekly_status": 1,
                    "current_weekly_remaining_percent": 60
                }
            ]
        });

        let mut utilizations: Vec<f64> = Vec::new();
        if let Some(arr) = json.get("model_remains").and_then(|v| v.as_array()) {
            for item in arr {
                let is_general = item
                    .get("model_name")
                    .and_then(|v| v.as_str())
                    .map(|s| s == "general")
                    .unwrap_or(false);
                if !is_general {
                    continue;
                }
                if let Some(remain) = item
                    .get("current_interval_remaining_percent")
                    .and_then(|v| v.as_f64())
                {
                    utilizations.push(100.0 - remain);
                }
                let weekly_status = item.get("current_weekly_status").and_then(|v| v.as_u64());
                if weekly_status == Some(1) {
                    if let Some(remain) = item
                        .get("current_weekly_remaining_percent")
                        .and_then(|v| v.as_f64())
                    {
                        utilizations.push(100.0 - remain);
                    }
                }
            }
        }
        // general: interval=100-70=30, weekly=100-60=40
        assert_eq!(utilizations, vec![30.0, 40.0]);
    }
}
