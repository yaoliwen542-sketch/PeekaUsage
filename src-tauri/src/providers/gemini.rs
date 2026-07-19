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
//! 修复 M8：刷新得到的新 access_token 会缓存进应用自己的 KeyStore
//! （键名 `gemini_access_token_cache`，含过期时间与可能的轮换 refresh_token），
//! 下次轮询优先使用未过期的缓存 token，不再每次都打 Google token 端点。
//! 注意：**不会回写用户的 `~/.gemini/oauth_creds.json`**——该文件同时被
//! Gemini CLI 使用，回写有冲突风险。
//!
//! 与 subscription.rs 的接口：`fetch_gemini_quota` 接收完整的 oauth_creds.json
//! 文本（作为 `oauth_token` 参数传入），返回 `SubscriptionUsage`。

use reqwest::Client;
use serde::{Deserialize, Serialize};

use super::types::{window_labels, ProviderStatus, SubscriptionUsage, SubscriptionWindow};
use crate::config::encryption::KeyStore;

/// Gemini CLI 的 OAuth client_id（公开固定值，来自 cc-switch 调研）
const GEMINI_CLIENT_ID: &str = "32555940559.apps.googleusercontent.com";
/// Gemini CLI 的 OAuth client_secret（公开固定值）
const GEMINI_CLIENT_SECRET: &str = "ZmssLNjJy2998hD4CTg2ejr2";

/// 修复 M8：刷新后的 token 在 KeyStore 中的缓存键名
const GEMINI_TOKEN_CACHE_KEY: &str = "gemini_access_token_cache";
/// 过期判定安全余量：剩余有效期不足该秒数即视为过期，避免用"马上过期"的 token 发请求
const TOKEN_EXPIRY_SKEW_SECONDS: i64 = 60;

/// Gemini OAuth 凭据（~/.gemini/oauth_creds.json 的结构）
#[derive(Debug, Deserialize)]
struct GeminiOauthCreds {
    access_token: String,
    refresh_token: Option<String>,
    /// ISO 8601 过期时间，如 "2026-07-17T10:00:00Z"
    expiry: Option<String>,
}

/// 修复 M8：KeyStore 中缓存的 token 结构（JSON 序列化后整体存入）
#[derive(Debug, Clone, Serialize, Deserialize)]
struct GeminiTokenCache {
    access_token: String,
    /// RFC3339 过期时间（由 refresh 响应的 expires_in 换算）
    expiry: Option<String>,
    /// Google 若在 refresh 响应里轮换了 refresh_token，缓存最新值
    refresh_token: Option<String>,
}

/// refresh 成功后的结果
struct RefreshedToken {
    access_token: String,
    expiry: Option<String>,
    /// 响应里带的新 refresh_token；未带则沿用发起本次刷新的那个
    refresh_token: Option<String>,
}

