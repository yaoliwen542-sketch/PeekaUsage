use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::traits::ProviderError;
use super::types::UsageData;

/// 脚本提取结果（JS extractor 返回值）
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExtractResult {
    #[serde(default)]
    plan_name: Option<String>,
    #[serde(default)]
    total: Option<f64>,
    #[serde(default)]
    used: Option<f64>,
    #[serde(default)]
    remaining: Option<f64>,
    #[serde(default)]
    currency: Option<String>,
    #[serde(default)]
    is_valid: Option<bool>,
    #[serde(default)]
    invalid_message: Option<String>,
}

/// 执行 JS 脚本查询
///
/// 流程：
/// 1. 模板变量替换（{{apiKey}} / {{baseUrl}} / {{accessToken}} / {{userId}}）
/// 2. 在 rquickjs 沙箱中执行脚本，拿到 {request, extractor}
/// 3. Rust 端按 request 发 HTTP 请求（脚本不能自己发）
/// 4. 把响应喂回 extractor 函数
/// 5. 转换结果为 UsageData
pub async fn run(
    client: &Client,
    code: &str,
    api_key: &str,
    base_url: Option<&str>,
    allow_http: bool,
    timeout_ms: u64,
) -> Result<UsageData, ProviderError> {
    // 1. 模板变量替换
    let resolved_code = code
        .replace("{{apiKey}}", api_key)
        .replace("{{baseUrl}}", base_url.unwrap_or(""));

    // NewAPI 用 accessToken / userId，阶段 1 暂用 api_key 占位
    // （实际 accessToken/userId 通过 KeyStore 读取，在命令层注入，这里先留接口）
    let resolved_code = resolved_code
        .replace("{{accessToken}}", api_key)
        .replace("{{userId}}", "");

    // 2. 在沙箱中执行脚本，拿到 request 配置
    let request_spec = run_script_for_request(&resolved_code)?;

    // 3. 安全校验
    validate_request_url(&request_spec.url, base_url, allow_http)?;

    // 4. Rust 端发请求
    let response = send_request(client, &request_spec, timeout_ms).await?;

    // 5. 把响应喂回 extractor
    let result = run_extractor(&resolved_code, &response)?;

    // 6. 转换结果
    if result.is_valid == Some(false) {
        return Err(ProviderError::RequestError(
            result
                .invalid_message
                .unwrap_or_else(|| "脚本返回无效结果".to_string()),
        ));
    }

    Ok(UsageData {
        total_used: result.used.unwrap_or(0.0),
        total_budget: result.total,
        remaining: result.remaining,
        currency: result.currency.unwrap_or_else(|| "credits".to_string()),
        period_start: None,
        period_end: None,
    })
}

/// 脚本中的 request 配置
#[derive(Debug, Deserialize)]
struct RequestSpec {
    url: String,
    #[serde(default = "default_method")]
    method: String,
    #[serde(default)]
    headers: std::collections::HashMap<String, String>,
    #[serde(default)]
    body: Option<Value>,
}

fn default_method() -> String {
    "GET".to_string()
}

/// 脚本执行后的 HTTP 响应（传给 extractor）
#[derive(Debug, Serialize)]
struct ScriptResponse {
    status: u16,
    success: bool,
    data: Value,
    headers: std::collections::HashMap<String, String>,
}

