use super::types::*;

/// 内置供应商注册表
///
/// 新增内置供应商 = 在此函数返回的 Vec 里追加一条 ProviderTemplate。
/// 不再需要修改任何 match 分支。
///
/// 注意：URL 中涉及动态时间戳的地方使用占位符（{{month_start_ts}} / {{now_ts}}
/// / {{month_start_date}} / {{today_date}}），由 balance.rs 在发请求前替换为当前时间。
/// 这样可避免进程启动时把时间戳固定下来（builtin_templates 仅在进程启动时调用一次）。
fn builtin_templates() -> Vec<ProviderTemplate> {
    vec![
        // === OpenAI（按量查询走 legacy fetch_usage，registry 只保留 Subscription）===
        // OpenAI 的 Balance 查询模板曾使用错误的 JSONPath（如 $.total_granted），
        // 实际 OpenAI credit_grants 响应字段并非如此，导致 C-1 解析失败。
        // 阶段 1 修复：registry 只保留 Subscription 查询；
        // 按量查询统一走 legacy OpenAIProvider::fetch_usage（credit_grants + costs + subscription 三条合并逻辑）。
        ProviderTemplate {
            id: "openai".to_string(),
            display_name: "OpenAI".to_string(),
            env_key_name: "OPENAI_API_KEY".to_string(),
            env_oauth_token_name: Some("OPENAI_OAUTH_TOKEN".to_string()),
            icon: "openai".to_string(),
            docs_url: Some("https://platform.openai.com/api-keys".to_string()),
            capabilities: ProviderCapabilities {
                has_balance: true,
                has_usage: true,
                has_rate_limit: false,
                has_subscription: true,
            },
            queries: vec![
                // OAuth 订阅（ChatGPT Plus/Pro/Max）
                // 按量 API 查询不在 registry 中，由 ProviderManager::fetch_api_usage 路由到 legacy fetch_usage
                QuerySpec {
                    query_type: QueryType::Subscription {
                        provider: "openai_wham".to_string(),
                    },
                    base_url: None,
                },
            ],
            // OAuth 自动检测：从 ~/.codex/auth.json 读取 tokens.access_token + tokens.account_id
            // （token_path 仅作前端展示/文档用，实际解析由 oauth_detect::detect_openai 处理，
            // 兼容字符串和索引对象两种 access_token 格式）
            oauth_detect: Some(OAuthDetectConfig {
                file_path: "~/.codex/auth.json".to_string(),
                token_path: "$.tokens.access_token".to_string(),
                keychain_service: None,
            }),
        },
        // === Anthropic（按量查询走 legacy fetch_usage，registry 只保留 Subscription）===
        // Anthropic 的 cost_report 模板在 registry 里走 Balance 查询分支，但 BalanceFieldMap
        // 的 JSONPath（$.total / $.used）与 cost_report 实际响应结构不符，导致 C-2 解析失败。
        // 阶段 1 修复：registry 只保留 Subscription 查询；
        // 按量查询统一走 legacy AnthropicProvider::fetch_usage（直接读 cost_cents 累加）。
        ProviderTemplate {
            id: "anthropic".to_string(),
            display_name: "Anthropic".to_string(),
            env_key_name: "ANTHROPIC_API_KEY".to_string(),
            env_oauth_token_name: Some("ANTHROPIC_OAUTH_TOKEN".to_string()),
            icon: "anthropic".to_string(),
            docs_url: Some("https://docs.anthropic.com/en/api/getting-started".to_string()),
            capabilities: ProviderCapabilities {
                has_balance: false,
                has_usage: true,
                has_rate_limit: true,
                has_subscription: true,
            },
            queries: vec![
                // OAuth 订阅
                // 按量 API 查询不在 registry 中，由 ProviderManager::fetch_api_usage 路由到 legacy fetch_usage
                QuerySpec {
                    query_type: QueryType::Subscription {
                        provider: "anthropic_oauth".to_string(),
                    },
                    base_url: None,
                },
            ],
            // OAuth 自动检测：从 ~/.claude/.credentials.json 读取 claudeAiOauth.accessToken
            // （兼容旧 key claude.ai_oauth）；macOS 额外尝试 Keychain
            // (service="Claude Code-credentials")
            oauth_detect: Some(OAuthDetectConfig {
                file_path: "~/.claude/.credentials.json".to_string(),
                token_path: "$.claudeAiOauth.accessToken".to_string(),
                keychain_service: Some("Claude Code-credentials".to_string()),
            }),
        },
        // === OpenRouter（Balance × 2，回退链路）===
        ProviderTemplate {
            id: "openrouter".to_string(),
            display_name: "OpenRouter".to_string(),
            env_key_name: "OPENROUTER_API_KEY".to_string(),
            env_oauth_token_name: None,
            icon: "openrouter".to_string(),
            docs_url: Some("https://openrouter.ai/keys".to_string()),
            capabilities: ProviderCapabilities {
                has_balance: true,
                has_usage: true,
                has_rate_limit: true,
                has_subscription: false,
            },
            queries: vec![
                // 主链路：/api/v1/credits
                QuerySpec {
                    query_type: QueryType::Balance {
                        url: "https://openrouter.ai/api/v1/credits".to_string(),
                        auth: AuthScheme::Bearer,
                        field_map: BalanceFieldMap {
                            total: "$.data.total_credits".to_string(),
                            used: Some("$.data.total_usage".to_string()),
                            remaining: None, // = total - used
                            currency: "USD".to_string(),
                        },
                    },
                    base_url: None,
                },
                // 回退链路：/api/v1/key
                QuerySpec {
                    query_type: QueryType::Balance {
                        url: "https://openrouter.ai/api/v1/key".to_string(),
                        auth: AuthScheme::Bearer,
                        field_map: BalanceFieldMap {
                            total: "$.data.limit".to_string(),
                            used: Some("$.data.usage".to_string()),
                            remaining: None,
                            currency: "USD".to_string(),
                        },
                    },
                    base_url: None,
                },
            ],
            oauth_detect: None,
        },
        // === DeepSeek（Balance × 1，新增）===
        ProviderTemplate {
            id: "deepseek".to_string(),
            display_name: "DeepSeek".to_string(),
            env_key_name: "DEEPSEEK_API_KEY".to_string(),
            env_oauth_token_name: None,
            icon: "deepseek".to_string(),
            docs_url: Some("https://platform.deepseek.com/api-keys".to_string()),
            capabilities: ProviderCapabilities {
                has_balance: true,
                has_usage: false,
                has_rate_limit: false,
                has_subscription: false,
            },
            queries: vec![QuerySpec {
                query_type: QueryType::Balance {
                    url: "https://api.deepseek.com/user/balance".to_string(),
                    auth: AuthScheme::Bearer,
                    field_map: BalanceFieldMap {
                        total: "$.balance_infos[0].total_balance".to_string(),
                        used: None,
                        remaining: Some("$.balance_infos[0].total_balance".to_string()),
                        currency: "USD".to_string(),
                    },
                },
                base_url: None,
            }],
            oauth_detect: None,
        },
        // === Kimi（月之暗面，CodingPlan）===
        // GET https://api.kimi.com/coding/v1/usages
        // Bearer 认证。响应含 limits[0].detail（5 小时窗口）和 usage（周限额窗口）。
        // 由 coding_plan::fetch_kimi 解析成百分比型 UsageData。
        ProviderTemplate {
            id: "kimi".to_string(),
            display_name: "Kimi".to_string(),
            env_key_name: "KIMI_API_KEY".to_string(),
            env_oauth_token_name: None,
            icon: "kimi".to_string(),
            docs_url: Some("https://platform.moonshot.cn/".to_string()),
            capabilities: ProviderCapabilities {
                has_balance: false,
                has_usage: true,
                has_rate_limit: false,
                has_subscription: false,
            },
            queries: vec![QuerySpec {
                query_type: QueryType::CodingPlan {
                    provider: "kimi".to_string(),
                },
                base_url: None,
            }],
            oauth_detect: None,
        },
        // === GLM（智谱，个人版，CodingPlan）===
        // GET https://open.bigmodel.cn/api/monitor/usage/quota/limit
        // 裸 key 认证（无 Bearer 前缀）+ Accept-Language: en-US,en。
        // 响应 data.limits[] 中 unit==3 -> 5 小时窗口，unit==6 -> 周限额窗口。
        // 由 coding_plan::fetch_glm 解析成百分比型 UsageData。
        ProviderTemplate {
            id: "glm".to_string(),
            display_name: "GLM".to_string(),
            env_key_name: "GLM_API_KEY".to_string(),
            env_oauth_token_name: None,
            icon: "glm".to_string(),
            docs_url: Some("https://open.bigmodel.cn/".to_string()),
            capabilities: ProviderCapabilities {
                has_balance: false,
                has_usage: true,
                has_rate_limit: false,
                has_subscription: false,
            },
            queries: vec![QuerySpec {
                query_type: QueryType::CodingPlan {
                    provider: "glm".to_string(),
                },
                base_url: None,
            }],
            oauth_detect: None,
        },
        // === MiniMax（CodingPlan）===
        // GET https://api.minimaxi.com/v1/api/openplatform/coding_plan/remains
        // Bearer 认证。响应 model_remains[] 中 model_name=="general" 的条目：
        // current_interval_remaining_percent -> 5 小时窗口（utilization = 100 - remain）；
        // current_weekly_status==1 时 current_weekly_remaining_percent -> 周限额窗口。
        // 由 coding_plan::fetch_minimax 解析成百分比型 UsageData。
        ProviderTemplate {
            id: "minimax".to_string(),
            display_name: "MiniMax".to_string(),
            env_key_name: "MINIMAX_API_KEY".to_string(),
            env_oauth_token_name: None,
            icon: "minimax".to_string(),
            docs_url: Some("https://platform.minimaxi.com/".to_string()),
            capabilities: ProviderCapabilities {
                has_balance: false,
                has_usage: true,
                has_rate_limit: false,
                has_subscription: false,
            },
            queries: vec![QuerySpec {
                query_type: QueryType::CodingPlan {
                    provider: "minimax".to_string(),
                },
                base_url: None,
            }],
            oauth_detect: None,
        },
    ]
}

