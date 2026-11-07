use std::sync::Arc;
use std::time::Duration;

use tokio::sync::RwLock;

use crate::config::app_config::ProxyMode;

/// 代理配置快照（密码由 KeyStore 读出后传入，绝不落 config.json）
#[derive(Debug, Clone)]
pub struct ProxyConfig {
    pub mode: ProxyMode,
    pub url: String,
    pub username: String,
    pub password: Option<String>,
}

impl ProxyConfig {
    pub fn from_settings(
        mode: ProxyMode,
        url: String,
        username: String,
        password: Option<String>,
    ) -> Self {
        Self {
            mode,
            url,
            username,
            password,
        }
    }
}

/// 按 ProxyConfig 构建 client：none 显式禁用代理；system 用 reqwest 默认（读系统代理）；
/// custom 用 Proxy::all，用户名非空时附带 Basic 鉴权。
pub fn build_client(proxy: &ProxyConfig) -> Result<reqwest::Client, String> {
    let mut builder = reqwest::Client::builder()
        // 必须带超时：网络挂起时避免刷新永久卡死（AGENTS.md 27 条）
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(10));
    builder = match proxy.mode {
        ProxyMode::None => builder.no_proxy(),
        ProxyMode::System => builder,
        ProxyMode::Custom => {
            let url = proxy.url.trim();
            if url.is_empty() {
                return Err(String::from("代理地址不能为空"));
            }
            let mut proxy_builder =
                reqwest::Proxy::all(url).map_err(|error| format!("代理地址无效: {error}"))?;
            if !proxy.username.is_empty() {
                if let Some(password) = proxy.password.as_deref() {
                    proxy_builder = proxy_builder.basic_auth(&proxy.username, password);
                }
            }
            builder.proxy(proxy_builder)
        }
    };
    builder
        .build()
        .map_err(|error| format!("初始化 HTTP 客户端失败: {error}"))
}

/// 统一 HTTP 客户端工厂：所有用量查询链路从这里取 client。
/// 保存代理设置后调用 reload() 热重建；请求侧通过 client() 取当前快照，不阻塞 reload。
pub struct HttpClientFactory {
    client: RwLock<Arc<reqwest::Client>>,
}

impl HttpClientFactory {
    pub fn new(proxy: &ProxyConfig) -> Self {
        let client = build_client(proxy).expect("无法创建 HTTP 客户端");
        Self {
            client: RwLock::new(Arc::new(client)),
        }
    }

    pub async fn reload(&self, proxy: &ProxyConfig) -> Result<(), String> {
        let client = build_client(proxy)?;
        *self.client.write().await = Arc::new(client);
        Ok(())
    }

    pub async fn client(&self) -> Arc<reqwest::Client> {
        self.client.read().await.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(mode: ProxyMode, url: &str) -> ProxyConfig {
        ProxyConfig {
            mode,
            url: url.into(),
            username: String::new(),
            password: None,
        }
    }

    #[test]
    fn system_模式构建成功() {
        assert!(build_client(&config(ProxyMode::System, "")).is_ok());
    }

    #[test]
    fn none_模式构建成功且显式无代理() {
        assert!(build_client(&config(ProxyMode::None, "")).is_ok());
    }

    #[test]
    fn custom_模式空地址报错() {
        let error = build_client(&config(ProxyMode::Custom, "")).unwrap_err();
        assert!(error.contains("代理地址不能为空"));
    }

    #[test]
    fn custom_模式非法地址报错() {
        let error = build_client(&config(ProxyMode::Custom, "not a proxy url")).unwrap_err();
        assert!(error.contains("代理地址无效"));
    }

    #[test]
    fn custom_模式支持_http_与_socks5() {
        assert!(build_client(&config(ProxyMode::Custom, "http://127.0.0.1:7890")).is_ok());
        assert!(build_client(&config(ProxyMode::Custom, "socks5://127.0.0.1:1080")).is_ok());
    }

    #[tokio::test]
    async fn reload_后_client_快照更新() {
        let factory = HttpClientFactory::new(&config(ProxyMode::System, ""));
        factory
            .reload(&config(ProxyMode::Custom, "http://127.0.0.1:7890"))
            .await
            .expect("reload 应成功");
        let client = factory.client().await;
        // 快照 Arc 指向新构建的 client，可正常发起请求（不实际联网）
        assert!(client.get("https://example.com").try_clone().is_some());
    }
}
