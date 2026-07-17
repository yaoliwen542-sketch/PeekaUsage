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
        // === OpenAI（复合型：Balance × 3 + Subscription × 1）===
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
                // 1. 预付费 credit grants
                QuerySpec {
                    query_type: QueryType::Balance {
                        url: "https://api.openai.com/v1/dashboard/billing/credit_grants"
                            .to_string(),
                        auth: AuthScheme::Bearer,
                        field_map: BalanceFieldMap {
                            total: "$.total_granted".to_string(),
                            used: Some("$.total_used".to_string()),
                            remaining: Some("$.total_available".to_string()),
                            currency: "USD".to_string(),
                        },
                    },
                    base_url: None,
                },
                // 2. 后付费 costs（本月，动态时间戳占位符）
                QuerySpec {
                    query_type: QueryType::Balance {
                        url: "https://api.openai.com/v1/organization/costs?start_time={{month_start_ts}}&end_time={{now_ts}}&group_by=line_item".to_string(),
                        auth: AuthScheme::Bearer,
                        field_map: BalanceFieldMap {
                            total: "$.total".to_string(),
                            used: Some("$.used".to_string()),
                            remaining: None,
                            currency: "USD".to_string(),
                        },
                    },
                    base_url: None,
                },
                // 3. 限额 subscription
                QuerySpec {
                    query_type: QueryType::Balance {
                        url: "https://api.openai.com/v1/dashboard/billing/subscription"
                            .to_string(),
                        auth: AuthScheme::Bearer,
                        field_map: BalanceFieldMap {
                            total: "$.hard_limit_usd".to_string(),
                            used: None,
                            remaining: None,
                            currency: "USD".to_string(),
                        },
                    },
                    base_url: None,
                },
                // 4. OAuth 订阅（ChatGPT Plus/Pro/Max）
                QuerySpec {
                    query_type: QueryType::Subscription {
                        provider: "openai_wham".to_string(),
                    },
                    base_url: None,
                },
            ],
            oauth_detect: None, // 阶段 2 填充
        },
        // === Anthropic（Balance × 1 + Subscription × 1）===
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
                // 1. cost_report（按量，x-api-key 认证，动态日期占位符）
                QuerySpec {
                    query_type: QueryType::Balance {
                        url: "https://api.anthropic.com/v1/organizations/cost_report?start_date={{month_start_date}}&end_date={{today_date}}".to_string(),
                        auth: AuthScheme::XApiKey,
                        field_map: BalanceFieldMap {
                            total: "$.total".to_string(),
                            used: Some("$.used".to_string()),
                            remaining: None,
                            currency: "USD".to_string(),
                        },
                    },
                    base_url: None,
                },
                // 2. OAuth 订阅
                QuerySpec {
                    query_type: QueryType::Subscription {
                        provider: "anthropic_oauth".to_string(),
                    },
                    base_url: None,
                },
            ],
            oauth_detect: None,
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
        // === NewAPI（Script，预置脚本）===
        ProviderTemplate {
            id: "newapi".to_string(),
            display_name: "NewAPI".to_string(),
            env_key_name: "NEWAPI_API_KEY".to_string(),
            env_oauth_token_name: None,
            icon: "newapi".to_string(),
            docs_url: Some("https://github.com/Calcium-Ion/new-api".to_string()),
            capabilities: ProviderCapabilities {
                has_balance: true,
                has_usage: false,
                has_rate_limit: false,
                has_subscription: false,
            },
            queries: vec![QuerySpec {
                query_type: QueryType::Script {
                    default_template: Some(newapi_script_template().to_string()),
                },
                base_url: None,
            }],
            oauth_detect: None,
        },
    ]
}

/// NewAPI 预置 JS 脚本模板
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
