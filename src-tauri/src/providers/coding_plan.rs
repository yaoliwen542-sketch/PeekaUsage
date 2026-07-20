use reqwest::Client;
use serde_json::Value;

use super::traits::ProviderError;
use super::types::{window_labels, SubscriptionWindow, UsageData};

/// 执行 CodingPlan 查询
///
/// CodingPlan 类供应商返回百分比型多窗口用量（如 Kimi 的 5 小时/周限额）。
/// 统一转成 UsageData（currency="%"，total_budget=100，total_used=最高窗口 utilization），
/// 各窗口明细放入 UsageData.windows，供前端逐窗口展示。
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
        "volcengine" => fetch_volcengine(client, api_key).await,
        _ => Err(ProviderError::RequestError(format!(
            "不支持的 CodingPlan 供应商: {}",
            provider
        ))),
    }
}

/// 把 utilization (0-100) 组装成百分比型 UsageData（无分窗口明细的兜底）
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
        windows: Vec::new(),
    }
}

/// 带分窗口明细的百分比型 UsageData（Coding Plan 类供应商的标准返回）
///
/// total_used 仍取最高窗口 utilization，兼容只读单值的旧展示链路；
/// windows 透传给前端逐窗口渲染（如 Kimi 的 5 小时窗口 + 周限额窗口）。
fn build_percent_usage_with_windows(windows: Vec<SubscriptionWindow>) -> UsageData {
    let mut data = build_percent_usage(windows.iter().map(|w| w.utilization).collect());
    data.windows = windows;
    data
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
// 响应（注意 limit / remaining 是字符串）：
// {
//   "limits": [
//     { "detail": { "limit": "100", "remaining": "30", "resetTime": "2026-07-17T10:00:00Z" } }
//   ],
//   "usage": { "limit": "1000", "remaining": "800", "resetTime": "2026-07-20T00:00:00Z" }
// }
//
// 映射：
// - limits[0].detail -> five_hour 窗口（utilization = (limit-remaining)/limit*100）
// - usage -> weekly_limit 窗口（utilization = (limit-remaining)/limit*100）
// - totalQuota -> monthly 窗口（仅部分套餐返回；空对象时不展示）
//
// 注意：5 小时窗口打满后 detail 里会省略 remaining 只留 used；
// 窗口未激活时 limits 可能整个是空数组。这两种情况都必须兜底展示，
// 不能让卡片上的 5 小时进度条时有时无。
async fn fetch_kimi(client: &Client, api_key: &str) -> Result<UsageData, ProviderError> {
    let req = client
        .get("https://api.kimi.com/coding/v1/usages")
        .bearer_auth(api_key)
        .header("Accept", "application/json");

    let json = fetch_json(req).await?;
    parse_kimi_response(&json)
}

/// 解析 Kimi usages 响应为分窗口 UsageData（纯函数，便于单测）
fn parse_kimi_response(json: &Value) -> Result<UsageData, ProviderError> {
    let mut windows: Vec<SubscriptionWindow> = Vec::new();

    // limits[0].detail -> five_hour（始终展示：字段缺失按 0% 兜底，保持卡片布局稳定）
    let five_hour_detail = json
        .get("limits")
        .and_then(|v| v.get(0))
        .and_then(|v| v.get("detail"));
    let (five_hour_utilization, five_hour_resets) = match five_hour_detail {
        Some(detail) => (
            utilization_from_quota(detail).unwrap_or(0.0),
            detail
                .get("resetTime")
                .and_then(|v| v.as_str())
                .map(str::to_string),
        ),
        None => (0.0, None),
    };
    windows.push(SubscriptionWindow {
        label: window_labels::FIVE_HOUR.to_string(),
        utilization: five_hour_utilization,
        resets_at: five_hour_resets,
    });

    // usage -> weekly_limit
    let mut has_weekly = false;
    if let Some(usage) = json.get("usage") {
        if let Some(u) = utilization_from_quota(usage) {
            has_weekly = true;
            windows.push(SubscriptionWindow {
                label: window_labels::WEEKLY_LIMIT.to_string(),
                utilization: u,
                resets_at: usage
                    .get("resetTime")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
            });
        }
    }

    // totalQuota -> monthly（只有部分套餐级别返回有效额度；LEVEL_INTERMEDIATE 是空对象）
    if let Some(total) = json.get("totalQuota") {
        if let Some(u) = utilization_from_quota(total) {
            windows.push(SubscriptionWindow {
                label: window_labels::MONTHLY.to_string(),
                utilization: u,
                resets_at: total
                    .get("resetTime")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
            });
        }
    }

    // 周限额是 Kimi 订阅的核心窗口；连它都解析不出来才认为响应无效
    if !has_weekly {
        return Err(ProviderError::ParseError(
            "Kimi 响应中未找到有效的 usage 周限额字段".into(),
        ));
    }

    Ok(build_percent_usage_with_windows(windows))
}

/// 从 { limit, remaining } 或 { limit, used } 对象计算 utilization 百分比 (0-100)
///
/// utilization = (limit - remaining) / limit * 100，或 used / limit * 100。
/// 优先读 remaining；remaining 缺失时回退到 used（Kimi 的 5 小时窗口打满后
/// detail 里只剩 limit + used，没有 remaining）。
/// 若 limit 为 0、缺失，或 remaining / used 都缺失，返回 None。
/// 注意：Kimi usages 接口的 limit / remaining / used 是字符串（"100"）而非数字，
/// 必须两种形态都兼容，否则永远解析不出有效利用率。
fn utilization_from_quota(obj: &Value) -> Option<f64> {
    let limit = json_number(obj.get("limit"))?;
    if limit <= 0.0 {
        return None;
    }
    let used = if let Some(remaining) = json_number(obj.get("remaining")) {
        (limit - remaining).max(0.0)
    } else {
        json_number(obj.get("used"))?.max(0.0)
    };
    Some((used / limit * 100.0).clamp(0.0, 100.0))
}

/// 兼容数字与字符串两种 JSON 数值形态（如 100 与 "100"）
fn json_number(value: Option<&Value>) -> Option<f64> {
    value
        .and_then(|v| v.as_f64())
        .or_else(|| value?.as_str()?.trim().parse::<f64>().ok())
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

    let mut windows: Vec<SubscriptionWindow> = Vec::new();

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
                let label = match u {
                    3 => Some(window_labels::FIVE_HOUR),
                    6 => Some(window_labels::WEEKLY_LIMIT),
                    _ => None,
                };
                if let Some(label) = label {
                    windows.push(SubscriptionWindow {
                        label: label.to_string(),
                        utilization: p.clamp(0.0, 100.0),
                        resets_at: None,
                    });
                }
            }
        }
    }

    if windows.is_empty() {
        return Err(ProviderError::ParseError(
            "GLM 响应中未找到 unit==3 或 unit==6 的 limits 条目".into(),
        ));
    }

    Ok(build_percent_usage_with_windows(windows))
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

    let mut windows: Vec<SubscriptionWindow> = Vec::new();

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
                windows.push(SubscriptionWindow {
                    label: window_labels::FIVE_HOUR.to_string(),
                    utilization: (100.0 - remain).clamp(0.0, 100.0),
                    resets_at: None,
                });
            }

            // weekly_limit: 仅当 current_weekly_status==1 时取
            let weekly_status = item.get("current_weekly_status").and_then(|v| v.as_u64());
            if weekly_status == Some(1) {
                if let Some(remain) = item
                    .get("current_weekly_remaining_percent")
                    .and_then(|v| v.as_f64())
                {
                    windows.push(SubscriptionWindow {
                        label: window_labels::WEEKLY_LIMIT.to_string(),
                        utilization: (100.0 - remain).clamp(0.0, 100.0),
                        resets_at: None,
                    });
                }
            }
        }
    }

    if windows.is_empty() {
        return Err(ProviderError::ParseError(
            "MiniMax 响应中未找到 model_name==\"general\" 的有效用量条目".into(),
        ));
    }

    Ok(build_percent_usage_with_windows(windows))
}