/// 在 rquickjs 沙箱中执行脚本，返回 request 配置
///
/// 使用 rquickjs 0.7 的同步 API：Runtime::new() + Context::full() + ctx.with(|ctx| {...})
fn run_script_for_request(code: &str) -> Result<RequestSpec, ProviderError> {
    use rquickjs::{Context, Runtime};

    let runtime = Runtime::new()
        .map_err(|e| ProviderError::ParseError(format!("无法创建 JS 运行时: {}", e)))?;

    let ctx = Context::full(&runtime)
        .map_err(|e| ProviderError::ParseError(format!("无法创建 JS 上下文: {}", e)))?;

    let result: Result<RequestSpec, ProviderError> = ctx.with(|ctx| {
        // 执行脚本代码（返回一个对象 {request, extractor}）
        let value: rquickjs::Value = ctx
            .eval(code)
            .map_err(|e| ProviderError::ParseError(format!("执行脚本失败: {}", e)))?;

        let obj = value
            .into_object()
            .ok_or_else(|| ProviderError::ParseError("脚本未返回对象".into()))?;

        // 取 request 属性
        let request_value: rquickjs::Value = obj
            .get("request")
            .map_err(|e| ProviderError::ParseError(format!("读取 request 失败: {}", e)))?;

        let request_obj = request_value
            .into_object()
            .ok_or_else(|| ProviderError::ParseError("request 不是对象".into()))?;

        // 提取字段
        let url: String = request_obj
            .get("url")
            .map_err(|e| ProviderError::ParseError(format!("读取 url 失败: {}", e)))?;

        let method: String = request_obj
            .get("method")
            .unwrap_or_else(|_| "GET".to_string());

        // headers：可能为 undefined，需要兼容
        let headers_value: Option<rquickjs::Value> = request_obj.get("headers").ok();
        let headers = match headers_value.and_then(|v| v.into_object()) {
            Some(obj) => {
                let mut map = std::collections::HashMap::new();
                for key in obj.keys::<String>() {
                    if let Ok(key) = key {
                        if let Ok(val) = obj.get::<String, String>(key.clone()) {
                            map.insert(key, val);
                        }
                    }
                }
                map
            }
            None => std::collections::HashMap::new(),
        };

        // body：可能为 undefined
        let body: Option<Value> = request_obj.get("body").ok().and_then(|v: rquickjs::Value| {
            // 先序列化成 JSON 再反序列化成 serde_json::Value
            // 用 JSON.stringify 处理（避免 rquickjs::Value 到 serde_json 的直接转换）
            let body_str: Option<String> = stringify_js_value_in_ctx(&ctx, &v).ok();
            body_str.and_then(|s| serde_json::from_str(&s).ok())
        });

        Ok(RequestSpec {
            url,
            method,
            headers,
            body,
        })
    });

    result
}

/// 校验请求 URL：HTTPS 强制 + 同源校验
fn validate_request_url(
    url: &str,
    base_url: Option<&str>,
    allow_http: bool,
) -> Result<(), ProviderError> {
    let parsed = url::Url::parse(url)
        .map_err(|e| ProviderError::ParseError(format!("无效的 URL '{}': {}", url, e)))?;

    // HTTPS 强制
    if parsed.scheme() == "http" && !allow_http {
        return Err(ProviderError::RequestError(
            "默认禁止 HTTP 请求，请在自定义供应商配置中开启'允许 HTTP'".into(),
        ));
    }

    if parsed.scheme() != "https" && parsed.scheme() != "http" {
        return Err(ProviderError::RequestError(format!(
            "不支持的协议: {}",
            parsed.scheme()
        )));
    }

    // 同源校验：如果有 base_url，请求 URL 的 host:port 必须和 base_url 一致
    if let Some(base) = base_url {
        if let Ok(base_parsed) = url::Url::parse(base) {
            let req_host = parsed.host_str();
            let base_host = base_parsed.host_str();
            let req_port = parsed.port_or_known_default();
            let base_port = base_parsed.port_or_known_default();

            if req_host != base_host || req_port != base_port {
                return Err(ProviderError::RequestError(format!(
                    "请求 URL 的主机 {} 与 base_url 的主机 {} 不一致（同源校验失败）",
                    req_host.unwrap_or("unknown"),
                    base_host.unwrap_or("unknown")
                )));
            }
        }
    }

    Ok(())
}