/// 查询 Gemini 配额
///
/// `oauth_creds_json` 是 `~/.gemini/oauth_creds.json` 的完整内容（含
/// access_token + refresh_token + expiry）。整个 JSON 通过订阅的 oauth_token
/// 字段透传进来，由本模块解析。
///
/// `key_store` 用于缓存刷新后的 token（修复 M8）；传 None 时退化为
/// 每次过期都重新 refresh 的旧行为（主要用于测试）。
pub async fn fetch_gemini_quota(
    client: &Client,
    oauth_creds_json: &str,
    key_store: Option<&KeyStore>,
) -> SubscriptionUsage {
    // 1. 解析凭据
    let creds: GeminiOauthCreds = match serde_json::from_str(oauth_creds_json) {
        Ok(c) => c,
        Err(e) => return error_usage(format!("解析 Gemini 凭据失败: {}", e)),
    };

    // 2. 解析可用的 access_token（文件 token -> 缓存 token -> refresh，修复 M8）
    let token = match resolve_access_token(client, &creds, key_store).await {
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

/// 解析当前可用的 access_token。
///
/// 优先级（修复 M8）：
/// 1. 文件里的 token 未过期 -> 直接用（Gemini CLI 是凭据的"源头"，
///    用户重新登录 CLI 后能立即感知账号切换）
/// 2. KeyStore 缓存的 token 未过期 -> 直接用（上次轮询刷新过，
///    避免每 5 分钟都打一次 Google token 端点）
/// 3. 两者都过期 -> refresh；refresh_token 优先用缓存里的最新值
///    （Google 可能轮换 refresh_token，本地文件里的旧值可能已失效），
///    缓存值失败再回退文件里的值；成功后把新 token 写回 KeyStore 缓存
async fn resolve_access_token(
    client: &Client,
    creds: &GeminiOauthCreds,
    key_store: Option<&KeyStore>,
) -> Result<String, String> {
    let cache = load_token_cache(key_store).await;

    if !token_expired(creds.expiry.as_deref()) {
        return Ok(creds.access_token.clone());
    }

    if let Some(cache) = cache.as_ref() {
        if !token_expired(cache.expiry.as_deref()) {
            return Ok(cache.access_token.clone());
        }
    }

    // 候选 refresh_token：缓存的最新值优先，文件值兜底，去重后依次尝试
    let mut candidates: Vec<String> = Vec::new();
    if let Some(rt) = cache
        .as_ref()
        .and_then(|c| c.refresh_token.as_ref())
        .filter(|s| !s.is_empty())
    {
        candidates.push(rt.clone());
    }
    if let Some(rt) = creds.refresh_token.as_ref().filter(|s| !s.is_empty()) {
        if !candidates.contains(rt) {
            candidates.push(rt.clone());
        }
    }
    if candidates.is_empty() {
        return Err("无 refresh_token，无法刷新".to_string());
    }

    let mut last_error = String::new();
    for refresh_token in &candidates {
        match refresh_access_token(client, refresh_token).await {
            Ok(refreshed) => {
                let access_token = refreshed.access_token.clone();
                save_token_cache(key_store, &refreshed).await;
                return Ok(access_token);
            }
            Err(error) => {
                last_error = error;
            }
        }
    }

    Err(last_error)
}

/// 判断 token 是否已过期（含安全余量）。
///
/// 无 expiry 或 expiry 解析失败一律视为已过期（与原行为一致）。
fn token_expired(expiry: Option<&str>) -> bool {
    let Some(expiry) = expiry else {
        return true;
    };
    chrono::DateTime::parse_from_rfc3339(expiry)
        .map(|dt| {
            dt.with_timezone(&chrono::Utc)
                < chrono::Utc::now() + chrono::Duration::seconds(TOKEN_EXPIRY_SKEW_SECONDS)
        })
        .unwrap_or(true)
}

/// 从 KeyStore 读取缓存的 token；缓存缺失或损坏时静默视为无缓存
async fn load_token_cache(key_store: Option<&KeyStore>) -> Option<GeminiTokenCache> {
    let raw = key_store?.get_stored_key(GEMINI_TOKEN_CACHE_KEY).await?;
    serde_json::from_str(&raw).ok()
}

/// 把刷新后的 token 写入 KeyStore 缓存。
///
/// 只写应用自己的 KeyStore，不回写用户的 `~/.gemini/oauth_creds.json`。
/// 写入失败只打日志，不影响本次查询（下次轮询会重新 refresh）。
async fn save_token_cache(key_store: Option<&KeyStore>, refreshed: &RefreshedToken) {
    let Some(key_store) = key_store else {
        return;
    };

    let cache = GeminiTokenCache {
        access_token: refreshed.access_token.clone(),
        expiry: refreshed.expiry.clone(),
        refresh_token: refreshed.refresh_token.clone(),
    };

    match serde_json::to_string(&cache) {
        Ok(raw) => {
            if let Err(error) = key_store.set_key(GEMINI_TOKEN_CACHE_KEY, &raw).await {
                eprintln!("缓存 Gemini token 失败: {}", error);
            }
        }
        Err(error) => eprintln!("序列化 Gemini token 缓存失败: {}", error),
    }
}

/// 用 refresh_token 向 Google token 端点换取新 access_token
async fn refresh_access_token(
    client: &Client,
    refresh_token: &str,
) -> Result<RefreshedToken, String> {
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

    let access_token = token_resp
        .get("access_token")
        .and_then(|t| t.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "响应中无 access_token".to_string())?;

    // Google 返回 expires_in（秒），换算成绝对过期时间（RFC3339）存缓存
    let expiry = token_resp
        .get("expires_in")
        .and_then(|v| v.as_i64())
        .map(|seconds| (chrono::Utc::now() + chrono::Duration::seconds(seconds)).to_rfc3339());

    // Google 偶尔会在 refresh 响应里轮换 refresh_token，有则缓存最新值，
    // 没有则沿用本次使用的 refresh_token
    let next_refresh_token = token_resp
        .get("refresh_token")
        .and_then(|t| t.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .or_else(|| Some(refresh_token.to_string()));

    Ok(RefreshedToken {
        access_token,
        expiry,
        refresh_token: next_refresh_token,
    })
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
    //
    // 修复 L16：用 BTreeMap 代替 HashMap，窗口输出顺序稳定（按 modelId 字典序），
    // 不再每次刷新随机跳动；最后按 label 再稳定排序一次，同类窗口聚集。
    use std::collections::BTreeMap;
    let mut model_min: BTreeMap<String, f64> = BTreeMap::new();
    let mut model_reset: BTreeMap<String, String> = BTreeMap::new();

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
        let label = gemini_window_label(model);
        let resets_at = model_reset.get(model).filter(|s| !s.is_empty()).cloned();
        windows.push(SubscriptionWindow {
            label,
            utilization,
            resets_at,
        });
    }

    // 按 label 字典序稳定排序（同 label 内保持 modelId 字典序），输出顺序固定
    windows.sort_by(|left, right| left.label.cmp(&right.label));

    Ok(SubscriptionUsage {
        plan_name: Some("Gemini".into()),
        windows,
        extra_usage: None,
        status: ProviderStatus::Success,
        error_message: None,
    })
}

/// Gemini 配额窗口的机器常量标签（修复 L16）
///
/// 真实 modelId 带版本号（如 "gemini-2.5-pro" / "gemini-2.0-flash" /
/// "gemini-2.5-pro-preview-06-05"），精确匹配 "gemini-pro" / "gemini-flash"
/// 永远命中不了。改为大小写不敏感的包含匹配：
/// 含 "pro" -> seven_day，含 "flash" -> five_hour，其它保留原始 modelId。
fn gemini_window_label(model_id: &str) -> String {
    let lower = model_id.to_lowercase();
    if lower.contains("pro") {
        window_labels::SEVEN_DAY.to_string()
    } else if lower.contains("flash") {
        window_labels::FIVE_HOUR.to_string()
    } else {
        model_id.to_string()
    }
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

    #[test]
    fn test_token_expired_logic() {
        // 无 expiry / 非法 expiry -> 视为过期
        assert!(token_expired(None));
        assert!(token_expired(Some("not-a-date")));

        // 未来 1 小时 -> 未过期
        let future = (chrono::Utc::now() + chrono::Duration::hours(1)).to_rfc3339();
        assert!(!token_expired(Some(&future)));

        // 过去 1 小时 -> 已过期
        let past = (chrono::Utc::now() - chrono::Duration::hours(1)).to_rfc3339();
        assert!(token_expired(Some(&past)));

        // 30 秒后过期（小于 60 秒安全余量）-> 视为已过期
        let soon = (chrono::Utc::now() + chrono::Duration::seconds(30)).to_rfc3339();
        assert!(token_expired(Some(&soon)));
    }

    #[test]
    fn test_gemini_window_label_versioned_model_ids() {
        // 真实 modelId 带版本号：包含匹配命中机器常量
        assert_eq!(gemini_window_label("gemini-2.5-pro"), "seven_day");
        assert_eq!(
            gemini_window_label("gemini-2.5-pro-preview-06-05"),
            "seven_day"
        );
        assert_eq!(gemini_window_label("gemini-2.0-flash"), "five_hour");
        assert_eq!(gemini_window_label("Gemini-2.0-Flash-Lite"), "five_hour");
        // 无法归类的保留原始 modelId
        assert_eq!(
            gemini_window_label("gemini-embedding-001"),
            "gemini-embedding-001"
        );
        assert_eq!(gemini_window_label("unknown"), "unknown");
    }

    #[test]
    fn test_gemini_token_cache_serde_roundtrip() {
        let cache = GeminiTokenCache {
            access_token: "at".to_string(),
            expiry: Some("2026-07-17T10:00:00+00:00".to_string()),
            refresh_token: Some("rt".to_string()),
        };
        let raw = serde_json::to_string(&cache).unwrap();
        let parsed: GeminiTokenCache = serde_json::from_str(&raw).unwrap();
        assert_eq!(parsed.access_token, "at");
        assert_eq!(parsed.refresh_token.as_deref(), Some("rt"));

        // 损坏的缓存 JSON 解析失败 -> 调用方按无缓存处理
        assert!(serde_json::from_str::<GeminiTokenCache>("{broken").is_err());
    }
}