// ===== 火山方舟（Volcengine）=====
//
// 火山方舟套餐用量查询走控制面 OpenAPI（非数据面 ark 域名），使用火山签名 V4（AK/SK）认证。
// api_key 参数实际格式为 "AccessKeyId:SecretAccessKey"（用冒号分隔），在函数内部解析。
//
// 唯一链路：POST https://open.volcengineapi.com/?Action=GetAFPUsage&Version=2024-01-01
//   - 官方接口（Agent Plan / Coding Plan API 分组），返回 AFP 额度的
//     5 小时 / 每日 / 每周 / 每月四个窗口（Quota + Used + ResetTime 毫秒时间戳）
//   - 已用真实 AK/SK 端到端验证（2026-07）
//
// 历史教训：
// - API Version 必须是 2024-01-01；此前臆测的 2024-09-30 会被网关直接 404
// - 臆造的 GetCodingPlanUsage Action 不存在（官方 Coding Plan 分组下只有
//   ListArkCodingPlanModel / GetSeatInfoUsage / ListSeatInfoUsages），不要再加回退
// - 请求体为 {}，多传 Region 等字段可能触发 InvalidParameter
async fn fetch_volcengine(client: &Client, api_key: &str) -> Result<UsageData, ProviderError> {
    // 解析 AK:SK
    let (ak, sk) = api_key.split_once(':').ok_or_else(|| {
        ProviderError::AuthError("火山方舟 Key 格式应为 AccessKeyId:SecretAccessKey".into())
    })?;
    if ak.is_empty() || sk.is_empty() {
        return Err(ProviderError::AuthError(
            "火山方舟 AccessKeyId 或 SecretAccessKey 为空".into(),
        ));
    }

    fetch_volc_action(client, ak, sk, "GetAFPUsage").await
}