/// NewAPI 预置 JS 脚本模板（供自定义供应商向导的"NewAPI 预设"按钮调用）
///
/// NewAPI 不再作为内置供应商条目存在于 registry：它的 base_url / accessToken / userId
/// 因部署而异，无法用一组固定的模板覆盖所有部署。阶段 1 修复 C-3：
/// NewAPI 改为"自定义供应商预设"，用户通过向导一键填充此脚本模板，
/// 然后填入自己的 base_url / accessToken / userId。
pub fn newapi_script_template() -> &'static str {
    r#"({
  request: {
    url: "{{baseUrl}}/api/user/self",
    method: "GET",
    headers: {
      "Authorization": "Bearer {{accessToken}}",
      "Content-Type": "application/json",
      "User-Agent": "PeekaUsage/1.0",
      "New-Api-User": "{{userId}}"
    }
  },
  extractor: function(response) {
    if (response.success && response.data) {
      return {
        planName: response.data.group || "默认分组",
        remaining: response.data.quota / 500000,
        used: response.data.used_quota / 500000,
        total: (response.data.quota + response.data.used_quota) / 500000,
        currency: "USD"
      };
    }
    return { isValid: false, invalidMessage: response.message || "查询失败" };
  }
})"#
}

/// 按 ID 获取内置供应商模板
pub fn get(id: &str) -> Option<ProviderTemplate> {
    builtin_templates().into_iter().find(|t| t.id == id)
}

/// 获取所有内置供应商模板
pub fn all() -> Vec<ProviderTemplate> {
    builtin_templates()
}
