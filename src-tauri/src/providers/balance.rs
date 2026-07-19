use reqwest::Client;
use serde_json::Value;

use super::traits::ProviderError;
use super::types::{AuthScheme, BalanceFieldMap, UsageData};

/// 执行 Balance 查询
///
/// 流程：
/// 1. 用 url + auth 构造请求（URL 中的动态时间占位符会被替换为当前时间）
/// 2. 发请求（30s 超时，复用 ProviderManager 的 client），先 bytes() 再 serde_json::from_slice（区分网络错和解析错）
/// 3. 用 jsonpath 按 field_map 提取字段
/// 4. 组装 UsageData 返回
pub async fn execute_balance_query(
    client: &Client,
    url: &str,
    auth: &AuthScheme,
    field_map: &BalanceFieldMap,
    api_key: &str,
) -> Result<UsageData, ProviderError> {
    // 替换 URL 中的动态时间占位符为当前时间
    let resolved_url = resolve_time_placeholders(url);

    // 构造请求
    let req_builder = client.get(&resolved_url);
    let req_builder = apply_auth(req_builder, auth, api_key);

    // 发请求
    let resp = req_builder.send().await.map_err(|e| {
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

    let status = resp.status();
    if status.as_u16() == 401 || status.as_u16() == 403 {
        return Err(ProviderError::AuthError(format!(
            "认证失败 (HTTP {})",
            status.as_u16()
        )));
    }
    if status.as_u16() == 429 {
        return Err(ProviderError::RateLimited("请求过于频繁".to_string()));
    }
    if !status.is_success() {
        return Err(ProviderError::RequestError(format!("HTTP {}", status)));
    }

    // bytes-then-parse 模式（抄 cc-switch，区分读体错和解析错）
    let body_bytes = resp
        .bytes()
        .await
        .map_err(|e| ProviderError::RequestError(format!("读取响应体失败: {}", e)))?;

    let json: Value = serde_json::from_slice(&body_bytes)
        .map_err(|e| ProviderError::ParseError(format!("解析 JSON 失败: {}", e)))?;

    // 用 jsonpath 提取字段
    let scale = field_map.scale;
    let total = extract_field(&json, &field_map.total)?.map(|v| apply_scale(v, scale));
    let used = match &field_map.used {
        Some(path) => extract_field(&json, path)?.map(|v| apply_scale(v, scale)),
        None => None,
    };
    let remaining = match &field_map.remaining {
        Some(path) => extract_field(&json, path)?.map(|v| apply_scale(v, scale)),
        None => match (&total, &used) {
            (Some(t), Some(u)) => Some(t - u),
            _ => None,
        },
    };

    let total_budget = total;
    let total_used = used.unwrap_or(0.0);

    Ok(UsageData {
        total_used,
        total_budget,
        remaining,
        currency: field_map.currency.clone(),
        period_start: None,
        period_end: None,
    })
}

/// DeepSeek 余额查询（/user/balance 的特化处理，修复 L15）
///
/// DeepSeek 响应的 `balance_infos` 是按币种分条的数组，每个条目自带 `currency`
/// 字段（国内账号为 CNY，海外账号为 USD，理论上可同时存在多条）：
///
/// ```json
/// { "is_available": true,
///   "balance_infos": [
///     { "currency": "CNY", "total_balance": "10.00", ... },
///     { "currency": "USD", "total_balance": "5.00", ... }
///   ] }
/// ```
///
/// 通用 BalanceFieldMap 模板只能写死 `$.balance_infos[0]` + 固定币种，
/// 对 CNY 账号会把人民币余额标成美元。这里改为：
/// 1. 优先取 `currency == "CNY"` 的条目（存在时），否则取数组第一项
/// 2. 币种使用该条目里的实际 `currency` 值
///
/// URL 仍来自 registry 模板（调用方透传），认证固定 Bearer（与模板一致）。
pub async fn execute_deepseek_balance_query(
    client: &Client,
    url: &str,
    api_key: &str,
) -> Result<UsageData, ProviderError> {
    let resp = client
        .get(url)
        .bearer_auth(api_key)
        .send()
        .await
        .map_err(|e| {
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

    let status = resp.status();
    if status.as_u16() == 401 || status.as_u16() == 403 {
        return Err(ProviderError::AuthError(format!(
            "认证失败 (HTTP {})",
            status.as_u16()
        )));
    }
    if status.as_u16() == 429 {
        return Err(ProviderError::RateLimited("请求过于频繁".to_string()));
    }
    if !status.is_success() {
        return Err(ProviderError::RequestError(format!("HTTP {}", status)));
    }

    // bytes-then-parse 模式（与 execute_balance_query 一致，区分读体错和解析错）
    let body_bytes = resp
        .bytes()
        .await
        .map_err(|e| ProviderError::RequestError(format!("读取响应体失败: {}", e)))?;

    let json: Value = serde_json::from_slice(&body_bytes)
        .map_err(|e| ProviderError::ParseError(format!("解析 JSON 失败: {}", e)))?;

    let entries = json
        .get("balance_infos")
        .and_then(|v| v.as_array())
        .ok_or_else(|| ProviderError::ParseError("响应中无 balance_infos".to_string()))?;

    // 优先 CNY 条目，否则取第一项；空数组报解析错
    let entry = entries
        .iter()
        .find(|item| item.get("currency").and_then(|c| c.as_str()) == Some("CNY"))
        .or_else(|| entries.first())
        .ok_or_else(|| ProviderError::ParseError("balance_infos 为空".to_string()))?;

    // total_balance 在响应里是字符串形式的数字
    let balance = entry
        .get("total_balance")
        .and_then(|v| match v {
            Value::Number(n) => n.as_f64(),
            Value::String(s) => s.trim().parse::<f64>().ok(),
            _ => None,
        })
        .ok_or_else(|| ProviderError::ParseError("条目中无 total_balance".to_string()))?;

    // 币种取条目实际值，缺失时回退 USD（DeepSeek 老账号默认美元计费）
    let currency = entry
        .get("currency")
        .and_then(|c| c.as_str())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("USD")
        .to_string();

    Ok(UsageData {
        total_used: 0.0,
        total_budget: Some(balance),
        remaining: Some(balance),
        currency,
        period_start: None,
        period_end: None,
    })
}

/// 把 URL 中的动态时间占位符替换为当前时间值
///
/// 支持的占位符：
/// - `{{now_ts}}`：当前 Unix 时间戳（秒）
/// - `{{month_start_ts}}`：本月 1 号 0 点的 Unix 时间戳（秒）
/// - `{{today_date}}`：今天的日期，格式 YYYY-MM-DD
/// - `{{month_start_date}}`：本月 1 号的日期，格式 YYYY-MM-DD
fn resolve_time_placeholders(url: &str) -> String {
    use chrono::{Datelike, Timelike, Utc};

    let now = Utc::now();
    let now_ts = now.timestamp().to_string();

    let month_start_ts = now
        .with_day(1)
        .and_then(|d| d.with_hour(0))
        .and_then(|d| d.with_minute(0))
        .and_then(|d| d.with_second(0))
        .and_then(|d| d.with_nanosecond(0))
        .unwrap_or(now)
        .timestamp()
        .to_string();

    let today_date = now.format("%Y-%m-%d").to_string();
    let month_start_date = now
        .with_day(1)
        .map(|d| d.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| now.format("%Y-%m-%d").to_string());

    url.replace("{{now_ts}}", &now_ts)
        .replace("{{month_start_ts}}", &month_start_ts)
        .replace("{{today_date}}", &today_date)
        .replace("{{month_start_date}}", &month_start_date)
}

/// 给 reqwest 请求应用认证方案
fn apply_auth(
    mut builder: reqwest::RequestBuilder,
    auth: &AuthScheme,
    api_key: &str,
) -> reqwest::RequestBuilder {
    match auth {
        AuthScheme::Bearer => {
            builder = builder.bearer_auth(api_key);
        }
        AuthScheme::XApiKey => {
            builder = builder.header("x-api-key", api_key);
            // Anthropic 额外需要 anthropic-version header
            builder = builder.header("anthropic-version", "2023-06-01");
        }
        AuthScheme::RawKey => {
            builder = builder.header("Authorization", api_key);
        }
        AuthScheme::Custom(headers) => {
            for (key, value) in headers {
                // value 中的 {{apiKey}} 占位符替换为实际 key
                let resolved = value.replace("{{apiKey}}", api_key);
                builder = builder.header(key.as_str(), resolved);
            }
        }
    }
    builder
}

/// 用 JSONPath 从 JSON 提取 f64 字段
///
/// 使用 jsonpath-rust crate（0.7.x）。支持简单路径如 "$.data.total" 和
/// 数组索引 "$.balance_infos[0].total_balance"。
/// 返回 None 表示字段不存在（非错误）。
fn extract_field(json: &Value, path: &str) -> Result<Option<f64>, ProviderError> {
    use jsonpath_rust::JsonPath;

    let jp = JsonPath::try_from(path)
        .map_err(|e| ProviderError::ParseError(format!("无效的 JSONPath '{}': {}", path, e)))?;

    // find_slice 返回 Vec<JsonPathValue<Value>>，空表示字段不存在
    let results = jp.find_slice(json);

    // 过滤掉 NoValue
    let value = results.into_iter().find(|v| v.has_value());
    let value = match value {
        Some(v) => v.to_data(),
        None => return Ok(None),
    };

    // 支持 number / string 形式的数字
    let num = match &value {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.parse::<f64>().ok(),
        _ => None,
    };

    Ok(num)
}

/// 把提取出来的数值乘以 scale（如 Novita 的 0.0001）
///
/// scale 为 None 时不换算，直接返回原值。
fn apply_scale(value: f64, scale: Option<f64>) -> f64 {
    scale.map_or(value, |s| value * s)
}