/// 火山方舟控制面 API 版本（全部 Agent Plan / Coding Plan 接口统一）
const VOLC_API_VERSION: &str = "2024-01-01";

/// 执行火山方舟 Action 查询
///
/// 通用流程：构造请求体 -> 签名 -> 发送 -> 状态码检查 -> 响应解析。
/// 响应字段由 parse_afp_response 处理。
async fn fetch_volc_action(
    client: &Client,
    ak: &str,
    sk: &str,
    action: &str,
) -> Result<UsageData, ProviderError> {
    let host = "open.volcengineapi.com";
    let query = format!("Action={}&Version={}", action, VOLC_API_VERSION);
    let url = format!("https://{}/?{}", host, query);

    // 官方请求体为空对象
    let body_bytes = b"{}".to_vec();

    // 签名
    let headers = super::sigv4::sign_volc_request(super::sigv4::VolcSignParams {
        access_key_id: ak,
        secret_access_key: sk,
        region: "cn-beijing",
        service: "ark",
        host,
        method: "POST",
        query: &query,
        body: &body_bytes,
    });

    // 构造请求
    let mut req = client.post(&url).body(body_bytes);
    for (k, v) in headers {
        req = req.header(k, v);
    }

    // 发请求
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

    // 火山 OpenAPI 错误响应：{"ResponseMetadata": {"Error": {"Code": "...", "Message": "..."}}, "Result": null}
    // 或顶层 {"Code": "...", "Message": "..."}
    if let Some(err_msg) = extract_volc_error(&json) {
        return Err(ProviderError::RequestError(err_msg));
    }

    parse_afp_response(&json)
}

