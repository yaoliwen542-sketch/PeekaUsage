//! 火山方舟（Volcengine）请求签名 V4
//!
//! 火山方舟控制面 OpenAPI（`open.volcengineapi.com`）使用 AK/SK 签名认证，
//! 属于火山签名 V4（与 AWS SigV4 类似但不完全相同）。本模块实现该签名算法。
//!
//! 与 AWS SigV4 的差异：
//! - algorithm 标识符为 `HMAC-SHA256`（AWS 是 `AWS4-HMAC-SHA256`）
//! - 派生签名密钥时 SecretKey 直接使用，不加前缀（AWS 是 `AWS4` 前缀）
//! - scope 结尾固定为 `request`（AWS 是 `aws4_request`）
//! - 固定参与签名的 header 顺序：`host;x-content-sha256;x-date`
//!   （AWS 通常为 `host;x-amz-content-sha256;x-amz-date`）
//!
//! 算法步骤：
//! 1. 计算请求体 SHA256 -> `x-content-sha256`
//! 2. 构造 canonical request（method + URI + query + canonical headers + signed headers + payload hash）
//! 3. 构造 string to sign（algorithm + x-date + scope + SHA256(canonical request)）
//! 4. 派生签名密钥：HMAC(HMAC(HMAC(HMAC(SK, date), region), service), "request")
//! 5. 计算 signature = HEX(HMAC(signing_key, string_to_sign))
//! 6. 组装 Authorization header
//!
//! 已经真实 AK/SK 端到端验证（2026-07）：官方算法可通过网关认证；
//! 此前参照 cc-switch 的实现（content-type 参与签名、canonical request 末尾留空、
//! VOLC: 前缀派生密钥）会被网关拒绝（SignatureDoesNotMatch），不要回退。

use chrono::Utc;
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

/// 火山方舟请求签名参数
pub struct VolcSignParams<'a> {
    /// Access Key ID（火山控制台 AK）
    pub access_key_id: &'a str,
    /// Secret Access Key（火山控制台 SK）
    pub secret_access_key: &'a str,
    /// 区域，如 "cn-beijing"
    pub region: &'a str,
    /// 服务名，火山方舟为 "ark"
    pub service: &'a str,
    /// 主机名，如 "open.volcengineapi.com"
    pub host: &'a str,
    /// HTTP 方法，如 "POST"
    pub method: &'a str,
    /// canonical query string，如 "Action=GetAFPUsage&Version=2024-01-01"
    pub query: &'a str,
    /// 请求体字节
    pub body: &'a [u8],
}

/// 生成签名所需的 HTTP headers
///
/// 返回值包含：
/// - `Content-Type: application/json`
/// - `Host: {host}`
/// - `X-Date: {ISO 基本格式时间}`
/// - `X-Content-Sha256: {body 的 SHA256}`
/// - `Authorization: HMAC-SHA256 Credential=...`
pub fn sign_volc_request(params: VolcSignParams) -> Vec<(String, String)> {
    let now = Utc::now();
    // X-Date 格式：yyyyMMddTHHmmssZ（ISO 8601 基本格式）
    let x_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    // 短日期：yyyyMMdd
    let short_date = now.format("%Y%m%d").to_string();

    // 1. 计算请求体的 SHA256
    let body_hash = hex_sha256(params.body);

    // 2. 构造 canonical request
    // canonical headers：每个 header 一行，格式 "key:value\n"
    // 顺序必须与 signed_headers 一致：host;x-content-sha256;x-date
    let canonical_headers = format!(
        "host:{}\nx-content-sha256:{}\nx-date:{}\n",
        params.host, body_hash, x_date
    );
    let signed_headers = "host;x-content-sha256;x-date";

    // canonical request = method + canonical URI + canonical query + canonical headers + signed headers + payload hash
    let canonical_request = format!(
        "{}\n{}\n{}\n{}\n{}\n{}",
        params.method, "/", params.query, canonical_headers, signed_headers, body_hash
    );

    // 3. 构造 string to sign
    // scope = {short_date}/{region}/{service}/request
    let scope = format!(
        "{}/{}/{}/request",
        short_date, params.region, params.service
    );
    let string_to_sign = format!(
        "HMAC-SHA256\n{}\n{}\n{}",
        x_date,
        scope,
        hex_sha256(canonical_request.as_bytes())
    );

    // 4. 派生签名密钥
    // k_date = HMAC(SK, short_date)（官方算法，无前缀）
    // k_region = HMAC(k_date, region)
    // k_service = HMAC(k_region, service)
    // k_signing = HMAC(k_service, "request")
    let k_date = hmac_sha256(params.secret_access_key.as_bytes(), short_date.as_bytes());
    let k_region = hmac_sha256(&k_date, params.region.as_bytes());
    let k_service = hmac_sha256(&k_region, params.service.as_bytes());
    let k_signing = hmac_sha256(&k_service, b"request");

    // 5. 计算 signature
    let signature = hex_hmac_sha256(&k_signing, string_to_sign.as_bytes());

    // 6. 组装 Authorization header
    let authorization = format!(
        "HMAC-SHA256 Credential={}/{}, SignedHeaders={}, Signature={}",
        params.access_key_id, scope, signed_headers, signature
    );

    vec![
        ("Content-Type".to_string(), "application/json".to_string()),
        ("Host".to_string(), params.host.to_string()),
        ("X-Date".to_string(), x_date),
        ("X-Content-Sha256".to_string(), body_hash),
        ("Authorization".to_string(), authorization),
    ]
}

