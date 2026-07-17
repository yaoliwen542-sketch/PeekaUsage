//! Gemini（Google Cloud Code Assist）订阅用量查询
//!
//! 两步 OAuth 流程：
//! 1. `loadCodeAssist` 拿 projectId
//! 2. `retrieveUserQuota` 拿配额（按 modelId 分组的 buckets）
//!
//! 凭据来源：`~/.gemini/oauth_creds.json`，含 `access_token` / `refresh_token` / `expiry`。
//! 当 token 过期时，用 Gemini CLI 公开的 client_id / client_secret 调用
//! `https://oauth2.googleapis.com/token` 刷新。
//!
//! 与 subscription.rs 的接口：`fetch_gemini_quota` 接收完整的 oauth_creds.json
//! 文本（作为 `oauth_token` 参数传入），返回 `SubscriptionUsage`。

use reqwest::Client;
use serde::Deserialize;

use super::types::{window_labels, ProviderStatus, SubscriptionUsage, SubscriptionWindow};

/// Gemini CLI 的 OAuth client_id（公开固定值，来自 cc-switch 调研）
const GEMINI_CLIENT_ID: &str = "32555940559.apps.googleusercontent.com";
/// Gemini CLI 的 OAuth client_secret（公开固定值）
const GEMINI_CLIENT_SECRET: &str = "ZmssLNjJy2998hD4CTg2ejr2";

/// Gemini OAuth 凭据（~/.gemini/oauth_creds.json 的结构）
#[derive(Debug, Deserialize)]
struct GeminiOauthCreds {
    access_token: String,
    refresh_token: Option<String>,
    /// ISO 8601 过期时间，如 "2026-07-17T10:00:00Z"
    expiry: Option<String>,
}

/// 查询 Gemini 配额
///
/// `oauth_creds_json` 是 `~/.gemini/oauth_creds.json` 的完整内容（含
/// access_token + refresh_token + expiry）。整个 JSON 通过订阅的 oauth_token
/// 字段透传进来，由本模块解析。
pub async fn fetch_gemini_quota(client: &Client, oauth_creds_json: &str) -> SubscriptionUsage {
    // 1. 解析凭据
    let creds: GeminiOauthCreds = match serde_json::from_str(oauth_creds_json) {
        Ok(c) => c,
        Err(e) => return error_usage(format!("解析 Gemini 凭据失败: {}", e)),
    };

    // 2. 检查 token 是否过期，过期则 refresh
    let token = match refresh_if_needed(client, &creds).await {
        Ok(t) => t,
        Err(e) => return error_usage(format!("刷新 token 失败: {}", e)),
    };

    // 3. loadCodeAssist 拿 projectId
    let project_id = match load_code_assist(client, &token).await {
        Ok(id) => id,
        Err(e) => return error_usage(format!("获取 projectId 失败: {}", e)),
    };

    // 4. retrieveUserQuota 拿配额
    match retrieve_user_quota(client, &token, &project_id).await {
        Ok(usage) => usage,
        Err(e) => error_usage(format!("查询配额失败: {}", e)),
    }
}

/// 若 access_token 已过期（或无 expiry 视为已过期），则用 refresh_token 换取新 token。
///
/// 成功返回可用的 access_token（原 token 或刷新后的新 token）。
async fn refresh_if_needed(client: &Client, creds: &GeminiOauthCreds) -> Result<String, String> {
    let need_refresh = creds
        .expiry
        .as_ref()
        .map(|exp| {
            chrono::DateTime::parse_from_rfc3339(exp)
                .map(|dt| dt.with_timezone(&chrono::Utc) < chrono::Utc::now())
                .unwrap_or(true)
        })
        .unwrap_or(true);

    if !need_refresh {
        return Ok(creds.access_token.clone());
    }

    let refresh_token = creds
        .refresh_token
        .as_ref()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "无 refresh_token，无法刷新".to_string())?;

    let resp = client
        .post("https://oauth2.googleapis.com/token")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(urlencoded_form_body(&[
            ("grant_type", "refresh_token"),
            ("client_id", GEMINI_CLIENT_ID),
            ("client_secret", GEMINI_CLIENT_SECRET),
            ("refresh_token", refresh_token),
        ]))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }

    let token_resp: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    token_resp
        .get("access_token")
        .and_then(|t| t.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "响应中无 access_token".to_string())
}

/// loadCodeAssist：拿到 Gemini 的 cloudaicompanionProject.id
///
/// 响应结构存在多种可能（`cloudaicompanionProject.id` / `project.id` / 顶层 `id`），
/// 这里用 serde_json::Value 宽松解析，按优先级回退。
async fn load_code_assist(client: &Client, token: &str) -> Result<String, String> {
    let resp = client
        .post("https://cloudcode-pa.googleapis.com/v1internal:loadCodeAssist")
        .header("Authorization", format!("Bearer {}", token))
        .json(&serde_json::json!({
            "metadata": {"ideType": "GEMINI_CLI", "pluginType": "GEMINI"}
        }))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if resp.status().as_u16() == 401 || resp.status().as_u16() == 403 {
        return Err(format!("OAuth Token 无效或已过期 (HTTP {})", resp.status()));
    }
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }

    let body: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    // 兼容多种响应结构
    body.get("cloudaicompanionProject")
        .and_then(|p| p.get("id"))
        .or_else(|| body.get("project").and_then(|p| p.get("id")))
        .or_else(|| body.get("id"))
        .and_then(|id| id.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "响应中无 projectId".to_string())
}