/// 从火山 OpenAPI 响应中提取错误信息
///
/// 火山错误响应通常为：
/// ```json
/// { "ResponseMetadata": { "Error": { "Code": "xxx", "Message": "yyy" } } }
/// ```
/// 或简化形式：
/// ```json
/// { "Code": "xxx", "Message": "yyy" }
/// ```
/// 返回 Some("Code: Message") 表示业务错误；None 表示无业务错误。
fn extract_volc_error(json: &Value) -> Option<String> {
    // 形式 1：ResponseMetadata.Error
    if let Some(err) = json.get("ResponseMetadata").and_then(|v| v.get("Error")) {
        let code = err
            .get("Code")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown");
        let message = err
            .get("Message")
            .and_then(|v| v.as_str())
            .unwrap_or("无错误消息");
        return Some(format!("{}: {}", code, message));
    }
    // 形式 2：顶层 Code/Message
    if let Some(code) = json.get("Code").and_then(|v| v.as_str()) {
        let message = json
            .get("Message")
            .and_then(|v| v.as_str())
            .unwrap_or("无错误消息");
        return Some(format!("{}: {}", code, message));
    }
    None
}

/// 解析 GetAFPUsage 响应为分窗口 UsageData（纯函数，便于单测）
///
/// 真实响应结构（官方文档 + 真实 AK/SK 验证，Version 2024-01-01）：
/// ```json
/// {
///   "Result": {
///     "PlanType": "medium",
///     "AFPFiveHour": { "Quota": 10000, "Used": 1.9221, "SubscribeTime": 1784526841000, "ResetTime": 1784544841000 },
///     "AFPDaily":    { "Quota": 50000, "Used": 0,      "SubscribeTime": 1784476800000, "ResetTime": 1784563200000 },
///     "AFPWeekly":   { "Quota": 35000, "Used": 4.1392, "SubscribeTime": 1784476800000, "ResetTime": 1785081600000 },
///     "AFPMonthly":  { "Quota": 100000, "Used": 89968.2881, "SubscribeTime": 1782877567000, "ResetTime": 1785599999000 }
///   }
/// }
/// ```
/// 映射：四个窗口 -> five_hour / daily / weekly_limit / monthly，
/// utilization = Used / Quota * 100；ResetTime 是 epoch 毫秒，转成 RFC3339。
/// 窗口缺失或 Quota<=0 时跳过该窗口；四个窗口全无效才报错。
fn parse_afp_response(json: &Value) -> Result<UsageData, ProviderError> {
    let result = json
        .get("Result")
        .ok_or_else(|| ProviderError::ParseError("GetAFPUsage 响应缺少 Result 字段".into()))?;

    let mut windows: Vec<SubscriptionWindow> = Vec::new();
    for (field, label) in [
        ("AFPFiveHour", window_labels::FIVE_HOUR),
        ("AFPDaily", window_labels::DAILY),
        ("AFPWeekly", window_labels::WEEKLY_LIMIT),
        ("AFPMonthly", window_labels::MONTHLY),
    ] {
        if let Some(window) = parse_afp_window(result.get(field), label) {
            windows.push(window);
        }
    }

    if windows.is_empty() {
        return Err(ProviderError::ParseError(
            "GetAFPUsage 响应中未找到有效的 AFPFiveHour/AFPDaily/AFPWeekly/AFPMonthly 窗口".into(),
        ));
    }

    Ok(build_percent_usage_with_windows(windows))
}

