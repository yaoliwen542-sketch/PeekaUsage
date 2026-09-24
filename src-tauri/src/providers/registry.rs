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
        // === Gemini（Google Cloud Code Assist，Subscription，两步 OAuth）===
        // 1. POST loadCodeAssist 拿 projectId
        // 2. POST retrieveUserQuota 拿 buckets（按 modelId 分组，取最低 remainingFraction
        //    -> utilization = (1 - remainingFraction) * 100）
        // 由 gemini::fetch_gemini_quota 处理。token 过期时用 Gemini CLI 公开的
        // client_id/client_secret 调 oauth2.googleapis.com/token 自动刷新。
        //
        // 凭据特殊性：Gemini 的 OAuth token 是 ~/.gemini/oauth_creds.json 的**完整 JSON**
        // （含 access_token + refresh_token + expiry），而非单个 token 字符串。
        // oauth_detect 的 token_path 指向 $.access_token 仅用于展示/校验存在性，
        // 实际 detect_gemini 返回的 token 字段是整个文件内容（供 subscription 透传）。
        ProviderTemplate {
            id: "gemini".to_string(),
            display_name: "Gemini".to_string(),
            env_key_name: String::new(), // Gemini 不用 API Key
            env_oauth_token_name: Some("GEMINI_OAUTH_CREDS".to_string()),
            icon: "gemini".to_string(),
            docs_url: Some("https://ai.google.dev/".to_string()),
            capabilities: ProviderCapabilities {
                has_balance: false,
                has_usage: false,
                has_rate_limit: false,
                has_subscription: true,
            },
            queries: vec![QuerySpec {
                query_type: QueryType::Subscription {
                    provider: "gemini".to_string(),
                },
                base_url: None,
            }],
            oauth_detect: Some(OAuthDetectConfig {
                file_path: "~/.gemini/oauth_creds.json".to_string(),
                token_path: "$.access_token".to_string(),
                keychain_service: None,
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
                            scale: None,
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
                            scale: None,
                        },
                    },
                    base_url: None,
                },
            ],
            oauth_detect: None,
        },
        // === DeepSeek（Balance × 1，新增）===
        // 修复 L15：实际查询不走这里的 field_map（balance_infos 是多币种数组，
        // 模板无法表达"优先 CNY 条目 + 币种取条目实际值"），
        // 由 ProviderManager 路由到 balance::execute_deepseek_balance_query 特化处理。
        // 此模板保留 URL / 认证 / 能力声明，作为该供应商的注册入口。
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
                        scale: None,
                    },
                },
                base_url: None,
            }],
            oauth_detect: None,
        },
        // === SiliconFlow（硅基流动，Balance × 1）===
        // GET https://api.siliconflow.cn/v1/user/info
        // Bearer 认证。响应 { "data": { "totalBalance": 50.0 } }。
        // 注：仅做国内版（.cn，CNY）；海外版（api.siliconflow.com，USD）用户可走自定义供应商。
        ProviderTemplate {
            id: "siliconflow".to_string(),
            display_name: "SiliconFlow".to_string(),
            env_key_name: "SILICONFLOW_API_KEY".to_string(),
            env_oauth_token_name: None,
            icon: "siliconflow".to_string(),
            docs_url: Some("https://cloud.siliconflow.cn/account/ak".to_string()),
            capabilities: ProviderCapabilities {
                has_balance: true,
                has_usage: false,
                has_rate_limit: false,
                has_subscription: false,
            },
            queries: vec![QuerySpec {
                query_type: QueryType::Balance {
                    url: "https://api.siliconflow.cn/v1/user/info".to_string(),
                    auth: AuthScheme::Bearer,
                    field_map: BalanceFieldMap {
                        total: "$.data.totalBalance".to_string(),
                        used: None,
                        remaining: Some("$.data.totalBalance".to_string()),
                        currency: "CNY".to_string(),
                        scale: None,
                    },
                },
                base_url: None,
            }],
            oauth_detect: None,
        },
        // === StepFun（阶跃星辰，Balance × 1）===
        // GET https://api.stepfun.com/v1/accounts
        // Bearer 认证。响应 { "balance": 100.0 }。
        ProviderTemplate {
            id: "stepfun".to_string(),
            display_name: "StepFun".to_string(),
            env_key_name: "STEPFUN_API_KEY".to_string(),
            env_oauth_token_name: None,
            icon: "stepfun".to_string(),
            docs_url: Some("https://platform.stepfun.com/".to_string()),
            capabilities: ProviderCapabilities {
                has_balance: true,
                has_usage: false,
                has_rate_limit: false,
                has_subscription: false,
            },
            queries: vec![QuerySpec {
                query_type: QueryType::Balance {
                    url: "https://api.stepfun.com/v1/accounts".to_string(),
                    auth: AuthScheme::Bearer,
                    field_map: BalanceFieldMap {
                        total: "$.balance".to_string(),
                        used: None,
                        remaining: Some("$.balance".to_string()),
                        currency: "CNY".to_string(),
                        scale: None,
                    },
                },
                base_url: None,
            }],
            oauth_detect: None,
        },
        // === Novita AI（Balance × 1，单位换算）===
        // GET https://api.novita.ai/v3/user/balance
        // Bearer 认证。响应 { "availableBalance": 500000 }。
        // 注意：availableBalance 单位是万分之一美元，需 scale=0.0001 换算为美元（500000 * 0.0001 = 5.0 USD）。
        ProviderTemplate {
            id: "novita".to_string(),
            display_name: "Novita AI".to_string(),
            env_key_name: "NOVITA_API_KEY".to_string(),
            env_oauth_token_name: None,
            icon: "novita".to_string(),
            docs_url: Some("https://novita.ai/".to_string()),
            capabilities: ProviderCapabilities {
                has_balance: true,
                has_usage: false,
                has_rate_limit: false,
                has_subscription: false,
            },
            queries: vec![QuerySpec {
                query_type: QueryType::Balance {
                    url: "https://api.novita.ai/v3/user/balance".to_string(),
                    auth: AuthScheme::Bearer,
                    field_map: BalanceFieldMap {
                        total: "$.availableBalance".to_string(),
                        used: None,
                        remaining: Some("$.availableBalance".to_string()),
                        currency: "USD".to_string(),
                        scale: Some(0.0001),
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
        // === 火山方舟（Volcengine，CodingPlan + SigV4 签名）===
        // POST https://open.volcengineapi.com/?Action=GetAFPUsage&Version=2024-09-30
        // 火山签名 V4（AK/SK）认证，非 Bearer。由 coding_plan::fetch_volcengine 处理：
        // 1. 解析 "AccessKeyId:SecretAccessKey" 格式的 api_key
        // 2. 调用 sigv4::sign_volc_request 生成签名 headers
        // 3. 主链路 GetAFPUsage（绝对额度 -> 百分比）；失败回退 GetCodingPlanUsage（百分比）
        //
        // 注意：火山方舟的 api_key 字段格式特殊（AK:SK），前端在添加供应商时需提示用户
        // 输入完整 "AKID:SecretKey" 字符串。环境变量 VOLC_ACCESSKEY 同样存储该拼接格式。
        // 签名算法待真实 AK/SK 端到端验证。
        ProviderTemplate {
            id: "volcengine".to_string(),
            display_name: "火山方舟".to_string(),
            env_key_name: "VOLC_ACCESSKEY".to_string(),
            env_oauth_token_name: None,
            icon: "volcengine".to_string(),
            docs_url: Some("https://www.volcengine.com/docs/82379".to_string()),
            capabilities: ProviderCapabilities {
                has_balance: false,
                has_usage: true,
                has_rate_limit: false,
                has_subscription: false,
            },
            queries: vec![QuerySpec {
                query_type: QueryType::CodingPlan {
                    provider: "volcengine".to_string(),
                },
                base_url: None,
            }],
            oauth_detect: None,
        },
        // === 小米 MiMo（CodingPlan：Token Plan 套餐 + 按量余额回退，Cookie 认证）===
        // 主链路 GET https://platform.xiaomimimo.com/api/v1/tokenPlan/usage
        //   响应 code/data 包裹：data.monthUsage.items[0] 的 percent -> monthly 窗口。
        //   辅助 GET /api/v1/tokenPlan/detail 提供套餐名（planCode）与
        //   月度重置时间（currentPeriodEnd，"yyyy-MM-dd HH:mm:ss" UTC）。
        //   Token Plan 不可用（未购买套餐等）时回退 GET /api/v1/balance
        //   （data.balance + data.currency 动态币种的按量余额）。
        // 由 coding_plan::fetch_mimo 处理（响应结构与 CodexBar 开源实现一致）。
        //
        // 认证（重要，v0.4.1 修正）：该域名是小米账号 Cookie 认证的控制台内部
        // 接口，API Key Bearer 实测一律 401；凭据是用户从浏览器复制的整段
        // Cookie（需含 api-platform_serviceToken 和 userId）。因此
        // env_key_name 留空（Cookie 不能用于推理，不接管环境变量），
        // docs_url 直达余额控制台方便用户抓取 Cookie。
        ProviderTemplate {
            id: "mimo".to_string(),
            display_name: "小米 MiMo".to_string(),
            env_key_name: String::new(),
            env_oauth_token_name: None,
            icon: "mimo".to_string(),
            docs_url: Some("https://platform.xiaomimimo.com/#/console/balance".to_string()),
            capabilities: ProviderCapabilities {
                has_balance: false,
                has_usage: true,
                has_rate_limit: false,
                has_subscription: false,
            },
            queries: vec![QuerySpec {
                query_type: QueryType::CodingPlan {
                    provider: "mimo".to_string(),
                },
                base_url: None,
            }],
            oauth_detect: None,
        },
        // === Kimi 开放平台（Moonshot 按量，Balance × 2，国内/国际回退链路）===
        // GET https://api.moonshot.cn/v1/users/me/balance（国内站，CNY）
        //   Bearer 认证。响应 code/data 包裹：data.available_balance
        //   （可用余额 = 现金 + 代金券，≤0 时无法调用推理 API）。
        // 回退链路 GET https://api.moonshot.ai/v1/users/me/balance（国际站，USD）。
        // 注意：与 kimi（api.kimi.com Coding Plan 订阅）是两条独立产品线、
        // 两套独立 Key，国内站与国际站的 Key 也互不通用（混用 401）。
        // 字段来自官方文档《查询账户余额》，数值可能为数字或字符串，extract_field 已兼容。
        ProviderTemplate {
            id: "moonshot".to_string(),
            display_name: "Kimi 开放平台".to_string(),
            env_key_name: "MOONSHOT_API_KEY".to_string(),
            env_oauth_token_name: None,
            icon: "moonshot".to_string(),
            docs_url: Some("https://platform.kimi.com/".to_string()),
            capabilities: ProviderCapabilities {
                has_balance: true,
                has_usage: false,
                has_rate_limit: false,
                has_subscription: false,
            },
            queries: vec![
                QuerySpec {
                    query_type: QueryType::Balance {
                        url: "https://api.moonshot.cn/v1/users/me/balance".to_string(),
                        auth: AuthScheme::Bearer,
                        field_map: BalanceFieldMap {
                            total: "$.data.available_balance".to_string(),
                            used: None,
                            remaining: Some("$.data.available_balance".to_string()),
                            currency: "CNY".to_string(),
                            scale: None,
                        },
                    },
                    base_url: None,
                },
                QuerySpec {
                    query_type: QueryType::Balance {
                        url: "https://api.moonshot.ai/v1/users/me/balance".to_string(),
                        auth: AuthScheme::Bearer,
                        field_map: BalanceFieldMap {
                            total: "$.data.available_balance".to_string(),
                            used: None,
                            remaining: Some("$.data.available_balance".to_string()),
                            currency: "USD".to_string(),
                            scale: None,
                        },
                    },
                    base_url: None,
                },
            ],
            oauth_detect: None,
        },
        // === Together AI（Balance × 1）===
        // GET https://api.together.xyz/v1/getBalance
        // Bearer 认证。响应 { "balance": 123.45 }（USD）。
        // 该端点未收录在官方文档首页，但被社区广泛使用且字段单一；
        // 若官方改字段只会导致解析失败并透出错误，不会静默错值。
        ProviderTemplate {
            id: "together".to_string(),
            display_name: "Together AI".to_string(),
            env_key_name: "TOGETHER_API_KEY".to_string(),
            env_oauth_token_name: None,
            icon: "together".to_string(),
            docs_url: Some("https://api.together.xyz/settings/api-keys".to_string()),
            capabilities: ProviderCapabilities {
                has_balance: true,
                has_usage: false,
                has_rate_limit: false,
                has_subscription: false,
            },
            queries: vec![QuerySpec {
                query_type: QueryType::Balance {
                    url: "https://api.together.xyz/v1/getBalance".to_string(),
                    auth: AuthScheme::Bearer,
                    field_map: BalanceFieldMap {
                        total: "$.balance".to_string(),
                        used: None,
                        remaining: Some("$.balance".to_string()),
                        currency: "USD".to_string(),
                        scale: None,
                    },
                },
                base_url: None,
            }],
            oauth_detect: None,
        },
        // === ZenMux（CodingPlan，Management API 订阅详情）===
        // GET https://zenmux.ai/api/v1/management/subscription/detail
        // Bearer 认证，仅接受 ZenMux 控制台创建的 Management API Key
        // （普通推理 API Key 会 401）。quota_5_hour / quota_7_day 的
        // usage_percentage 是 0-1 小数。端点来自 ZenMux 官方文档，
        // 并与 cc-switch 实现交叉验证。
        ProviderTemplate {
            id: "zenmux".to_string(),
            display_name: "ZenMux".to_string(),
            env_key_name: "ZENMUX_MANAGEMENT_API_KEY".to_string(),
            env_oauth_token_name: None,
            icon: "zenmux".to_string(),
            docs_url: Some("https://zenmux.ai/".to_string()),
            capabilities: ProviderCapabilities {
                has_balance: false,
                has_usage: true,
                has_rate_limit: false,
                has_subscription: false,
            },
            queries: vec![QuerySpec {
                query_type: QueryType::CodingPlan {
                    provider: "zenmux".to_string(),
                },
                base_url: None,
            }],
            oauth_detect: None,
        },
        // === OpenCode Zen Go（CodingPlan，$10/月订阅三时间窗口）===
        // GET https://opencode.ai/zen/go/v1/usage
        // Bearer 认证（推理侧 /messages 只认 x-api-key，用量端点只认 Bearer）。
        // HTTP 403 表示 Key 有效但无 Go 订阅。响应结构来自 cc-switch 实现。
        ProviderTemplate {
            id: "opencode_go".to_string(),
            display_name: "OpenCode Zen Go".to_string(),
            env_key_name: "OPENCODE_API_KEY".to_string(),
            env_oauth_token_name: None,
            icon: "opencode".to_string(),
            docs_url: Some("https://opencode.ai/zen".to_string()),
            capabilities: ProviderCapabilities {
                has_balance: false,
                has_usage: true,
                has_rate_limit: false,
                has_subscription: false,
            },
            queries: vec![QuerySpec {
                query_type: QueryType::CodingPlan {
                    provider: "opencode_go".to_string(),
                },
                base_url: None,
            }],
            oauth_detect: None,
        },
        // === GLM 国际版（Z.AI，CodingPlan）===
        // 与国内版 open.bigmodel.cn 同路径同响应结构，仅域名不同
        // （api.z.ai，裸 key 认证）。解析复用 fetch_glm_at / parse_glm_response。
        ProviderTemplate {
            id: "glm_en".to_string(),
            display_name: "GLM 国际版".to_string(),
            env_key_name: "ZAI_API_KEY".to_string(),
            env_oauth_token_name: None,
            icon: "glm_en".to_string(),
            docs_url: Some("https://z.ai/".to_string()),
            capabilities: ProviderCapabilities {
                has_balance: false,
                has_usage: true,
                has_rate_limit: false,
                has_subscription: false,
            },
            queries: vec![QuerySpec {
                query_type: QueryType::CodingPlan {
                    provider: "glm_en".to_string(),
                },
                base_url: None,
            }],
            oauth_detect: None,
        },
        // === MiniMax 国际版（CodingPlan）===
        // 与国内版 api.minimaxi.com 同路径同响应结构，仅域名不同
        // （api.minimax.io）。解析复用 fetch_minimax_at。
        ProviderTemplate {
            id: "minimax_en".to_string(),
            display_name: "MiniMax 国际版".to_string(),
            env_key_name: "MINIMAX_GLOBAL_API_KEY".to_string(),
            env_oauth_token_name: None,
            icon: "minimax_en".to_string(),
            docs_url: Some("https://www.minimax.io/".to_string()),
            capabilities: ProviderCapabilities {
                has_balance: false,
                has_usage: true,
                has_rate_limit: false,
                has_subscription: false,
            },
            queries: vec![QuerySpec {
                query_type: QueryType::CodingPlan {
                    provider: "minimax_en".to_string(),
                },
                base_url: None,
            }],
            oauth_detect: None,
        },
        // === SiliconFlow 国际版（Balance × 1）===
        // 与国内版 api.siliconflow.cn 同路径（/v1/user/info），域名
        // api.siliconflow.com，美元计价。Key 与国内版互不通用。
        ProviderTemplate {
            id: "siliconflow_en".to_string(),
            display_name: "SiliconFlow 国际版".to_string(),
            env_key_name: "SILICONFLOW_GLOBAL_API_KEY".to_string(),
            env_oauth_token_name: None,
            icon: "siliconflow_en".to_string(),
            docs_url: Some("https://cloud.siliconflow.com/account/ak".to_string()),
            capabilities: ProviderCapabilities {
                has_balance: true,
                has_usage: false,
                has_rate_limit: false,
                has_subscription: false,
            },
            queries: vec![QuerySpec {
                query_type: QueryType::Balance {
                    url: "https://api.siliconflow.com/v1/user/info".to_string(),
                    auth: AuthScheme::Bearer,
                    field_map: BalanceFieldMap {
                        total: "$.data.totalBalance".to_string(),
                        used: None,
                        remaining: Some("$.data.totalBalance".to_string()),
                        currency: "USD".to_string(),
                        scale: None,
                    },
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