/// Rust 端发送 HTTP 请求
async fn send_request(
    client: &Client,
    spec: &RequestSpec,
    timeout_ms: u64,
) -> Result<ScriptResponse, ProviderError> {
    let method = match spec.method.to_uppercase().as_str() {
        "GET" => reqwest::Method::GET,
        "POST" => reqwest::Method::POST,
        "PUT" => reqwest::Method::PUT,
        "DELETE" => reqwest::Method::DELETE,
        _ => reqwest::Method::GET,
    };

    let mut builder = client
        .request(method, &spec.url)
        .timeout(std::time::Duration::from_millis(timeout_ms));

    for (key, value) in &spec.headers {
        builder = builder.header(key, value);
    }

    if let Some(body) = &spec.body {
        builder = builder.json(body);
    }

    let resp = builder.send().await.map_err(|e| {
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
    let success = status.is_success();

    // 收集响应 header
    let mut headers = std::collections::HashMap::new();
    for (key, value) in resp.headers() {
        if let Ok(v) = value.to_str() {
            headers.insert(key.as_str().to_string(), v.to_string());
        }
    }

    // bytes-then-parse
    let body_bytes = resp
        .bytes()
        .await
        .map_err(|e| ProviderError::RequestError(format!("读取响应体失败: {}", e)))?;

    let data: Value = serde_json::from_slice(&body_bytes).unwrap_or(Value::Null);

    Ok(ScriptResponse {
        status: status.as_u16(),
        success,
        data,
        headers,
    })
}

/// 在 rquickjs 沙箱中执行 extractor 函数
fn run_extractor(code: &str, response: &ScriptResponse) -> Result<ExtractResult, ProviderError> {
    use rquickjs::{Context, Runtime};

    let runtime = Runtime::new()
        .map_err(|e| ProviderError::ParseError(format!("无法创建 JS 运行时: {}", e)))?;

    let ctx = Context::full(&runtime)
        .map_err(|e| ProviderError::ParseError(format!("无法创建 JS 上下文: {}", e)))?;

    let response_json = serde_json::to_string(response)
        .map_err(|e| ProviderError::ParseError(format!("序列化响应失败: {}", e)))?;

    let result: Result<ExtractResult, ProviderError> = ctx.with(|ctx| {
        // 执行脚本拿到 {request, extractor} 对象
        let value: rquickjs::Value = ctx
            .eval(code)
            .map_err(|e| ProviderError::ParseError(format!("执行脚本失败: {}", e)))?;

        let obj = value
            .into_object()
            .ok_or_else(|| ProviderError::ParseError("脚本未返回对象".into()))?;

        // 取 extractor 函数
        let extractor: rquickjs::Function = obj
            .get("extractor")
            .map_err(|e| ProviderError::ParseError(format!("读取 extractor 失败: {}", e)))?;

        // 把 response JSON 解析成 JS 对象，传给 extractor
        // 用括号包裹 JSON 字符串，使其成为 JS 表达式求值
        let response_expr = format!("({})", response_json);
        let response_value: rquickjs::Value = ctx
            .eval(response_expr.as_str())
            .map_err(|e| ProviderError::ParseError(format!("解析响应失败: {}", e)))?;

        // 调用 extractor(response)
        let result_value: rquickjs::Value = extractor
            .call((response_value,))
            .map_err(|e| ProviderError::ParseError(format!("执行 extractor 失败: {}", e)))?;

        // 把结果序列化成 JSON 字符串再反序列化
        // 在 ctx 闭包内完成 stringify，保证生命周期一致
        let result_json = stringify_js_value_in_ctx(&ctx, &result_value)?;

        serde_json::from_str::<ExtractResult>(&result_json)
            .map_err(|e| ProviderError::ParseError(format!("解析 extractor 结果失败: {}", e)))
    });

    result
}

/// 用 JS 内置的 JSON.stringify 把一个 rquickjs::Value 序列化成 JSON 字符串
///
/// 必须在 `ctx.with` 闭包内调用。ctx 与 value 共享同一个 JS 生命周期 'js。
fn stringify_js_value_in_ctx<'js>(
    ctx: &rquickjs::Ctx<'js>,
    value: &rquickjs::Value<'js>,
) -> Result<String, ProviderError> {
    use rquickjs::Object;

    // 取全局 JSON 对象
    let json_obj: Object<'js> = ctx
        .globals()
        .get("JSON")
        .map_err(|e| ProviderError::ParseError(format!("取全局 JSON 失败: {}", e)))?;

    let stringify: rquickjs::Function<'js> = json_obj
        .get("stringify")
        .map_err(|e| ProviderError::ParseError(format!("取 JSON.stringify 失败: {}", e)))?;

    // JSON.stringify(value) -> String
    let json_str: String = stringify
        .call((value.clone(),))
        .map_err(|e| ProviderError::ParseError(format!("调用 JSON.stringify 失败: {}", e)))?;

    Ok(json_str)
}