/// 解析单个 AFP 窗口对象 {Quota, Used, SubscribeTime, ResetTime}
/// Quota<=0 或字段缺失时返回 None
fn parse_afp_window(value: Option<&Value>, label: &str) -> Option<SubscriptionWindow> {
    let obj = value?;
    let quota = json_number(obj.get("Quota"))?;
    let used = json_number(obj.get("Used"))?;
    if quota <= 0.0 {
        return None;
    }
    let resets_at = obj
        .get("ResetTime")
        .and_then(|v| v.as_i64())
        .and_then(|ms| chrono::DateTime::from_timestamp_millis(ms))
        .filter(|dt| dt.timestamp_millis() > 0)
        .map(|dt| dt.to_rfc3339());
    Some(SubscriptionWindow {
        label: label.to_string(),
        utilization: (used / quota * 100.0).clamp(0.0, 100.0),
        resets_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_utilization_from_quota_remaining_form() {
        let v = serde_json::json!({ "limit": 100, "remaining": 30 });
        assert!((utilization_from_quota(&v).unwrap() - 70.0).abs() < 1e-6);

        let v = serde_json::json!({ "limit": 1000, "remaining": 800 });
        assert!((utilization_from_quota(&v).unwrap() - 20.0).abs() < 1e-6);

        // limit=0 -> None
        let v = serde_json::json!({ "limit": 0, "remaining": 0 });
        assert!(utilization_from_quota(&v).is_none());

        // remaining > limit -> clamped to 0
        let v = serde_json::json!({ "limit": 100, "remaining": 150 });
        assert!((utilization_from_quota(&v).unwrap() - 0.0).abs() < 1e-6);

        // remaining / used 都缺失 -> None
        let v = serde_json::json!({ "limit": 100 });
        assert!(utilization_from_quota(&v).is_none());
    }

    #[test]
    fn test_utilization_from_quota_used_fallback() {
        // Kimi 5 小时窗口打满后 detail 只剩 limit + used，没有 remaining
        let v = serde_json::json!({ "limit": "100", "used": "100" });
        assert!((utilization_from_quota(&v).unwrap() - 100.0).abs() < 1e-6);

        let v = serde_json::json!({ "limit": "100", "used": "80" });
        assert!((utilization_from_quota(&v).unwrap() - 80.0).abs() < 1e-6);

        // used > limit -> clamped to 100
        let v = serde_json::json!({ "limit": 100, "used": 120 });
        assert!((utilization_from_quota(&v).unwrap() - 100.0).abs() < 1e-6);

        // remaining 优先于 used
        let v = serde_json::json!({ "limit": 100, "remaining": 90, "used": 50 });
        assert!((utilization_from_quota(&v).unwrap() - 10.0).abs() < 1e-6);
    }

    #[test]
    fn test_utilization_from_quota_string_values() {
        // Kimi usages 接口实际返回字符串形态（"100"），必须兼容
        let v = serde_json::json!({ "limit": "100", "remaining": "30" });
        assert!((utilization_from_quota(&v).unwrap() - 70.0).abs() < 1e-6);

        // 数字与字符串混用
        let v = serde_json::json!({ "limit": "1000", "remaining": 800 });
        assert!((utilization_from_quota(&v).unwrap() - 20.0).abs() < 1e-6);

        // 非法字符串 -> None
        let v = serde_json::json!({ "limit": "abc", "remaining": "30" });
        assert!(utilization_from_quota(&v).is_none());
    }

    #[test]
    fn test_parse_kimi_response_normal() {
        // 常规形态：remaining 存在，5 小时窗口 + 周限额 + 空 totalQuota
        let json = serde_json::json!({
            "usage": { "limit": "100", "used": "16", "remaining": "84", "resetTime": "2026-07-26T00:00:00Z" },
            "limits": [
                { "window": { "duration": 300, "timeUnit": "TIME_UNIT_MINUTE" },
                  "detail": { "limit": "100", "remaining": "20", "used": "80", "resetTime": "2026-07-19T10:00:00Z" } }
            ],
            "totalQuota": {}
        });
        let usage = parse_kimi_response(&json).unwrap();
        assert_eq!(usage.windows.len(), 2);
        assert_eq!(usage.windows[0].label, "five_hour");
        assert!((usage.windows[0].utilization - 80.0).abs() < 1e-6);
        assert!(usage.windows[0].resets_at.is_some());
        assert_eq!(usage.windows[1].label, "weekly_limit");
        assert!((usage.windows[1].utilization - 16.0).abs() < 1e-6);
    }

    #[test]
    fn test_parse_kimi_response_five_hour_full_without_remaining() {
        // 线上实测：5 小时窗口打满后 detail 省略 remaining，只剩 limit + used
        let json = serde_json::json!({
            "usage": { "limit": "100", "used": "20", "remaining": "80", "resetTime": "2026-07-26T00:00:00Z" },
            "limits": [
                { "window": { "duration": 300, "timeUnit": "TIME_UNIT_MINUTE" },
                  "detail": { "limit": "100", "used": "100", "resetTime": "2026-07-20T06:33:24Z" } }
            ],
            "totalQuota": {}
        });
        let usage = parse_kimi_response(&json).unwrap();
        assert_eq!(usage.windows.len(), 2);
        assert_eq!(usage.windows[0].label, "five_hour");
        assert!((usage.windows[0].utilization - 100.0).abs() < 1e-6);
        assert_eq!(usage.windows[1].label, "weekly_limit");
    }

    #[test]
    fn test_parse_kimi_response_empty_limits_shows_five_hour_zero() {
        // 5 小时窗口未激活时 limits 是空数组：仍要展示 0%，不能让卡片少一行
        let json = serde_json::json!({
            "usage": { "limit": "100", "used": "20", "remaining": "80", "resetTime": "2026-07-26T00:00:00Z" },
            "limits": [],
            "totalQuota": {}
        });
        let usage = parse_kimi_response(&json).unwrap();
        assert_eq!(usage.windows.len(), 2);
        assert_eq!(usage.windows[0].label, "five_hour");
        assert!((usage.windows[0].utilization - 0.0).abs() < 1e-6);
        assert!(usage.windows[0].resets_at.is_none());
        assert_eq!(usage.windows[1].label, "weekly_limit");
    }

    #[test]
    fn test_parse_kimi_response_monthly_quota() {
        // 更高套餐返回有效 totalQuota 时要展示月度窗口
        let json = serde_json::json!({
            "usage": { "limit": "100", "used": "20", "remaining": "80", "resetTime": "2026-07-26T00:00:00Z" },
            "limits": [],
            "totalQuota": { "limit": "1000", "used": "500", "remaining": "500", "resetTime": "2026-08-01T00:00:00Z" }
        });
        let usage = parse_kimi_response(&json).unwrap();
        assert_eq!(usage.windows.len(), 3);
        assert_eq!(usage.windows[2].label, "monthly");
        assert!((usage.windows[2].utilization - 50.0).abs() < 1e-6);
    }

    #[test]
    fn test_parse_kimi_response_missing_usage_is_error() {
        let json = serde_json::json!({ "limits": [], "totalQuota": {} });
        assert!(parse_kimi_response(&json).is_err());
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
    fn test_build_percent_usage_with_windows() {
        let usage = build_percent_usage_with_windows(vec![
            SubscriptionWindow {
                label: window_labels::FIVE_HOUR.to_string(),
                utilization: 30.0,
                resets_at: None,
            },
            SubscriptionWindow {
                label: window_labels::WEEKLY_LIMIT.to_string(),
                utilization: 80.0,
                resets_at: Some("2026-07-20T00:00:00Z".to_string()),
            },
        ]);
        // total_used 仍取最高窗口，windows 明细完整透传
        assert!((usage.total_used - 80.0).abs() < 1e-6);
        assert_eq!(usage.windows.len(), 2);
        assert_eq!(usage.windows[0].label, "five_hour");
        assert_eq!(usage.windows[1].label, "weekly_limit");
        assert!(usage.windows[1].resets_at.is_some());
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

    #[test]
    fn test_volc_extract_error_metadata_form() {
        let json = serde_json::json!({
            "ResponseMetadata": {
                "Error": { "Code": "NotFound", "Message": "AFP not subscribed" }
            }
        });
        let err = extract_volc_error(&json).unwrap();
        assert_eq!(err, "NotFound: AFP not subscribed");
    }

    #[test]
    fn test_volc_extract_error_top_level_form() {
        let json = serde_json::json!({
            "Code": "InvalidParameter",
            "Message": "bad region"
        });
        let err = extract_volc_error(&json).unwrap();
        assert_eq!(err, "InvalidParameter: bad region");
    }

    #[test]
    fn test_volc_extract_error_none_on_success() {
        let json = serde_json::json!({
            "Result": { "TotalAmount": 100.0, "UsedAmount": 30.0 }
        });
        assert!(extract_volc_error(&json).is_none());
    }

    #[test]
    fn test_volc_parse_afp_response() {
        // 真实响应形态（2026-07 实测）：四个 AFP 窗口
        let json = serde_json::json!({
            "Result": {
                "PlanType": "medium",
                "AFPFiveHour": { "Quota": 10000, "Used": 1.9221, "SubscribeTime": 1784526841000i64, "ResetTime": 1784544841000i64 },
                "AFPDaily":    { "Quota": 50000, "Used": 0,      "SubscribeTime": 1784476800000i64, "ResetTime": 1784563200000i64 },
                "AFPWeekly":   { "Quota": 35000, "Used": 4.1392, "SubscribeTime": 1784476800000i64, "ResetTime": 1785081600000i64 },
                "AFPMonthly":  { "Quota": 100000, "Used": 89968.2881, "SubscribeTime": 1782877567000i64, "ResetTime": 1785599999000i64 }
            }
        });
        let usage = parse_afp_response(&json).unwrap();
        assert_eq!(usage.currency, "%");
        assert_eq!(usage.windows.len(), 4);
        assert_eq!(usage.windows[0].label, "five_hour");
        assert!((usage.windows[0].utilization - 1.9221 / 10000.0 * 100.0).abs() < 1e-6);
        assert!(usage.windows[0].resets_at.is_some());
        assert_eq!(usage.windows[1].label, "daily");
        assert!((usage.windows[1].utilization - 0.0).abs() < 1e-6);
        assert_eq!(usage.windows[2].label, "weekly_limit");
        assert_eq!(usage.windows[3].label, "monthly");
        assert!((usage.windows[3].utilization - 89968.2881 / 100000.0 * 100.0).abs() < 1e-4);
        // total_used 取最高窗口（月度 89.97%）
        assert!((usage.total_used - 89968.2881 / 100000.0 * 100.0).abs() < 1e-4);
    }

    #[test]
    fn test_volc_parse_afp_response_skips_invalid_windows() {
        // Quota=0 的窗口跳过；部分窗口缺失时其余正常展示
        let json = serde_json::json!({
            "Result": {
                "PlanType": "medium",
                "AFPFiveHour": { "Quota": 0, "Used": 0, "SubscribeTime": 0, "ResetTime": 0 },
                "AFPMonthly":  { "Quota": 100000, "Used": 50000, "SubscribeTime": 1782877567000i64, "ResetTime": 1785599999000i64 }
            }
        });
        let usage = parse_afp_response(&json).unwrap();
        assert_eq!(usage.windows.len(), 1);
        assert_eq!(usage.windows[0].label, "monthly");
        assert!((usage.windows[0].utilization - 50.0).abs() < 1e-6);
    }

    #[test]
    fn test_volc_parse_afp_response_missing_fields() {
        let json = serde_json::json!({ "Result": { "Foo": "bar" } });
        assert!(parse_afp_response(&json).is_err());
    }

    #[test]
    fn test_volc_parse_afp_window_reset_time_zero_is_none() {
        // ResetTime 为 0（未激活窗口）时 resets_at 为 None，不产生 1970 时间
        let json =
            serde_json::json!({ "Quota": 100, "Used": 10, "SubscribeTime": 0, "ResetTime": 0 });
        let window = parse_afp_window(Some(&json), "five_hour").unwrap();
        assert!(window.resets_at.is_none());
        assert!((window.utilization - 10.0).abs() < 1e-6);
    }
}
