//! OAuth 凭据自动检测
//!
//! 从本地凭据文件读取 OAuth token / account_id，供：
//! - 订阅查询链路在用户未手动填写 token 时自动补全（如 `fetch_openai_wham` 的 `account_id` 回退）
//! - 设置页"自动检测"按钮填充输入框（通过 `commands::window_commands::detect_oauth_tokens`）
//!
//! 与 `commands::window_commands` 的关系：
//! - `window_commands::detect_oauth_tokens` 是面向前端的 Tauri 命令，返回带 environment/source
//!   元信息的 `DetectedTokens`（支持 Windows / WSL 双环境枚举），供用户在 UI 上挑选。
//! - 本模块是纯函数式底层能力：同步读单个文件、返回 `(token, account_id)`，无元信息，
//!   供后端在请求时直接调用。两条链路读取的是同一批凭据文件，解析逻辑保持一致。

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::Value;

/// OAuth 自动检测结果
#[derive(Debug, Clone)]
pub struct DetectedOAuth {
    /// OAuth access token
    pub token: String,
    /// OpenAI/Codex 的 account_id（用于 `ChatGPT-Account-Id` header）；Anthropic 恒为 None
    pub account_id: Option<String>,
    /// 可读的来源描述（如 "Claude Code (~/.claude/.credentials.json)"），便于日志/调试
    pub source: String,
}

/// 检测 Anthropic OAuth token
///
/// 读取顺序：
/// 1. `~/.claude/.credentials.json` 的 `claudeAiOauth.accessToken`（兼容旧 key `claude.ai_oauth`）
/// 2. macOS Keychain（service="Claude Code-credentials"，仅 macos）
///
/// 注意：WSL 凭据检测由 `commands::window_commands::detect_oauth_tokens` 负责（需要调 wsl.exe），
/// 本函数仅处理当前进程所在系统的原生凭据文件。
pub fn detect_anthropic() -> Option<DetectedOAuth> {
    // 1. 读 ~/.claude/.credentials.json
    if let Some(creds_path) = claude_credentials_path() {
        if let Some(detected) = detect_anthropic_from_file(&creds_path) {
            return Some(detected);
        }
    }

    // 2. macOS Keychain（仅在 macos 上）
    #[cfg(target_os = "macos")]
    {
        if let Some(token) = read_keychain("Claude Code-credentials") {
            return Some(DetectedOAuth {
                token,
                account_id: None,
                source: "macOS Keychain (Claude Code-credentials)".to_string(),
            });
        }
    }

    None
}

/// 从指定 credentials.json 文件读取 Anthropic OAuth token（纯函数，便于测试）
fn detect_anthropic_from_file(creds_path: &std::path::Path) -> Option<DetectedOAuth> {
    let content = std::fs::read_to_string(creds_path).ok()?;
    let json = serde_json::from_str::<Value>(&content).ok()?;
    // 兼容 claudeAiOauth 和 claude.ai_oauth 两种 key（前者是当前 Claude Code 的写法）
    let oauth = json
        .get("claudeAiOauth")
        .or_else(|| json.get("claude.ai_oauth"));
    let token = oauth
        .and_then(|o| o.get("accessToken"))
        .and_then(|t| t.as_str())
        .filter(|t| !t.is_empty())?;
    Some(DetectedOAuth {
        token: token.to_string(),
        account_id: None,
        source: "Claude Code (~/.claude/.credentials.json)".to_string(),
    })
}

/// 检测 OpenAI/Codex OAuth token
///
/// 读取 `~/.codex/auth.json`，仅当 `auth_mode == "chatgpt"` 时有效。
/// 返回 `tokens.access_token` + `tokens.account_id`。
///
/// `tokens.access_token` 可能是字符串或索引对象（`{"0":"a","1":"b",...}`），
/// 必须同时兼容（参考 AGENTS.md 第 1 点 / `window_commands::parse_codex_access_token`）。
pub fn detect_openai() -> Option<DetectedOAuth> {
    let creds_path = codex_auth_path()?;
    detect_openai_from_file(&creds_path)
}