/// 计算数据的 SHA256 并返回小写十六进制字符串
fn hex_sha256(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

/// HMAC-SHA256，返回原始字节数组
fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC-SHA256 接受任意长度密钥，不会失败");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

/// HMAC-SHA256 并返回小写十六进制字符串
fn hex_hmac_sha256(key: &[u8], data: &[u8]) -> String {
    hex::encode(hmac_sha256(key, data))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 验证签名基本流程：相同输入应产生确定性输出（不验证签名正确性，仅验证幂等性）
    #[test]
    fn test_sign_is_deterministic() {
        let body = b"{}";
        let params = VolcSignParams {
            access_key_id: "AKTEST123",
            secret_access_key: "SKTEST456",
            region: "cn-beijing",
            service: "ark",
            host: "open.volcengineapi.com",
            method: "POST",
            query: "Action=GetAFPUsage&Version=2024-01-01",
            body,
        };

        // 同一时刻两次签名应完全相同（X-Date 在秒级粒度）
        let h1 = sign_volc_request(VolcSignParams {
            access_key_id: params.access_key_id,
            secret_access_key: params.secret_access_key,
            region: params.region,
            service: params.service,
            host: params.host,
            method: params.method,
            query: params.query,
            body,
        });
        let h2 = sign_volc_request(VolcSignParams {
            access_key_id: params.access_key_id,
            secret_access_key: params.secret_access_key,
            region: params.region,
            service: params.service,
            host: params.host,
            method: params.method,
            query: params.query,
            body,
        });

        // 找 Authorization header 对比
        let auth1: String = h1
            .iter()
            .find(|(k, _)| k == "Authorization")
            .map(|(_, v)| v.clone())
            .unwrap();
        let auth2: String = h2
            .iter()
            .find(|(k, _)| k == "Authorization")
            .map(|(_, v)| v.clone())
            .unwrap();
        assert_eq!(auth1, auth2);
    }

    /// 验证返回的 header 集合完整
    #[test]
    fn test_sign_returns_all_required_headers() {
        let params = VolcSignParams {
            access_key_id: "AK",
            secret_access_key: "SK",
            region: "cn-beijing",
            service: "ark",
            host: "open.volcengineapi.com",
            method: "POST",
            query: "Action=GetAFPUsage&Version=2024-01-01",
            body: b"{}",
        };

        let headers = sign_volc_request(params);
        let keys: Vec<&str> = headers.iter().map(|(k, _)| k.as_str()).collect();
        assert!(keys.contains(&"Content-Type"));
        assert!(keys.contains(&"Host"));
        assert!(keys.contains(&"X-Date"));
        assert!(keys.contains(&"X-Content-Sha256"));
        assert!(keys.contains(&"Authorization"));
    }

    /// 验证 X-Content-Sha256 是 body 的 SHA256
    #[test]
    fn test_x_content_sha256_matches_body() {
        let body = br#"{"Region":"cn-beijing"}"#;
        let expected_hash = hex_sha256(body);

        let params = VolcSignParams {
            access_key_id: "AK",
            secret_access_key: "SK",
            region: "cn-beijing",
            service: "ark",
            host: "open.volcengineapi.com",
            method: "POST",
            query: "Action=GetAFPUsage&Version=2024-01-01",
            body,
        };

        let headers = sign_volc_request(params);
        let x_content = headers
            .iter()
            .find(|(k, _)| k == "X-Content-Sha256")
            .map(|(_, v)| v.clone())
            .unwrap();
        assert_eq!(x_content, expected_hash);
    }

    /// 验证 Authorization 格式包含必要的字段
    #[test]
    fn test_authorization_format() {
        let params = VolcSignParams {
            access_key_id: "AKTEST",
            secret_access_key: "SKTEST",
            region: "cn-beijing",
            service: "ark",
            host: "open.volcengineapi.com",
            method: "POST",
            query: "Action=GetAFPUsage&Version=2024-01-01",
            body: b"{}",
        };

        let headers = sign_volc_request(params);
        let auth = headers
            .iter()
            .find(|(k, _)| k == "Authorization")
            .map(|(_, v)| v.clone())
            .unwrap();

        assert!(auth.starts_with("HMAC-SHA256 Credential=AKTEST/"));
        assert!(auth.contains("/cn-beijing/ark/request"));
        assert!(auth.contains("SignedHeaders=host;x-content-sha256;x-date"));
        assert!(auth.contains("Signature="));
    }

    /// 验证不同的 body 产生不同的签名
    #[test]
    fn test_different_body_produces_different_signature() {
        let mk = |body: &'static [u8]| {
            sign_volc_request(VolcSignParams {
                access_key_id: "AK",
                secret_access_key: "SK",
                region: "cn-beijing",
                service: "ark",
                host: "open.volcengineapi.com",
                method: "POST",
                query: "Action=GetAFPUsage&Version=2024-01-01",
                body,
            })
        };

        let auth_a = mk(b"{}")
            .iter()
            .find(|(k, _)| k == "Authorization")
            .map(|(_, v)| v.clone())
            .unwrap();
        let auth_b = mk(br#"{"k":"v"}"#)
            .iter()
            .find(|(k, _)| k == "Authorization")
            .map(|(_, v)| v.clone())
            .unwrap();

        assert_ne!(auth_a, auth_b);
    }
}
