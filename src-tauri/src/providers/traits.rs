use crate::providers::types::*;
use async_trait::async_trait;

/// 用量供应商核心抽象
///
/// 新增供应商只需实现此 trait，然后在 ProviderManager 中注册即可。
/// 注意：阶段 1 期间，此 trait 仍由 openai/anthropic/openrouter 三个旧 provider 实现，
/// 但 ProviderManager 已改为优先从 ProviderRegistry 取模板查询。
/// 阶段 2 起，新供应商不再实现此 trait，而是通过 registry 的 QueryType 配置。
#[async_trait]
pub trait UsageProvider: Send + Sync {
    /// 供应商唯一 ID
    fn id(&self) -> String;

    /// 显示名称
    fn display_name(&self) -> &str;

    /// 供应商支持的能力
    fn capabilities(&self) -> ProviderCapabilities;

    /// 获取用量数据
    async fn fetch_usage(&self, api_key: &str) -> Result<UsageData, ProviderError>;

    /// 获取速率限制数据
    async fn fetch_rate_limits(
        &self,
        api_key: &str,
    ) -> Result<Option<RateLimitData>, ProviderError>;

    /// 验证 API Key 是否有效
    async fn validate_key(&self, api_key: &str) -> Result<bool, ProviderError>;
}

/// 供应商错误类型
#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("认证失败: {0}")]
    AuthError(String),

    #[error("请求失败: {0}")]
    RequestError(String),

    #[error("解析响应失败: {0}")]
    ParseError(String),

    #[error("速率限制: {0}")]
    RateLimited(String),
}

impl ProviderError {
    /// 是否为瞬时错误（网络抖动 / 限流，前端应保留上次成功值并重试）
    pub fn is_transient(&self) -> bool {
        matches!(
            self,
            ProviderError::RequestError(_) | ProviderError::RateLimited(_)
        )
    }
}

impl From<reqwest::Error> for ProviderError {
    fn from(e: reqwest::Error) -> Self {
        if e.is_status() {
            if let Some(status) = e.status() {
                if status.as_u16() == 401 || status.as_u16() == 403 {
                    return ProviderError::AuthError(e.to_string());
                }
                if status.as_u16() == 429 {
                    return ProviderError::RateLimited(e.to_string());
                }
            }
        }
        ProviderError::RequestError(e.to_string())
    }
}
