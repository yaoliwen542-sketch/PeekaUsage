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
        "volcengine" => fetch_volcengine(client, api_key).await,
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

// ===== 火山方舟（Volcengine）=====
//
// 火山方舟用量查询走控制面 OpenAPI（非数据面 ark 域名），使用火山签名 V4（AK/SK）认证。
// api_key 参数实际格式为 "AccessKeyId:SecretAccessKey"（用冒号分隔），在函数内部解析。
//
// 主链路：POST https://open.volcengineapi.com/?Action=GetAFPUsage&Version=2024-09-30
//   - 返回绝对额度（已用/总额），转换成百分比型 UsageData
//   - 若未订阅 AFP 或接口不可用，回退到 GetCodingPlanUsage
//
// 回退链路：POST https://open.volcengineapi.com/?Action=GetCodingPlanUsage&Version=2024-09-30
//   - 返回百分比利用率，直接映射
//
// 注意：签名逻辑见 super::sigv4；本函数仅负责发请求 + 解析。
// 签名算法待真实 AK/SK 端到端验证（当前实现遵循 cc-switch 的 V4 变体）。
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

    // 主链路：GetAFPUsage
    match fetch_volc_action(client, ak, sk, "GetAFPUsage").await {
        Ok(usage) => Ok(usage),
        Err(ProviderError::AuthError(e)) => Err(ProviderError::AuthError(e)),
        Err(afp_err) => {
            // AFP 失败（非鉴权类），回退到 GetCodingPlanUsage
            match fetch_volc_action(client, ak, sk, "GetCodingPlanUsage").await {
                Ok(usage) => Ok(usage),
                Err(ProviderError::AuthError(e)) => Err(ProviderError::AuthError(e)),
                Err(coding_err) => Err(ProviderError::RequestError(format!(
                    "GetAFPUsage 失败({}) 且 GetCodingPlanUsage 失败({})",
                    afp_err, coding_err
                ))),
            }
        }
    }
}

/// 执行单个火山方舟 Action 查询
///
/// 通用流程：构造请求体 -> 签名 -> 发送 -> 状态码检查 -> 响应解析。
/// 不同 Action 的响应字段不同，由 parse_*_response 处理。
async fn fetch_volc_action(
    client: &Client,
    ak: &str,
    sk: &str,
    action: &str,
) -> Result<UsageData, ProviderError> {
    let host = "open.volcengineapi.com";
    let query = format!("Action={}&Version=2024-09-30", action);
    let url = format!("https://{}?{}", host, query);

    // 请求体：火山方舟部分接口要求 Region 等参数。这里传最小 JSON。
    // 即使是空对象 {} 也能签名（x-content-sha256 会反映 body 哈希）。
    let body = serde_json::json!({
        "Region": "cn-beijing",
    });
    let body_bytes = serde_json::to_vec(&body)
        .map_err(|e| ProviderError::RequestError(format!("序列化请求体失败: {}", e)))?;

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
        // 业务层错误：返回 RequestError（非鉴权类），让上层回退或汇总
        return Err(ProviderError::RequestError(err_msg));
    }

    // 按 Action 分发解析
    match action {
        "GetAFPUsage" => parse_afp_response(&json),
        "GetCodingPlanUsage" => parse_coding_plan_response(&json),
        _ => Err(ProviderError::ParseError(format!(
            "未实现的火山方舟 Action: {}",
            action
        ))),
    }
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

/// 解析 GetAFPUsage 响应（绝对额度 -> 百分比型）
///
/// 预期响应结构（火山方舟 GetAFPUsage）：
/// ```json
/// {
///   "Result": {
///     "TotalAmount": 100.0,
///     "UsedAmount": 30.0,
///     "RemainingAmount": 70.0,
///     "Currency": "CNY"
///   }
/// }
/// ```
/// 映射：utilization = UsedAmount / TotalAmount * 100（TotalAmount 为 0 时返回 0%）。
///
/// 注意：实际字段名可能因火山接口版本不同而异。当前实现基于公开文档与 cc-switch 调研，
/// 待真实 AK/SK 端到端验证。若字段缺失则回退到 GetCodingPlanUsage。
fn parse_afp_response(json: &Value) -> Result<UsageData, ProviderError> {
    let result = json
        .get("Result")
        .ok_or_else(|| ProviderError::ParseError("GetAFPUsage 响应缺少 Result 字段".into()))?;

    let total = result.get("TotalAmount").and_then(|v| v.as_f64());
    let used = result.get("UsedAmount").and_then(|v| v.as_f64());

    match (total, used) {
        (Some(t), Some(u)) if t > 0.0 => {
            let utilization = (u / t * 100.0).clamp(0.0, 100.0);
            Ok(build_percent_usage(vec![utilization]))
        }
        _ => Err(ProviderError::ParseError(
            "GetAFPUsage 响应缺少 TotalAmount/UsedAmount 或 TotalAmount 为 0".into(),
        )),
    }
}

/// 解析 GetCodingPlanUsage 响应（百分比利用率）
///
/// 预期响应结构（火山方舟 GetCodingPlanUsage）：
/// ```json
/// {
///   "Result": {
///     "UsagePercent": 65.0,
///     "ResetTime": "2026-07-20T00:00:00Z"
///   }
/// }
/// ```
/// 映射：utilization = UsagePercent（0-100）。
///
/// 注意：实际字段名待真实 AK/SK 端到端验证。
fn parse_coding_plan_response(json: &Value) -> Result<UsageData, ProviderError> {
    let result = json.get("Result").ok_or_else(|| {
        ProviderError::ParseError("GetCodingPlanUsage 响应缺少 Result 字段".into())
    })?;

    // 优先用 UsagePercent
    if let Some(percent) = result.get("UsagePercent").and_then(|v| v.as_f64()) {
        return Ok(build_percent_usage(vec![percent.clamp(0.0, 100.0)]));
    }

    // 回退：尝试 Utilization 字段
    if let Some(percent) = result.get("Utilization").and_then(|v| v.as_f64()) {
        return Ok(build_percent_usage(vec![percent.clamp(0.0, 100.0)]));
    }

    Err(ProviderError::ParseError(
        "GetCodingPlanUsage 响应缺少 UsagePercent/Utilization 字段".into(),
    ))
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
        let json = serde_json::json!({
            "Result": {
                "TotalAmount": 200.0,
                "UsedAmount": 50.0,
                "RemainingAmount": 150.0,
                "Currency": "CNY"
            }
        });
        let usage = parse_afp_response(&json).unwrap();
        // 50 / 200 = 25%
        assert!((usage.total_used - 25.0).abs() < 1e-6);
        assert_eq!(usage.currency, "%");
    }

    #[test]
    fn test_volc_parse_afp_response_missing_fields() {
        let json = serde_json::json!({ "Result": { "Foo": "bar" } });
        assert!(parse_afp_response(&json).is_err());
    }

    #[test]
    fn test_volc_parse_coding_plan_response() {
        let json = serde_json::json!({
            "Result": { "UsagePercent": 65.0, "ResetTime": "2026-07-20T00:00:00Z" }
        });
        let usage = parse_coding_plan_response(&json).unwrap();
        assert!((usage.total_used - 65.0).abs() < 1e-6);
    }

    #[test]
    fn test_volc_parse_coding_plan_response_fallback_field() {
        // UsagePercent 缺失时回退到 Utilization
        let json = serde_json::json!({ "Result": { "Utilization": 40.0 } });
        let usage = parse_coding_plan_response(&json).unwrap();
        assert!((usage.total_used - 40.0).abs() < 1e-6);
    }
}