/// 从指定 auth.json 文件读取 OpenAI/Codex OAuth token（纯函数，便于测试）
fn detect_openai_from_file(creds_path: &std::path::Path) -> Option<DetectedOAuth> {
    let content = std::fs::read_to_string(creds_path).ok()?;
    let json = serde_json::from_str::<Value>(&content).ok()?;

    // 检查 auth_mode（仅 chatgpt 模式才有可用的 ChatGPT OAuth token）
    let auth_mode = json.get("auth_mode").and_then(|m| m.as_str());
    if auth_mode != Some("chatgpt") {
        return None;
    }

    let tokens = json.get("tokens")?;
    let token = tokens
        .get("access_token")
        .and_then(|v| parse_codex_access_token(v))?;
    let account_id = tokens
        .get("account_id")
        .and_then(|a| a.as_str())
        .map(|s| s.to_string());

    Some(DetectedOAuth {
        token,
        account_id,
        source: "Codex CLI (~/.codex/auth.json)".to_string(),
    })
}

/// `~/.claude/.credentials.json` 路径
fn claude_credentials_path() -> Option<PathBuf> {
    home_dir().map(|h| h.join(".claude").join(".credentials.json"))
}

/// `~/.codex/auth.json` 路径
fn codex_auth_path() -> Option<PathBuf> {
    home_dir().map(|h| h.join(".codex").join("auth.json"))
}

/// 获取用户 home 目录
///
/// 与 `commands::window_commands::dirs_next` 保持一致：优先 USERPROFILE（Windows），
/// 回退 HOME（Linux/macOS）。不依赖 `dirs` crate，避免新增依赖。
fn home_dir() -> Option<PathBuf> {
    std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .ok()
        .map(PathBuf::from)
}

/// 把 `{ "0": "a", "1": "b", ... }` 格式的对象转为字符串
///
/// 复用 `commands::window_commands::indexed_object_to_string` 的逻辑（保持两边解析一致）。
fn indexed_object_to_string(map: &BTreeMap<String, Value>) -> String {
    let mut entries: Vec<(usize, &str)> = map
        .iter()
        .filter_map(|(k, v)| {
            let idx = k.parse::<usize>().ok()?;
            let ch = v.as_str()?;
            Some((idx, ch))
        })
        .collect();
    entries.sort_by_key(|(idx, _)| *idx);
    entries.iter().map(|(_, ch)| *ch).collect()
}

/// 解析 Codex `tokens.access_token`：兼容字符串和索引对象两种格式
fn parse_codex_access_token(value: &Value) -> Option<String> {
    match value {
        Value::String(token) if !token.is_empty() => Some(token.clone()),
        Value::Object(map) => {
            let ordered: BTreeMap<String, Value> =
                map.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
            let token = indexed_object_to_string(&ordered);
            if token.is_empty() {
                None
            } else {
                Some(token)
            }
        }
        _ => None,
    }
}

