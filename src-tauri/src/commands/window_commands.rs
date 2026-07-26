use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
#[cfg(windows)]
use std::process::Command;

/// 窗口边界动画代次：新动画递增代次，旧动画任务发现代次被顶掉立即退出，
/// 保证收起/展开连续触发时动画可打断、可反向。
static WINDOW_BOUNDS_ANIMATION_GENERATION: AtomicU64 = AtomicU64::new(0);

fn ease_in_out_cubic(t: f64) -> f64 {
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
    }
}

/// 把窗口从当前边界缓动动画到目标边界（逻辑像素）。
/// 在独立任务里按 ~125fps 步进 set_size/set_position，避免 JS 侧每帧 IPC 洪泛；
/// 目标与起点一致或时长为 0 时直接落定。
#[tauri::command]
pub async fn animate_window_bounds(
    window: tauri::WebviewWindow,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    duration_ms: u64,
) -> Result<(), String> {
    let scale_factor = window.scale_factor().map_err(|e| e.to_string())?;
    let start_position = window.outer_position().map_err(|e| e.to_string())?;
    let start_size = window.inner_size().map_err(|e| e.to_string())?;

    let target_position = tauri::PhysicalPosition::new(
        (x as f64 * scale_factor).round() as i32,
        (y as f64 * scale_factor).round() as i32,
    );
    let target_size = tauri::PhysicalSize::new(
        ((width as f64 * scale_factor).round() as u32).max(1),
        ((height as f64 * scale_factor).round() as u32).max(1),
    );

    let generation = WINDOW_BOUNDS_ANIMATION_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;

    if duration_ms == 0 || (start_position == target_position && start_size == target_size) {
        window.set_size(target_size).map_err(|e| e.to_string())?;
        window
            .set_position(target_position)
            .map_err(|e| e.to_string())?;
        return Ok(());
    }

    tauri::async_runtime::spawn(async move {
        let started = Instant::now();
        let duration = Duration::from_millis(duration_ms.max(1));
        loop {
            if WINDOW_BOUNDS_ANIMATION_GENERATION.load(Ordering::SeqCst) != generation {
                // 被更新的动画顶掉：直接退出，由新动画接管
                return;
            }
            let t = (started.elapsed().as_secs_f64() / duration.as_secs_f64()).min(1.0);
            let eased = ease_in_out_cubic(t);
            let lerp_pos = |from: i32, to: i32| -> i32 {
                (from as f64 + (to - from) as f64 * eased).round() as i32
            };
            let lerp_size = |from: u32, to: u32| -> u32 {
                (from as f64 + (to as f64 - from as f64) * eased)
                    .round()
                    .max(1.0) as u32
            };
            let _ = window.set_size(tauri::PhysicalSize::new(
                lerp_size(start_size.width, target_size.width),
                lerp_size(start_size.height, target_size.height),
            ));
            let _ = window.set_position(tauri::PhysicalPosition::new(
                lerp_pos(start_position.x, target_position.x),
                lerp_pos(start_position.y, target_position.y),
            ));
            if t >= 1.0 {
                // 最终精确落定，消除四舍五入漂移
                let _ = window.set_size(target_size);
                let _ = window.set_position(target_position);
                return;
            }
            tokio::time::sleep(Duration::from_millis(8)).await;
        }
    });

    Ok(())
}

/// 自动检测本地 OAuth Token
#[tauri::command]
pub async fn detect_oauth_tokens() -> Result<DetectedTokens, String> {
    let home = dirs_next().ok_or_else(|| "无法获取用户目录".to_string())?;

    let mut result = DetectedTokens {
        anthropic: Vec::new(),
        openai: Vec::new(),
        gemini: Vec::new(),
    };

    if let Some(token) = read_claude_token_from_home(&home, "native") {
        result.anthropic.push(token);
    }

    if let Some(token) = read_codex_token_from_home(&home, "native") {
        result.openai.push(token);
    }

    // Gemini：复用 oauth_detect::detect_gemini，token 字段是完整 oauth_creds.json 文本
    // （含 refresh_token，供 subscription.rs 自动刷新）。与 Anthropic/OpenAI 不同，Gemini
    // 不区分 native/WSL 环境（oauth_detect 仅读当前进程 home），也没有多账号场景。
    if let Some(detected) = crate::providers::oauth_detect::detect_gemini() {
        result.gemini.push(DetectedToken {
            token: detected.token,
            source: detected.source.clone(),
            subscription_type: None,
            environment: "native".to_string(),
            display_source: detected.source,
            account_id: None,
        });
    }

    #[cfg(windows)]
    {
        if let Some(token) = read_wsl_claude_token() {
            result.anthropic.push(token);
        }

        if let Some(token) = read_wsl_codex_token() {
            result.openai.push(token);
        }

        for token in &mut result.anthropic {
            if token.environment == "native" {
                token.environment = "windows".to_string();
                token.display_source = format!("Windows {}", token.source);
            }
        }

        for token in &mut result.openai {
            if token.environment == "native" {
                token.environment = "windows".to_string();
                token.display_source = format!("Windows {}", token.source);
            }
        }
    }

    Ok(result)
}

