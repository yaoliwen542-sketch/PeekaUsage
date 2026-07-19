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

    /// 获取掩码后的 Key（用于前端展示）
    pub async fn get_masked_key(&self, storage_key: &str, env_var_name: Option<&str>) -> String {
        let value = if let Some(env_var_name) = env_var_name {
            self.get_key(storage_key, env_var_name).await
        } else {
            self.get_stored_key(storage_key).await
        };

        match value {
            Some(key) if key.len() > 8 => {
                let prefix = &key[..4];
                let suffix = &key[key.len() - 4..];
                format!("{}...{}", prefix, suffix)
            }
            Some(_) => "****".to_string(),
            None => String::new(),
        }
    }
}