/// macOS Keychain 读取（仅 macos 编译）
#[cfg(target_os = "macos")]
fn read_keychain(service: &str) -> Option<String> {
    use std::process::Command;
    let output = Command::new("security")
        .args(["find-generic-password", "-s", service, "-w"])
        .output()
        .ok()?;
    if output.status.success() {
        let token = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !token.is_empty() {
            return Some(token);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_codex_access_token_string() {
        let v = serde_json::json!("sk-abc123");
        assert_eq!(parse_codex_access_token(&v), Some("sk-abc123".to_string()));
    }

    #[test]
    fn test_parse_codex_access_token_empty_string() {
        let v = serde_json::json!("");
        assert_eq!(parse_codex_access_token(&v), None);
    }

    #[test]
    fn test_parse_codex_access_token_indexed_object() {
        let v = serde_json::json!({"0":"s","1":"k","2":"-","3":"1"});
        assert_eq!(parse_codex_access_token(&v), Some("sk-1".to_string()));
    }

    #[test]
    fn test_parse_codex_access_token_indexed_object_unordered() {
        // key 顺序乱也应该按数字序重组
        let v = serde_json::json!({"2":"-","0":"s","3":"1","1":"k"});
        assert_eq!(parse_codex_access_token(&v), Some("sk-1".to_string()));
    }

    #[test]
    fn test_parse_codex_access_token_other_types() {
        assert_eq!(parse_codex_access_token(&serde_json::json!(42)), None);
        assert_eq!(parse_codex_access_token(&serde_json::json!(null)), None);
        assert_eq!(parse_codex_access_token(&serde_json::json!(true)), None);
    }

    #[test]
    fn test_detect_openai_rejects_non_chatgpt_mode() {
        // auth_mode != "chatgpt" 时应返回 None（即使有 tokens 字段）
        let tmp = tempfile_dir();
        std::fs::create_dir_all(&tmp).ok();
        let path = tmp.join("auth.json");
        std::fs::write(
            &path,
            r#"{"auth_mode":"apikey","tokens":{"access_token":"sk-x","account_id":"acct-1"}}"#,
        )
        .ok();
        assert!(detect_openai_from_file(&path).is_none());
    }

    #[test]
    fn test_detect_openai_reads_chatgpt_token_and_account_id() {
        let tmp = tempfile_dir();
        std::fs::create_dir_all(&tmp).ok();
        let path = tmp.join("auth.json");
        std::fs::write(
            &path,
            r#"{"auth_mode":"chatgpt","tokens":{"access_token":"sk-abc","account_id":"acct-42"}}"#,
        )
        .ok();
        let detected = detect_openai_from_file(&path).expect("应检测到 token");
        assert_eq!(detected.token, "sk-abc");
        assert_eq!(detected.account_id.as_deref(), Some("acct-42"));
    }

    #[test]
    fn test_detect_openai_handles_indexed_access_token() {
        let tmp = tempfile_dir();
        std::fs::create_dir_all(&tmp).ok();
        let path = tmp.join("auth.json");
        std::fs::write(
            &path,
            r#"{"auth_mode":"chatgpt","tokens":{"access_token":{"0":"s","1":"k"},"account_id":"acct"}}"#,
        )
        .ok();
        let detected = detect_openai_from_file(&path).expect("应检测到 token");
        assert_eq!(detected.token, "sk");
        assert_eq!(detected.account_id.as_deref(), Some("acct"));
    }

    #[test]
    fn test_detect_openai_missing_tokens_returns_none() {
        let tmp = tempfile_dir();
        std::fs::create_dir_all(&tmp).ok();
        let path = tmp.join("auth.json");
        std::fs::write(&path, r#"{"auth_mode":"chatgpt"}"#).ok();
        assert!(detect_openai_from_file(&path).is_none());
    }

    #[test]
    fn test_detect_openai_missing_file_returns_none() {
        let tmp = tempfile_dir();
        std::fs::create_dir_all(&tmp).ok();
        let path = tmp.join("nope.json");
        assert!(detect_openai_from_file(&path).is_none());
    }

    #[test]
    fn test_detect_anthropic_reads_claude_ai_oauth() {
        let tmp = tempfile_dir();
        std::fs::create_dir_all(&tmp).ok();
        let path = tmp.join(".credentials.json");
        std::fs::write(
            &path,
            r#"{"claudeAiOauth":{"accessToken":"claude-tok-xyz","subscriptionType":"max"}}"#,
        )
        .ok();
        let detected = detect_anthropic_from_file(&path).expect("应检测到 token");
        assert_eq!(detected.token, "claude-tok-xyz");
        assert!(detected.account_id.is_none());
    }

    #[test]
    fn test_detect_anthropic_falls_back_to_legacy_key() {
        // 旧版 key 是 claude.ai_oauth
        let tmp = tempfile_dir();
        std::fs::create_dir_all(&tmp).ok();
        let path = tmp.join(".credentials.json");
        std::fs::write(&path, r#"{"claude.ai_oauth":{"accessToken":"legacy-tok"}}"#).ok();
        let detected = detect_anthropic_from_file(&path).expect("应检测到 token");
        assert_eq!(detected.token, "legacy-tok");
    }

    #[test]
    fn test_detect_anthropic_skips_empty_token() {
        let tmp = tempfile_dir();
        std::fs::create_dir_all(&tmp).ok();
        let path = tmp.join(".credentials.json");
        std::fs::write(&path, r#"{"claudeAiOauth":{"accessToken":""}}"#).ok();
        assert!(detect_anthropic_from_file(&path).is_none());
    }

    #[test]
    fn test_detect_anthropic_missing_file_returns_none() {
        let tmp = tempfile_dir();
        std::fs::create_dir_all(&tmp).ok();
        let path = tmp.join("nope.json");
        assert!(detect_anthropic_from_file(&path).is_none());
    }

    #[test]
    fn test_detect_anthropic_prefers_claude_ai_oauth_over_legacy() {
        // 两个 key 都存在时优先 claudeAiOauth
        let tmp = tempfile_dir();
        std::fs::create_dir_all(&tmp).ok();
        let path = tmp.join(".credentials.json");
        std::fs::write(
            &path,
            r#"{"claudeAiOauth":{"accessToken":"new-tok"},"claude.ai_oauth":{"accessToken":"old-tok"}}"#,
        )
        .ok();
        let detected = detect_anthropic_from_file(&path).expect("应检测到 token");
        assert_eq!(detected.token, "new-tok");
    }

    /// 测试用临时目录（基于 std::env::temp_dir + 唯一后缀，避免并发测试串扰）
    fn tempfile_dir() -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "peeka-oauth-detect-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        p
    }
}
