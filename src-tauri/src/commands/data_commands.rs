use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use chrono::Local;
use reqwest::{Client, Method, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, State};
use url::Url;

use crate::config::app_config::{AppConfig, AppSettings, ConfigFile};
use crate::config::encryption::KeyStore;
use crate::config::system_env::sync_active_api_key_envs;
use crate::stats::UsageStatsStore;

const WEBDAV_DEFAULT_FILENAME: &str = "peekausage-data.json";
const WEBDAV_TIMEOUT_SECONDS: u64 = 30;
const WEBDAV_PASSWORD_STORAGE_KEY: &str = "__peekausage_webdav_password";
const WEBDAV_AUTO_SYNC_POLL_SECONDS: u64 = 2;
const WEBDAV_AUTO_SYNC_RETRY_SECONDS: u64 = 30;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppDataSnapshot {
    pub config: ConfigFile,
    pub keys: HashMap<String, String>,
    pub usage_stats: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebDavSyncConfig {
    pub endpoint: String,
    pub username: String,
    pub password: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct SettingsChangedPayload {
    source: String,
    settings: AppSettings,
}

/// 导出完整快照，包含配置、密钥和用量历史。
#[tauri::command]
pub async fn export_app_data(
    app_config: State<'_, AppConfig>,
    key_store: State<'_, KeyStore>,
    usage_stats_store: State<'_, UsageStatsStore>,
) -> Result<AppDataSnapshot, String> {
    build_app_data_snapshot(
        app_config.inner(),
        key_store.inner(),
        usage_stats_store.inner(),
    )
    .await
}

#[tauri::command]
pub async fn export_app_data_to_downloads(
    app_handle: AppHandle,
    app_config: State<'_, AppConfig>,
    key_store: State<'_, KeyStore>,
    usage_stats_store: State<'_, UsageStatsStore>,
) -> Result<String, String> {
    let snapshot = build_app_data_snapshot(
        app_config.inner(),
        key_store.inner(),
        usage_stats_store.inner(),
    )
    .await?;
    let download_dir = app_handle
        .path()
        .download_dir()
        .map_err(|error| format!("无法确定系统下载目录: {error}"))?;
    let path = save_snapshot_to_directory(&snapshot, &download_dir)?;
    Ok(path.to_string_lossy().into_owned())
}

async fn build_app_data_snapshot(
    app_config: &AppConfig,
    key_store: &KeyStore,
    usage_stats_store: &UsageStatsStore,
) -> Result<AppDataSnapshot, String> {
    let config = app_config.get_snapshot().await;
    let mut keys = key_store.get_snapshot().await;
    keys.remove(WEBDAV_PASSWORD_STORAGE_KEY);
    let usage_stats = usage_stats_store.get_snapshot_json().await?;

    Ok(AppDataSnapshot {
        config,
        keys,
        usage_stats,
    })
}

fn save_snapshot_to_directory(
    snapshot: &AppDataSnapshot,
    directory: &Path,
) -> Result<PathBuf, String> {
    std::fs::create_dir_all(directory)
        .map_err(|error| format!("创建导出目录失败: {error}"))?;
    let content = serde_json::to_vec_pretty(snapshot)
        .map_err(|error| format!("序列化导出数据失败: {error}"))?;
    let filename = format!(
        "peekausage-data-{}.json",
        Local::now().format("%Y-%m-%d-%H%M%S-%3f")
    );
    let path = directory.join(filename);
    crate::config::atomic::atomic_write(&path, &content)
        .map_err(|error| format!("写入导出文件失败: {error}"))?;
    Ok(path)
}

/// 用快照覆盖当前全部应用数据：配置、密钥和用量历史。
#[tauri::command]
pub async fn import_app_data(
    snapshot: AppDataSnapshot,
    app_config: State<'_, AppConfig>,
    key_store: State<'_, KeyStore>,
    usage_stats_store: State<'_, UsageStatsStore>,
    app_handle: AppHandle,
) -> Result<(), String> {
    import_app_data_into_stores(
        snapshot,
        app_config.inner(),
        key_store.inner(),
        usage_stats_store.inner(),
    )
    .await?;
    sync_active_api_key_envs(app_config.inner(), key_store.inner()).await?;

    let settings = app_config.get_settings().await;
    let payload = SettingsChangedPayload {
        source: "import_app_data".to_string(),
        settings,
    };
    let _ = app_handle.emit("settings-changed", payload);

    Ok(())
}

async fn import_app_data_into_stores(
    snapshot: AppDataSnapshot,
    app_config: &AppConfig,
    key_store: &KeyStore,
    usage_stats_store: &UsageStatsStore,
) -> Result<(), String> {
    usage_stats_store.validate_snapshot_json(&snapshot.usage_stats)?;

    let AppDataSnapshot {
        config,
        mut keys,
        usage_stats,
    } = snapshot;
    let preserved_webdav_password = key_store
        .get_stored_key(WEBDAV_PASSWORD_STORAGE_KEY)
        .await;
    keys.remove(WEBDAV_PASSWORD_STORAGE_KEY);
    if let Some(password) = preserved_webdav_password.filter(|value| !value.is_empty()) {
        keys.insert(WEBDAV_PASSWORD_STORAGE_KEY.to_string(), password);
    }
    app_config.replace_all(config).await?;
    key_store.replace_all(keys).await?;
    usage_stats_store.replace_all_from_json(usage_stats).await
}

/// 上传快照到 WebDAV（endpoint 可为目录或文件 URL）。
#[tauri::command]
pub async fn upload_app_data_to_webdav(
    snapshot: AppDataSnapshot,
    sync_config: WebDavSyncConfig,
) -> Result<(), String> {
    upload_snapshot_to_webdav(&snapshot, &sync_config).await
}

#[tauri::command]
pub async fn upload_app_data_to_webdav_with_root(
    snapshot: AppDataSnapshot,
    sync_config: WebDavSyncConfig,
    remote_root: String,
) -> Result<(), String> {
    upload_snapshot_to_webdav_with_root(&snapshot, &sync_config, Some(&remote_root)).await
}

async fn upload_snapshot_to_webdav(
    snapshot: &AppDataSnapshot,
    sync_config: &WebDavSyncConfig,
) -> Result<(), String> {
    upload_snapshot_to_webdav_with_root(snapshot, sync_config, None).await
}

async fn upload_snapshot_to_webdav_with_root(
    snapshot: &AppDataSnapshot,
    sync_config: &WebDavSyncConfig,
    remote_root: Option<&str>,
) -> Result<(), String> {
    let client = build_webdav_client()?;
    let endpoint = if let Some(remote_root) = remote_root.filter(|value| !value.trim().is_empty()) {
        ensure_webdav_remote_root(&client, sync_config, remote_root).await?;
        normalize_webdav_endpoint(&append_webdav_remote_root(
            &sync_config.endpoint,
            remote_root,
        )?)?
    } else {
        normalize_webdav_endpoint(&sync_config.endpoint)?
    };
    let payload = serde_json::to_vec(snapshot)
        .map_err(|error| format!("序列化快照失败: {error}"))?;

    let response = client
        .put(&endpoint)
        .basic_auth(&sync_config.username, Some(&sync_config.password))
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(payload)
        .send()
        .await
        .map_err(|error| format!("WebDAV 上传请求失败: {error}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let message = response
            .text()
            .await
            .unwrap_or_else(|_| String::from("无法读取响应内容"));

        return Err(format!("WebDAV 上传失败: {status} {message}"));
    }

    Ok(())
}

/// 从 WebDAV 下载快照。
#[tauri::command]
pub async fn download_app_data_from_webdav(
    sync_config: WebDavSyncConfig,
) -> Result<AppDataSnapshot, String> {
    let endpoint = normalize_webdav_endpoint(&sync_config.endpoint)?;
    let client = build_webdav_client()?;

    let response = client
        .get(&endpoint)
        .basic_auth(sync_config.username, Some(sync_config.password))
        .send()
        .await
        .map_err(|error| format!("WebDAV 下载请求失败: {error}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let message = response
            .text()
            .await
            .unwrap_or_else(|_| String::from("无法读取响应内容"));

        return Err(format!("WebDAV 下载失败: {status} {message}"));
    }

    let payload = response
        .text()
        .await
        .map_err(|error| format!("读取 WebDAV 响应失败: {error}"))?;
    serde_json::from_str::<AppDataSnapshot>(&payload)
        .map_err(|error| format!("解析快照内容失败: {error}"))
}

#[tauri::command]
pub async fn get_webdav_sync_password(
    key_store: State<'_, KeyStore>,
) -> Result<String, String> {
    Ok(key_store
        .get_stored_key(WEBDAV_PASSWORD_STORAGE_KEY)
        .await
        .unwrap_or_default())
}

#[tauri::command]
pub async fn save_webdav_sync_password(
    password: String,
    key_store: State<'_, KeyStore>,
) -> Result<(), String> {
    key_store
        .set_key(WEBDAV_PASSWORD_STORAGE_KEY, &password)
        .await
}

pub fn start_webdav_auto_sync(app_handle: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut ticker =
            tokio::time::interval(Duration::from_secs(WEBDAV_AUTO_SYNC_POLL_SECONDS));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        let mut synced_fingerprint: Option<Vec<u8>> = None;
        let mut failed_fingerprint: Option<Vec<u8>> = None;
        let mut retry_after: Option<Instant> = None;

        loop {
            ticker.tick().await;

            let app_config = app_handle.state::<AppConfig>();
            let key_store = app_handle.state::<KeyStore>();
            let usage_stats_store = app_handle.state::<UsageStatsStore>();
            let settings = app_config.get_settings().await;
            let password = key_store
                .get_stored_key(WEBDAV_PASSWORD_STORAGE_KEY)
                .await
                .unwrap_or_default();
            let snapshot = match build_app_data_snapshot(
                app_config.inner(),
                key_store.inner(),
                usage_stats_store.inner(),
            )
            .await
            {
                Ok(snapshot) => snapshot,
                Err(error) => {
                    eprintln!("生成 WebDAV 自动同步快照失败: {error}");
                    continue;
                }
            };
            let fingerprint = match build_auto_sync_fingerprint(&snapshot, &password) {
                Ok(fingerprint) => fingerprint,
                Err(error) => {
                    eprintln!("生成 WebDAV 自动同步指纹失败: {error}");
                    continue;
                }
            };

            if synced_fingerprint.is_none() {
                synced_fingerprint = Some(fingerprint);
                continue;
            }

            if !settings.webdav_auto_sync_enabled {
                synced_fingerprint = Some(fingerprint);
                failed_fingerprint = None;
                retry_after = None;
                continue;
            }

            if synced_fingerprint.as_ref() == Some(&fingerprint) {
                continue;
            }

            let is_waiting_to_retry = failed_fingerprint.as_ref() == Some(&fingerprint)
                && retry_after.is_some_and(|deadline| Instant::now() < deadline);
            if is_waiting_to_retry {
                continue;
            }

            let sync_config = match configured_webdav_sync_config(&settings, password) {
                Ok(sync_config) => sync_config,
                Err(error) => {
                    eprintln!("WebDAV 自动同步配置无效: {error}");
                    failed_fingerprint = Some(fingerprint);
                    retry_after = Some(
                        Instant::now() + Duration::from_secs(WEBDAV_AUTO_SYNC_RETRY_SECONDS),
                    );
                    continue;
                }
            };

            match upload_snapshot_to_webdav_with_root(
                &snapshot,
                &sync_config,
                Some(&settings.webdav_remote_root),
            )
            .await
            {
                Ok(()) => {
                    synced_fingerprint = Some(fingerprint);
                    failed_fingerprint = None;
                    retry_after = None;
                }
                Err(error) => {
                    eprintln!("WebDAV 自动同步上传失败: {error}");
                    failed_fingerprint = Some(fingerprint);
                    retry_after = Some(
                        Instant::now() + Duration::from_secs(WEBDAV_AUTO_SYNC_RETRY_SECONDS),
                    );
                }
            }
        }
    });
}

fn build_auto_sync_fingerprint(
    snapshot: &AppDataSnapshot,
    password: &str,
) -> Result<Vec<u8>, String> {
    let mut fingerprint = serde_json::to_vec(snapshot)
        .map_err(|error| format!("序列化自动同步快照失败: {error}"))?;
    fingerprint.extend_from_slice(b"\0webdav-password\0");
    fingerprint.extend_from_slice(password.as_bytes());
    Ok(fingerprint)
}

fn configured_webdav_sync_config(
    settings: &AppSettings,
    password: String,
) -> Result<WebDavSyncConfig, String> {
    if settings.webdav_endpoint.trim().is_empty() {
        return Err(String::from("请先填写 WebDAV 服务地址"));
    }
    if settings.webdav_username.trim().is_empty() {
        return Err(String::from("请先填写 WebDAV 用户名"));
    }
    if password.is_empty() {
        return Err(String::from("请先填写 WebDAV 密码"));
    }

    Ok(WebDavSyncConfig {
        endpoint: settings.webdav_endpoint.trim().to_string(),
        username: settings.webdav_username.trim().to_string(),
        password,
    })
}

fn webdav_remote_root_segments(remote_root: &str) -> Result<Vec<String>, String> {
    let normalized_root = remote_root.trim().replace('\\', "/");
    let segments: Vec<String> = normalized_root
        .split('/')
        .filter(|segment| !segment.is_empty())
        .map(ToString::to_string)
        .collect();
    if segments
        .iter()
        .any(|segment| matches!(segment.as_str(), "." | ".."))
    {
        return Err(String::from("WebDAV 远程根目录不能包含 . 或 .. 路径段"));
    }
    Ok(segments)
}

fn append_webdav_remote_root(
    raw_endpoint: &str,
    remote_root: &str,
) -> Result<String, String> {
    let trimmed_endpoint = raw_endpoint.trim();
    if remote_root.trim().is_empty() {
        return Ok(trimmed_endpoint.to_string());
    }

    let mut endpoint =
        Url::parse(trimmed_endpoint).map_err(|error| format!("WebDAV 地址无效: {error}"))?;
    if !matches!(endpoint.scheme(), "http" | "https") {
        return Err(String::from("WebDAV 地址仅支持 HTTP 或 HTTPS"));
    }

    let segments = webdav_remote_root_segments(remote_root)?;

    let mut path = endpoint.path().trim_end_matches('/').to_string();
    for segment in segments {
        path.push('/');
        path.push_str(&segment);
    }
    path.push('/');
    endpoint.set_path(&path);
    Ok(endpoint.to_string())
}

fn webdav_remote_collection_urls(
    raw_endpoint: &str,
    remote_root: &str,
) -> Result<Vec<Url>, String> {
    let mut endpoint =
        Url::parse(raw_endpoint.trim()).map_err(|error| format!("WebDAV 地址无效: {error}"))?;
    if !matches!(endpoint.scheme(), "http" | "https") {
        return Err(String::from("WebDAV 地址仅支持 HTTP 或 HTTPS"));
    }

    let segments = webdav_remote_root_segments(remote_root)?;
    let mut path = endpoint.path().trim_end_matches('/').to_string();
    let mut collections = Vec::with_capacity(segments.len());
    for segment in segments {
        path.push('/');
        path.push_str(&segment);
        endpoint.set_path(&format!("{path}/"));
        collections.push(endpoint.clone());
    }
    Ok(collections)
}

async fn ensure_webdav_remote_root(
    client: &Client,
    sync_config: &WebDavSyncConfig,
    remote_root: &str,
) -> Result<(), String> {
    let mkcol = Method::from_bytes(b"MKCOL")
        .map_err(|error| format!("初始化 WebDAV MKCOL 请求失败: {error}"))?;

    for collection in webdav_remote_collection_urls(&sync_config.endpoint, remote_root)? {
        let response = client
            .request(mkcol.clone(), collection.clone())
            .basic_auth(&sync_config.username, Some(&sync_config.password))
            .send()
            .await
            .map_err(|error| format!("创建 WebDAV 远程目录失败: {error}"))?;
        let status = response.status();
        if status.is_success() || status == StatusCode::METHOD_NOT_ALLOWED {
            continue;
        }

        let message = response
            .text()
            .await
            .unwrap_or_else(|_| String::from("无法读取响应内容"));
        return Err(format!(
            "创建 WebDAV 远程目录失败: {status} {collection} {message}"
        ));
    }

    Ok(())
}

fn build_webdav_client() -> Result<Client, String> {
    Client::builder()
        .timeout(Duration::from_secs(WEBDAV_TIMEOUT_SECONDS))
        .build()
        .map_err(|error| format!("初始化 HTTP 客户端失败: {error}"))
}

fn normalize_webdav_endpoint(raw_endpoint: &str) -> Result<String, String> {
    let trimmed = raw_endpoint.trim();
    if trimmed.is_empty() {
        return Err(String::from("请先填写 WebDAV 地址"));
    }

    let mut endpoint = Url::parse(trimmed).map_err(|error| format!("WebDAV 地址无效: {error}"))?;
    if !matches!(endpoint.scheme(), "http" | "https") {
        return Err(String::from("WebDAV 地址仅支持 HTTP 或 HTTPS"));
    }

    let path = endpoint.path().to_string();
    let path_last = path.rsplit('/').next().unwrap_or("");
    let is_directory = path.ends_with('/') || path_last.is_empty() || !path_last.contains('.');

    if is_directory {
        let mut path = path;
        if !path.ends_with('/') {
            path.push('/');
        }
        path.push_str(WEBDAV_DEFAULT_FILENAME);
        endpoint.set_path(&path);
    }

    Ok(endpoint.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    fn sample_snapshot() -> AppDataSnapshot {
        AppDataSnapshot {
            config: ConfigFile {
                settings: AppSettings::default(),
                providers: HashMap::new(),
                provider_order: Vec::new(),
            },
            keys: HashMap::from([("openai:test".to_string(), "secret".to_string())]),
            usage_stats: serde_json::json!({
                "version": 1,
                "providers": {}
            }),
        }
    }

    fn temp_data_dir(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "peekausage_data_commands_{label}_{}",
            uuid::Uuid::new_v4()
        ))
    }

    async fn start_one_shot_server(response_body: String) -> (String, tokio::task::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut chunk = [0_u8; 4096];

            loop {
                let read = socket.read(&mut chunk).await.unwrap();
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&chunk[..read]);

                let Some(headers_end) = request.windows(4).position(|window| window == b"\r\n\r\n") else {
                    continue;
                };
                let headers_end = headers_end + 4;
                let headers = String::from_utf8_lossy(&request[..headers_end]);
                let content_length = headers
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().ok())
                            .flatten()
                    })
                    .unwrap_or(0);

                if request.len() >= headers_end + content_length {
                    break;
                }
            }

            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response_body.len(),
                response_body
            );
            socket.write_all(response.as_bytes()).await.unwrap();
            String::from_utf8(request).unwrap()
        });

        (format!("http://{address}/backup.json"), handle)
    }

    #[test]
    fn directory_endpoint_appends_default_filename() {
        let endpoint = normalize_webdav_endpoint("https://dav.example.test/backups/").unwrap();
        assert_eq!(
            endpoint,
            "https://dav.example.test/backups/peekausage-data.json"
        );
    }

    #[test]
    fn non_http_endpoint_is_rejected() {
        let error = normalize_webdav_endpoint("file:///tmp/backup.json").unwrap_err();
        assert!(error.contains("HTTP"));
    }

    #[test]
    fn saving_snapshot_writes_a_real_json_file() {
        let data_dir = temp_data_dir("local_export");
        std::fs::create_dir_all(&data_dir).unwrap();
        let snapshot = sample_snapshot();

        let path = save_snapshot_to_directory(&snapshot, &data_dir).unwrap();
        let written = std::fs::read_to_string(&path).unwrap();

        assert_eq!(path.parent(), Some(data_dir.as_path()));
        assert_eq!(
            serde_json::from_str::<Value>(&written).unwrap(),
            serde_json::to_value(snapshot).unwrap()
        );

        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn upload_uses_put_basic_auth_and_json_snapshot() {
        let (endpoint, request_handle) = start_one_shot_server(String::new()).await;
        let snapshot = sample_snapshot();

        upload_app_data_to_webdav(
            snapshot.clone(),
            WebDavSyncConfig {
                endpoint,
                username: "tester".to_string(),
                password: "secret".to_string(),
            },
        )
        .await
        .unwrap();

        let request = request_handle.await.unwrap();
        let (headers, body) = request.split_once("\r\n\r\n").unwrap();
        assert!(headers.starts_with("PUT /backup.json HTTP/1.1"));
        assert!(headers
            .lines()
            .any(|line| line.eq_ignore_ascii_case("authorization: Basic dGVzdGVyOnNlY3JldA==")));
        assert_eq!(
            serde_json::from_str::<Value>(body).unwrap(),
            serde_json::to_value(snapshot).unwrap()
        );
    }

    #[tokio::test]
    async fn download_uses_get_and_parses_snapshot() {
        let snapshot = sample_snapshot();
        let response_body = serde_json::to_string(&snapshot).unwrap();
        let (endpoint, request_handle) = start_one_shot_server(response_body).await;

        let downloaded = download_app_data_from_webdav(WebDavSyncConfig {
            endpoint,
            username: "tester".to_string(),
            password: "secret".to_string(),
        })
        .await
        .unwrap();

        let request = request_handle.await.unwrap();
        assert!(request.starts_with("GET /backup.json HTTP/1.1"));
        assert_eq!(
            serde_json::to_value(downloaded).unwrap(),
            serde_json::to_value(snapshot).unwrap()
        );
    }

    #[tokio::test]
    async fn valid_snapshot_replaces_config_keys_and_usage_stats() {
        let data_dir = temp_data_dir("valid_import");
        let app_config = AppConfig::new(data_dir.clone());
        let key_store = KeyStore::new(data_dir.clone());
        let usage_stats_store = UsageStatsStore::new(data_dir.clone());
        key_store.set_key("existing", "old-value").await.unwrap();

        let mut snapshot = sample_snapshot();
        snapshot.config.settings.polling_interval = 99;
        let expected_config = serde_json::to_value(&snapshot.config).unwrap();
        let expected_keys = snapshot.keys.clone();
        let expected_stats = snapshot.usage_stats.clone();

        import_app_data_into_stores(
            snapshot,
            &app_config,
            &key_store,
            &usage_stats_store,
        )
        .await
        .unwrap();

        assert_eq!(
            serde_json::to_value(app_config.get_snapshot().await).unwrap(),
            expected_config
        );
        assert_eq!(key_store.get_snapshot().await, expected_keys);
        assert_eq!(
            usage_stats_store.get_snapshot_json().await.unwrap(),
            expected_stats
        );

        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn invalid_usage_stats_do_not_overwrite_existing_data() {
        let data_dir = temp_data_dir("invalid_import");
        let app_config = AppConfig::new(data_dir.clone());
        let key_store = KeyStore::new(data_dir.clone());
        let usage_stats_store = UsageStatsStore::new(data_dir.clone());
        key_store.set_key("existing", "keep-me").await.unwrap();

        let original_config = serde_json::to_value(app_config.get_snapshot().await).unwrap();
        let original_keys = key_store.get_snapshot().await;
        let original_stats = usage_stats_store.get_snapshot_json().await.unwrap();

        let mut snapshot = sample_snapshot();
        snapshot.config.settings.polling_interval = 99;
        snapshot.keys = HashMap::from([("replacement".to_string(), "new-value".to_string())]);
        snapshot.usage_stats = serde_json::json!({
            "version": 1,
            "providers": []
        });

        let result = import_app_data_into_stores(
            snapshot,
            &app_config,
            &key_store,
            &usage_stats_store,
        )
        .await;

        assert!(result.is_err());
        assert_eq!(
            serde_json::to_value(app_config.get_snapshot().await).unwrap(),
            original_config
        );
        assert_eq!(key_store.get_snapshot().await, original_keys);
        assert_eq!(
            usage_stats_store.get_snapshot_json().await.unwrap(),
            original_stats
        );

        let _ = std::fs::remove_dir_all(data_dir);
    }
}
