use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

/// 简易加密存储（基于文件的 Key 管理）
///
/// 实际生产环境应使用 tauri-plugin-stronghold 或系统 keyring。
/// 这里先用简单的 base64 编码 + 文件存储实现 MVP，后续可升级。
pub struct KeyStore {
    keys: Arc<RwLock<HashMap<String, String>>>,
    store_path: PathBuf,
}

impl KeyStore {
    pub fn new(app_data_dir: PathBuf) -> Self {
        let store_path = app_data_dir.join("keys.dat");
        let keys = if store_path.exists() {
            match std::fs::read_to_string(&store_path) {
                Ok(content) => match Self::decode_keys(&content) {
                    Some(keys) => keys,
                    None => {
                        // 修复 M9：密钥文件解码失败（写半截/被外部改坏）时，
                        // 备份为 keys.dat.bak 再回退空存储，避免数据彻底丢失。
                        // 空文件视为尚未写入的新文件，不算损坏。
                        if content.trim().is_empty() {
                            HashMap::new()
                        } else {
                            eprintln!("解码 keys.dat 失败，回退空密钥存储");
                            crate::config::atomic::backup_corrupted_file(&store_path);
                            HashMap::new()
                        }
                    }
                },
                Err(_) => HashMap::new(),
            }
        } else {
            HashMap::new()
        };

        Self {
            keys: Arc::new(RwLock::new(keys)),
            store_path,
        }
    }

    fn encode_keys(keys: &HashMap<String, String>) -> String {
        let json = serde_json::to_string(keys).unwrap_or_default();
        use base64::Engine;
        base64::engine::general_purpose::STANDARD.encode(json.as_bytes())
    }

    /// 解码密钥文件；任一步骤失败返回 None（调用方据此判定文件损坏）
    fn decode_keys(encoded: &str) -> Option<HashMap<String, String>> {
        use base64::Engine;
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(encoded.trim().as_bytes())
            .ok()?;
        let json_str = String::from_utf8(decoded).ok()?;
        serde_json::from_str(&json_str).ok()
    }

    async fn save(&self) -> Result<(), String> {
        let keys = self.keys.read().await;
        let encoded = Self::encode_keys(&*keys);

        if let Some(parent) = self.store_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("创建密钥目录失败: {}", e))?;
        }

        // 修复 M9：原子写入（tmp + rename），写入中途崩溃不会留下半截 keys.dat
        crate::config::atomic::atomic_write(&self.store_path, encoded.as_bytes())
            .map_err(|e| format!("保存密钥失败: {}", e))?;

        Ok(())
    }

    /// 获取存储中的原始 Key，不读取环境变量
    pub async fn get_stored_key(&self, storage_key: &str) -> Option<String> {
        let keys = self.keys.read().await;
        keys.get(storage_key).cloned()
    }

    /// 获取 Key，优先读取环境变量，再回退到存储
    pub async fn get_key(&self, storage_key: &str, env_var_name: &str) -> Option<String> {
        if let Ok(key) = std::env::var(env_var_name) {
            if !key.is_empty() {
                return Some(key);
            }
        }

        self.get_stored_key(storage_key).await
    }

    /// 存储 Key
    pub async fn set_key(&self, storage_key: &str, key_value: &str) -> Result<(), String> {
        {
            let mut keys = self.keys.write().await;
            if key_value.is_empty() {
                keys.remove(storage_key);
            } else {
                keys.insert(storage_key.to_string(), key_value.to_string());
            }
        }
        self.save().await
    }
}

/// 生成密钥掩码串（前 4 字符 + "..." + 后 4 字符），用于前端回显。
///
/// 修复 L6：按 char 边界切片而非字节切片——多字节 UTF-8 字符落在
/// 第 4 字节边界时，`&value[..4]` 会直接 panic。长度判定同样按字符数。
///
/// 前端回显和「掩码占位符」判定（is_masked_placeholder）共用这一份实现，
/// 保证下发的掩码与回传比对时逐字符一致。
pub fn mask_secret(value: &str) -> String {
    if value.is_empty() {
        return String::new();
    }

    let char_count = value.chars().count();
    if char_count <= 8 {
        return "****".to_string();
    }

    let prefix: String = value.chars().take(4).collect();
    let suffix: String = value.chars().skip(char_count.saturating_sub(4)).collect();
    format!("{}...{}", prefix, suffix)
}

/// 判断前端回传的值是否为「未修改的掩码占位符」（即我们之前下发的掩码串）。
///
/// 修复 L6：不再用 `contains("...")` 判定——真实包含 "..." 的 key 会被误判为
/// 占位符而永远无法保存。改为与「上次保存值的掩码串」精确比较，只有前端
/// 原样回传了我们下发的掩码时才跳过写入。
pub fn is_masked_placeholder(value: &str, stored: Option<&str>) -> bool {
    match stored {
        Some(stored) if !stored.is_empty() => value == mask_secret(stored),
        _ => false,
    }
}