/// retrieveUserQuota：拿到按 modelId 分组的 buckets，组装成 SubscriptionUsage
///
/// 每个 bucket 含 `modelId` / `remainingFraction` / `resetTime`。
/// 按 modelId 聚合，每类取最低 remainingFraction（即最高利用率），
/// utilization = (1 - remainingFraction) * 100。
async fn retrieve_user_quota(
    client: &Client,
    token: &str,
    project_id: &str,
) -> Result<SubscriptionUsage, String> {
    let resp = client
        .post("https://cloudcode-pa.googleapis.com/v1internal:retrieveUserQuota")
        .header("Authorization", format!("Bearer {}", token))
        .json(&serde_json::json!({"project": {"id": project_id}}))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if resp.status().as_u16() == 401 || resp.status().as_u16() == 403 {
        return Err(format!("OAuth Token 无效或已过期 (HTTP {})", resp.status()));
    }
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }

    let body: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let buckets = body
        .get("buckets")
        .and_then(|b| b.as_array())
        .ok_or_else(|| "响应中无 buckets".to_string())?;

    // 按 modelId 分组，每类取最低 remainingFraction（最高利用率）
    use std::collections::HashMap;
    let mut model_min: HashMap<String, f64> = HashMap::new();
    let mut model_reset: HashMap<String, String> = HashMap::new();

    for bucket in buckets {
        let model_id = bucket
            .get("modelId")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown");
        let remaining = bucket
            .get("remainingFraction")
            .and_then(|r| r.as_f64())
            .unwrap_or(1.0);
        let reset = bucket
            .get("resetTime")
            .and_then(|t| t.as_str())
            .unwrap_or("");

        let entry = model_min.entry(model_id.to_string()).or_insert(1.0);
        if remaining < *entry {
            *entry = remaining;
            model_reset.insert(model_id.to_string(), reset.to_string());
        }
    }

    let mut windows = Vec::new();
    for (model, min_remaining) in &model_min {
        let utilization = ((1.0 - min_remaining) * 100.0).clamp(0.0, 100.0);
        let label = match model.as_str() {
            "gemini-pro" => window_labels::SEVEN_DAY.to_string(),
            "gemini-flash" => window_labels::FIVE_HOUR.to_string(),
            other => other.to_string(),
        };
        let resets_at = model_reset.get(model).filter(|s| !s.is_empty()).cloned();
        windows.push(SubscriptionWindow {
            label,
            utilization,
            resets_at,
        });
    }

    Ok(SubscriptionUsage {
        plan_name: Some("Gemini".into()),
        windows,
        extra_usage: None,
        status: ProviderStatus::Success,
        error_message: None,
    })
}

/// 构造一个错误状态的 SubscriptionUsage
fn error_usage(msg: String) -> SubscriptionUsage {
    SubscriptionUsage {
        plan_name: None,
        windows: vec![],
        extra_usage: None,
        status: ProviderStatus::Error,
        error_message: Some(msg),
    }
}

/// 手动拼接 application/x-www-form-urlencoded body。
///
/// reqwest 的 `.form()` 需要 `form` feature（当前 Cargo.toml 未启用），
/// 这里手写一份最小实现：对 key/value 做 percent-encoding 后用 `=` 和 `&` 连接。
/// 仅用于固定几个 ASCII 字段（client_id / client_secret / grant_type / refresh_token），
/// refresh_token 虽然是 OAuth 凭据可能含非 ASCII，故对值做完整 percent-encoding。
fn urlencoded_form_body(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| format!("{}={}", percent_encode(k), percent_encode(v)))
        .collect::<Vec<_>>()
        .join("&")
}

/// RFC 3986 unreserved 集合的 percent-encoding
fn percent_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for &b in input.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{:02X}", b));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_percent_encode_ascii_unreserved() {
        assert_eq!(percent_encode("abc-._~"), "abc-._~");
    }

    #[test]
    fn test_percent_encode_special_chars() {
        // 空格编码为 %20
        assert_eq!(percent_encode("a b"), "a%20b");
        // & 编码
        assert_eq!(percent_encode("a&b"), "a%26b");
        // = 编码
        assert_eq!(percent_encode("a=b"), "a%3Db");
    }

    #[test]
    fn test_urlencoded_form_body_basic() {
        let body = urlencoded_form_body(&[("grant_type", "refresh_token"), ("client_id", "cid")]);
        assert_eq!(body, "grant_type=refresh_token&client_id=cid");
    }

    #[test]
    fn test_urlencoded_form_body_encodes_values() {
        // refresh_token 含特殊字符时需编码
        let body = urlencoded_form_body(&[("refresh_token", "rt with space")]);
        assert_eq!(body, "refresh_token=rt%20with%20space");
    }

    #[test]
    fn test_gemini_oauth_creds_parse_full() {
        let json = r#"{"access_token":"at","refresh_token":"rt","expiry":"2026-07-17T10:00:00Z"}"#;
        let creds: GeminiOauthCreds = serde_json::from_str(json).unwrap();
        assert_eq!(creds.access_token, "at");
        assert_eq!(creds.refresh_token.as_deref(), Some("rt"));
        assert_eq!(creds.expiry.as_deref(), Some("2026-07-17T10:00:00Z"));
    }

    #[test]
    fn test_gemini_oauth_creds_parse_minimal() {
        // refresh_token / expiry 都可缺失
        let json = r#"{"access_token":"at"}"#;
        let creds: GeminiOauthCreds = serde_json::from_str(json).unwrap();
        assert_eq!(creds.access_token, "at");
        assert!(creds.refresh_token.is_none());
        assert!(creds.expiry.is_none());
    }

    #[test]
    fn test_gemini_oauth_creds_parse_missing_access_token_fails() {
        let json = r#"{"refresh_token":"rt"}"#;
        let result: Result<GeminiOauthCreds, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }
}