fn read_claude_token_from_home(home: &std::path::Path, environment: &str) -> Option<DetectedToken> {
    let credentials_path = home.join(".claude").join(".credentials.json");
    let content = std::fs::read_to_string(&credentials_path).ok()?;
    let creds = serde_json::from_str::<ClaudeCredentials>(&content).ok()?;
    let oauth = creds.claude_ai_oauth?;
    if oauth.access_token.is_empty() {
        return None;
    }

    let source = "Claude Code (~/.claude/.credentials.json)".to_string();
    Some(DetectedToken {
        token: oauth.access_token,
        source: source.clone(),
        subscription_type: oauth.subscription_type,
        environment: environment.to_string(),
        display_source: source,
        account_id: None,
    })
}

fn read_codex_token_from_home(home: &std::path::Path, environment: &str) -> Option<DetectedToken> {
    let auth_path = home.join(".codex").join("auth.json");
    let content = std::fs::read_to_string(&auth_path).ok()?;
    let auth = serde_json::from_str::<CodexAuth>(&content).ok()?;
    // 与 oauth_detect::detect_openai 对齐：非 chatgpt 模式不视作可用 OAuth 凭据
    if auth.auth_mode.as_deref() != Some("chatgpt") {
        return None;
    }
    let tokens = auth.tokens?;
    let token = tokens
        .access_token
        .as_ref()
        .and_then(parse_codex_access_token)?;
    let source = "Codex CLI (~/.codex/auth.json)".to_string();

    Some(DetectedToken {
        token,
        source: source.clone(),
        subscription_type: None,
        environment: environment.to_string(),
        display_source: source,
        account_id: tokens.account_id.filter(|s| !s.is_empty()),
    })
}

#[cfg(windows)]
fn read_wsl_claude_token() -> Option<DetectedToken> {
    let content = run_wsl_file_read("~/.claude/.credentials.json")?;
    let creds = serde_json::from_str::<ClaudeCredentials>(&content).ok()?;
    let oauth = creds.claude_ai_oauth?;
    if oauth.access_token.is_empty() {
        return None;
    }

    let source = "Claude Code (~/.claude/.credentials.json)".to_string();
    Some(DetectedToken {
        token: oauth.access_token,
        source: source.clone(),
        subscription_type: oauth.subscription_type,
        environment: "wsl".to_string(),
        display_source: format!("WSL {}", source),
        account_id: None,
    })
}

#[cfg(windows)]
fn read_wsl_codex_token() -> Option<DetectedToken> {
    let content = run_wsl_file_read("~/.codex/auth.json")?;
    let auth = serde_json::from_str::<CodexAuth>(&content).ok()?;
    // 与 oauth_detect::detect_openai 对齐：非 chatgpt 模式不视作可用 OAuth 凭据
    if auth.auth_mode.as_deref() != Some("chatgpt") {
        return None;
    }
    let tokens = auth.tokens?;
    let token = tokens
        .access_token
        .as_ref()
        .and_then(parse_codex_access_token)?;
    let source = "Codex CLI (~/.codex/auth.json)".to_string();

    Some(DetectedToken {
        token,
        source: source.clone(),
        subscription_type: None,
        environment: "wsl".to_string(),
        display_source: format!("WSL {}", source),
        account_id: tokens.account_id.filter(|s| !s.is_empty()),
    })
}

#[cfg(windows)]
fn run_wsl_file_read(path: &str) -> Option<String> {
    let script = format!("test -f {path} && cat {path}");
    let output = Command::new("wsl.exe")
        .args(["-e", "sh", "-lc", &script])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let content = String::from_utf8(output.stdout).ok()?;
    let trimmed = content.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// 把 { "0": "a", "1": "b", ... } 格式的对象转为字符串
fn indexed_object_to_string(map: &BTreeMap<String, serde_json::Value>) -> String {
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

fn parse_codex_access_token(value: &Value) -> Option<String> {
    match value {
        Value::String(token) if !token.is_empty() => Some(token.clone()),
        Value::Object(map) => {
            let ordered: BTreeMap<String, Value> = map
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect();
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

fn dirs_next() -> Option<std::path::PathBuf> {
    std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .ok()
        .map(std::path::PathBuf::from)
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectedTokens {
    pub anthropic: Vec<DetectedToken>,
    pub openai: Vec<DetectedToken>,
    /// Gemini OAuth 凭据（token 字段存完整 oauth_creds.json 文本，含 refresh_token）
    #[serde(default)]
    pub gemini: Vec<DetectedToken>,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectedToken {
    pub token: String,
    pub source: String,
    pub subscription_type: Option<String>,
    pub environment: String,
    pub display_source: String,
    /// OpenAI/Codex 的 account_id（用于 ChatGPT-Account-Id header，多账号场景）；
    /// Anthropic 恒为 None
    #[serde(default)]
    pub account_id: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeCredentials {
    claude_ai_oauth: Option<ClaudeOAuth>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeOAuth {
    access_token: String,
    subscription_type: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct CodexAuth {
    /// Codex CLI 认证模式；仅 "chatgpt" 模式才有可用的 ChatGPT OAuth token
    /// （与 providers::oauth_detect::detect_openai 的校验保持一致）
    #[serde(default)]
    auth_mode: Option<String>,
    tokens: Option<CodexTokens>,
}

#[derive(Debug, serde::Deserialize)]
struct CodexTokens {
    access_token: Option<Value>,
    /// OpenAI/Codex 的 account_id（用于 ChatGPT-Account-Id header）
    #[serde(default)]
    account_id: Option<String>,
}
