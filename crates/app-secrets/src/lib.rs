//! app-secrets：敏感信息存取抽象与后端实现。

use std::collections::{BTreeSet, HashMap};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use argon2::{Algorithm, Argon2, Params, Version};
use keyring::Entry;
use thiserror::Error;
use zeroize::Zeroizing;

const REDACTED: &str = "***REDACTED***";
const FILE_FORMAT_VERSION: u8 = 1;
const FILE_SALT_LEN: usize = 16;
const FILE_NONCE_LEN: usize = 12;
const FILE_TAG_LEN: usize = 16;
const ARGON2_MEMORY_KIB: u32 = 64 * 1024;
const ARGON2_ITERATIONS: u32 = 3;
const ARGON2_PARALLELISM: u32 = 1;
const KEYRING_SERVICE: &str = "subforge";
const KEYRING_INDEX_KEY: &str = "__keys__";

pub type SecretResult<T> = Result<T, SecretError>;

#[derive(Debug, Error)]
pub enum SecretError {
    #[error("无效 scope：{0}")]
    InvalidScope(String),
    #[error("无效 key：{0}")]
    InvalidKey(String),
    #[error("密钥不存在：{0}")]
    SecretMissing(String),
    #[error("后端错误：{0}")]
    Backend(String),
    #[error("IO 错误：{0}")]
    Io(#[from] std::io::Error),
    #[error("序列化错误：{0}")]
    Serde(#[from] serde_json::Error),
}

impl SecretError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidScope(_) | Self::InvalidKey(_) => "E_CONFIG_INVALID",
            Self::SecretMissing(_) => "E_SECRET_MISSING",
            Self::Backend(_) | Self::Io(_) | Self::Serde(_) => "E_INTERNAL",
        }
    }
}

/// 密钥存储统一接口。返回值使用 `Zeroizing<String>` 保证 drop 时清零。
pub trait SecretStore: Send + Sync + fmt::Debug {
    fn set(&self, scope: &str, key: &str, value: &str) -> SecretResult<()>;
    fn get(&self, scope: &str, key: &str) -> SecretResult<Zeroizing<String>>;
    fn delete(&self, scope: &str, key: &str) -> SecretResult<()>;
    fn list_keys(&self, scope: &str) -> SecretResult<Vec<String>>;
}

/// 用于日志/调试输出的密钥遮罩视图。
pub struct RedactedSecret<'a> {
    _value: &'a str,
}

pub fn redact_secret(value: &str) -> RedactedSecret<'_> {
    RedactedSecret { _value: value }
}

impl fmt::Debug for RedactedSecret<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(REDACTED)
    }
}

impl fmt::Display for RedactedSecret<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(REDACTED)
    }
}

#[derive(Default)]
pub struct MemorySecretStore {
    data: Mutex<HashMap<String, HashMap<String, String>>>,
}

impl MemorySecretStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl fmt::Debug for MemorySecretStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MemorySecretStore")
            .field("data", &REDACTED)
            .finish()
    }
}

impl SecretStore for MemorySecretStore {
    fn set(&self, scope: &str, key: &str, value: &str) -> SecretResult<()> {
        validate_scope(scope)?;
        validate_key(key)?;

        let mut data = lock_mutex(&self.data, "memory.data")?;
        data.entry(scope.to_string())
            .or_default()
            .insert(key.to_string(), value.to_string());
        Ok(())
    }

    fn get(&self, scope: &str, key: &str) -> SecretResult<Zeroizing<String>> {
        validate_scope(scope)?;
        validate_key(key)?;

        let data = lock_mutex(&self.data, "memory.data")?;
        let value = data
            .get(scope)
            .and_then(|scope_map| scope_map.get(key))
            .cloned()
            .ok_or_else(|| SecretError::SecretMissing(storage_key(scope, key)))?;

        Ok(Zeroizing::new(value))
    }

    fn delete(&self, scope: &str, key: &str) -> SecretResult<()> {
        validate_scope(scope)?;
        validate_key(key)?;

        let mut data = lock_mutex(&self.data, "memory.data")?;
        if let Some(scope_map) = data.get_mut(scope) {
            scope_map.remove(key);
            if scope_map.is_empty() {
                data.remove(scope);
            }
        }
        Ok(())
    }

    fn list_keys(&self, scope: &str) -> SecretResult<Vec<String>> {
        validate_scope(scope)?;

        let data = lock_mutex(&self.data, "memory.data")?;
        let mut keys = data
            .get(scope)
            .map(|scope_map| scope_map.keys().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        keys.sort();
        Ok(keys)
    }
}

#[derive(Default)]
pub struct EnvSecretStore {
    overrides: Mutex<HashMap<String, HashMap<String, Option<String>>>>,
}

impl EnvSecretStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl fmt::Debug for EnvSecretStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EnvSecretStore")
            .field("overrides", &REDACTED)
            .finish()
    }
}

impl SecretStore for EnvSecretStore {
    fn set(&self, scope: &str, key: &str, value: &str) -> SecretResult<()> {
        validate_scope(scope)?;
        validate_key(key)?;

        let mut overrides = lock_mutex(&self.overrides, "env.overrides")?;
        overrides
            .entry(scope.to_string())
            .or_default()
            .insert(key.to_string(), Some(value.to_string()));
        Ok(())
    }

    fn get(&self, scope: &str, key: &str) -> SecretResult<Zeroizing<String>> {
        validate_scope(scope)?;
        validate_key(key)?;

        {
            let overrides = lock_mutex(&self.overrides, "env.overrides")?;
            if let Some(value) = overrides
                .get(scope)
                .and_then(|scope_map| scope_map.get(key))
                .cloned()
            {
                return value
                    .map(Zeroizing::new)
                    .ok_or_else(|| SecretError::SecretMissing(storage_key(scope, key)));
            }
        }

        let env_key = env_key(scope, key);
        let value = std::env::var(&env_key)
            .map_err(|_| SecretError::SecretMissing(storage_key(scope, key)))?;
        Ok(Zeroizing::new(value))
    }

    fn delete(&self, scope: &str, key: &str) -> SecretResult<()> {
        validate_scope(scope)?;
        validate_key(key)?;

        let mut overrides = lock_mutex(&self.overrides, "env.overrides")?;
        overrides
            .entry(scope.to_string())
            .or_default()
            .insert(key.to_string(), None);
        Ok(())
    }

    fn list_keys(&self, scope: &str) -> SecretResult<Vec<String>> {
        validate_scope(scope)?;

        let prefix = format!("SUBFORGE_{}_", normalize_for_env(scope));
        let mut keys = BTreeSet::new();

        for (name, _) in std::env::vars() {
            if let Some(key_suffix) = name.strip_prefix(&prefix) {
                keys.insert(key_suffix.to_string());
            }
        }

        let overrides = lock_mutex(&self.overrides, "env.overrides")?;
        if let Some(scope_map) = overrides.get(scope) {
            for (key, value) in scope_map {
                if value.is_some() {
                    keys.insert(key.to_string());
                } else {
                    keys.remove(key);
                    keys.remove(&normalize_for_env(key));
                }
            }
        }

        Ok(keys.into_iter().collect())
    }
}

#[derive(Default)]
pub struct KeyringSecretStore;

impl KeyringSecretStore {
    pub fn new() -> Self {
        Self
    }

    fn secret_entry(&self, scope: &str, key: &str) -> SecretResult<Entry> {
        let storage = storage_key(scope, key);
        Entry::new(KEYRING_SERVICE, &storage)
            .map_err(|error| SecretError::Backend(format!("keyring 条目创建失败：{error}")))
    }

    fn index_entry(&self, scope: &str) -> SecretResult<Entry> {
        self.secret_entry(scope, KEYRING_INDEX_KEY)
    }

    fn load_scope_index(&self, scope: &str) -> SecretResult<BTreeSet<String>> {
        let entry = self.index_entry(scope)?;
        match entry.get_password() {
            Ok(value) => {
                let keys: Vec<String> = serde_json::from_str(&value)?;
                Ok(keys.into_iter().collect())
            }
            Err(keyring::Error::NoEntry) => Ok(BTreeSet::new()),
            Err(error) => Err(SecretError::Backend(format!(
                "读取 keyring 索引失败：{error}"
            ))),
        }
    }

    fn save_scope_index(&self, scope: &str, keys: &BTreeSet<String>) -> SecretResult<()> {
        let entry = self.index_entry(scope)?;
        let payload = serde_json::to_string(&keys.iter().collect::<Vec<_>>())?;

        entry
            .set_password(&payload)
            .map_err(|error| SecretError::Backend(format!("写入 keyring 索引失败：{error}")))
    }
}

impl fmt::Debug for KeyringSecretStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("KeyringSecretStore")
            .field("service", &KEYRING_SERVICE)
            .finish()
    }
}

impl SecretStore for KeyringSecretStore {
    fn set(&self, scope: &str, key: &str, value: &str) -> SecretResult<()> {
        validate_scope(scope)?;
        validate_key(key)?;

        let entry = self.secret_entry(scope, key)?;
        entry
            .set_password(value)
            .map_err(|error| SecretError::Backend(format!("写入 keyring 失败：{error}")))?;

        let mut keys = self.load_scope_index(scope)?;
        keys.insert(key.to_string());
        self.save_scope_index(scope, &keys)?;
        Ok(())
    }

    fn get(&self, scope: &str, key: &str) -> SecretResult<Zeroizing<String>> {
        validate_scope(scope)?;
        validate_key(key)?;

        let entry = self.secret_entry(scope, key)?;
        match entry.get_password() {
            Ok(value) => Ok(Zeroizing::new(value)),
            Err(keyring::Error::NoEntry) => {
                Err(SecretError::SecretMissing(storage_key(scope, key)))
            }
            Err(error) => Err(SecretError::Backend(format!("读取 keyring 失败：{error}"))),
        }
    }

    fn delete(&self, scope: &str, key: &str) -> SecretResult<()> {
        validate_scope(scope)?;
        validate_key(key)?;

        let entry = self.secret_entry(scope, key)?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => {}
            Err(error) => {
                return Err(SecretError::Backend(format!("删除 keyring 失败：{error}")));
            }
        }

        let mut keys = self.load_scope_index(scope)?;
        keys.remove(key);
        self.save_scope_index(scope, &keys)?;
        Ok(())
    }

    fn list_keys(&self, scope: &str) -> SecretResult<Vec<String>> {
        validate_scope(scope)?;
        Ok(self.load_scope_index(scope)?.into_iter().collect())
    }
}

pub struct FileSecretStore {
    path: PathBuf,
    master_key: Zeroizing<String>,
    lock: Mutex<()>,
}

impl FileSecretStore {
    pub fn new(path: impl AsRef<Path>, master_key: impl Into<String>) -> SecretResult<Self> {
        let path = path.as_ref().to_path_buf();
        let master_key = master_key.into();
        if master_key.trim().is_empty() {
            return Err(SecretError::InvalidKey("master_key 不能为空".to_string()));
        }

        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }

        Ok(Self {
            path,
            master_key: Zeroizing::new(master_key),
            lock: Mutex::new(()),
        })
    }

    fn load_data(&self) -> SecretResult<HashMap<String, HashMap<String, String>>> {
        if !self.path.exists() {
            return Ok(HashMap::new());
        }

        let payload = fs::read(&self.path)?;
        decrypt_payload(&payload, self.master_key.as_str())
    }

    fn save_data(&self, data: &HashMap<String, HashMap<String, String>>) -> SecretResult<()> {
        let payload = encrypt_payload(data, self.master_key.as_str())?;
        write_atomic(&self.path, &payload)?;
        ensure_file_permission(&self.path)?;
        Ok(())
    }
}

impl fmt::Debug for FileSecretStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FileSecretStore")
            .field("path", &self.path)
            .field("master_key", &REDACTED)
            .finish()
    }
}

impl SecretStore for FileSecretStore {
    fn set(&self, scope: &str, key: &str, value: &str) -> SecretResult<()> {
        validate_scope(scope)?;
        validate_key(key)?;

        let _guard = lock_mutex(&self.lock, "file.lock")?;
        let mut data = self.load_data()?;
        data.entry(scope.to_string())
            .or_default()
            .insert(key.to_string(), value.to_string());
        self.save_data(&data)
    }

    fn get(&self, scope: &str, key: &str) -> SecretResult<Zeroizing<String>> {
        validate_scope(scope)?;
        validate_key(key)?;

        let _guard = lock_mutex(&self.lock, "file.lock")?;
        let data = self.load_data()?;
        let value = data
            .get(scope)
            .and_then(|scope_map| scope_map.get(key))
            .cloned()
            .ok_or_else(|| SecretError::SecretMissing(storage_key(scope, key)))?;
        Ok(Zeroizing::new(value))
    }

    fn delete(&self, scope: &str, key: &str) -> SecretResult<()> {
        validate_scope(scope)?;
        validate_key(key)?;

        let _guard = lock_mutex(&self.lock, "file.lock")?;
        let mut data = self.load_data()?;
        if let Some(scope_map) = data.get_mut(scope) {
            scope_map.remove(key);
            if scope_map.is_empty() {
                data.remove(scope);
            }
            self.save_data(&data)?;
        }
        Ok(())
    }

    fn list_keys(&self, scope: &str) -> SecretResult<Vec<String>> {
        validate_scope(scope)?;

        let _guard = lock_mutex(&self.lock, "file.lock")?;
        let data = self.load_data()?;
        let mut keys = data
            .get(scope)
            .map(|scope_map| scope_map.keys().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        keys.sort();
        Ok(keys)
    }
}

fn validate_scope(scope: &str) -> SecretResult<()> {
    if scope == "system" {
        return Ok(());
    }

    if let Some(plugin_id) = scope.strip_prefix("plugin:") {
        if !plugin_id.is_empty() && plugin_id.chars().all(is_allowed_scope_char) {
            return Ok(());
        }
    }

    Err(SecretError::InvalidScope(scope.to_string()))
}

fn validate_key(key: &str) -> SecretResult<()> {
    if !key.is_empty() && key.chars().all(is_allowed_key_char) {
        return Ok(());
    }

    Err(SecretError::InvalidKey(key.to_string()))
}

fn is_allowed_scope_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-')
}

fn is_allowed_key_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-')
}

fn storage_key(scope: &str, key: &str) -> String {
    format!("subforge:{scope}:{key}")
}

fn env_key(scope: &str, key: &str) -> String {
    format!(
        "SUBFORGE_{}_{}",
        normalize_for_env(scope),
        normalize_for_env(key)
    )
}

fn normalize_for_env(input: &str) -> String {
    input
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect()
}

fn lock_mutex<'a, T>(mutex: &'a Mutex<T>, name: &str) -> SecretResult<MutexGuard<'a, T>> {
    mutex
        .lock()
        .map_err(|_| SecretError::Backend(format!("互斥锁已中毒：{name}")))
}

fn encrypt_payload(
    data: &HashMap<String, HashMap<String, String>>,
    master_key: &str,
) -> SecretResult<Vec<u8>> {
    let plaintext = serde_json::to_vec(data)?;

    let mut salt = [0_u8; FILE_SALT_LEN];
    let mut nonce = [0_u8; FILE_NONCE_LEN];
    getrandom::fill(&mut salt)
        .map_err(|error| SecretError::Backend(format!("生成 salt 失败：{error}")))?;
    getrandom::fill(&mut nonce)
        .map_err(|error| SecretError::Backend(format!("生成 nonce 失败：{error}")))?;

    let key = Zeroizing::new(derive_key(master_key, &salt)?);
    let cipher = Aes256Gcm::new_from_slice(key.as_ref())
        .map_err(|error| SecretError::Backend(format!("AES 初始化失败：{error}")))?;

    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce), plaintext.as_ref())
        .map_err(|_| SecretError::Backend("AES-GCM 加密失败".to_string()))?;

    let mut payload = Vec::with_capacity(1 + FILE_SALT_LEN + FILE_NONCE_LEN + ciphertext.len());
    payload.push(FILE_FORMAT_VERSION);
    payload.extend_from_slice(&salt);
    payload.extend_from_slice(&nonce);
    payload.extend_from_slice(&ciphertext);
    Ok(payload)
}

fn decrypt_payload(
    payload: &[u8],
    master_key: &str,
) -> SecretResult<HashMap<String, HashMap<String, String>>> {
    let min_len = 1 + FILE_SALT_LEN + FILE_NONCE_LEN + FILE_TAG_LEN;
    if payload.len() < min_len {
        return Err(SecretError::SecretMissing(
            "密钥文件损坏或主密码错误".to_string(),
        ));
    }

    let version = payload[0];
    if version != FILE_FORMAT_VERSION {
        return Err(SecretError::Backend(format!("不支持的密文版本：{version}")));
    }

    let salt_start = 1;
    let nonce_start = salt_start + FILE_SALT_LEN;
    let cipher_start = nonce_start + FILE_NONCE_LEN;

    let mut salt = [0_u8; FILE_SALT_LEN];
    salt.copy_from_slice(&payload[salt_start..nonce_start]);

    let mut nonce = [0_u8; FILE_NONCE_LEN];
    nonce.copy_from_slice(&payload[nonce_start..cipher_start]);

    let key = Zeroizing::new(derive_key(master_key, &salt)?);
    let cipher = Aes256Gcm::new_from_slice(key.as_ref())
        .map_err(|error| SecretError::Backend(format!("AES 初始化失败：{error}")))?;

    let plaintext = cipher
        .decrypt(Nonce::from_slice(&nonce), &payload[cipher_start..])
        .map_err(|_| SecretError::SecretMissing("密钥文件解密失败（主密码错误）".to_string()))?;

    let data = serde_json::from_slice(&plaintext)?;
    Ok(data)
}

fn derive_key(master_key: &str, salt: &[u8; FILE_SALT_LEN]) -> SecretResult<[u8; 32]> {
    let params = Params::new(
        ARGON2_MEMORY_KIB,
        ARGON2_ITERATIONS,
        ARGON2_PARALLELISM,
        Some(32),
    )
    .map_err(|error| SecretError::Backend(format!("Argon2 参数无效：{error}")))?;

    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);

    let mut key = [0_u8; 32];
    argon2
        .hash_password_into(master_key.as_bytes(), salt, &mut key)
        .map_err(|error| SecretError::Backend(format!("Argon2 密钥派生失败：{error}")))?;
    Ok(key)
}

fn write_atomic(path: &Path, payload: &[u8]) -> std::io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|item| item.to_str())
        .unwrap_or("secrets.enc");

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temp_name = format!(".{file_name}.{nanos}.tmp");
    let temp_path = parent.join(temp_name);

    fs::write(&temp_path, payload)?;
    fs::rename(&temp_path, path)?;
    Ok(())
}

#[cfg(unix)]
fn ensure_file_permission(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn ensure_file_permission(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    use zeroize::Zeroize;

    use super::{
        EnvSecretStore, FileSecretStore, KeyringSecretStore, MemorySecretStore, SecretError,
        SecretResult, SecretStore, env_key, redact_secret,
    };

    static ENV_TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn memory_store_supports_crud_and_namespace_isolation() -> SecretResult<()> {
        let store = MemorySecretStore::new();

        store.set("plugin:plugin_a", "password", "alpha")?;
        store.set("plugin:plugin_b", "password", "bravo")?;

        let value_a = store.get("plugin:plugin_a", "password")?;
        assert_eq!(value_a.as_str(), "alpha");

        // 编译期保证返回类型是 Zeroizing<String>。
        fn assert_zeroizing_type(_: zeroize::Zeroizing<String>) {}
        assert_zeroizing_type(value_a);

        let value_b = store.get("plugin:plugin_b", "password")?;
        assert_eq!(value_b.as_str(), "bravo");

        let keys = store.list_keys("plugin:plugin_a")?;
        assert_eq!(keys, vec!["password".to_string()]);

        store.delete("plugin:plugin_a", "password")?;
        let missing = store.get("plugin:plugin_a", "password").unwrap_err();
        assert_eq!(missing.code(), "E_SECRET_MISSING");

        let still_exists = store.get("plugin:plugin_b", "password")?;
        assert_eq!(still_exists.as_str(), "bravo");

        Ok(())
    }

    #[test]
    fn env_store_reads_from_environment() -> SecretResult<()> {
        let _guard = ENV_TEST_LOCK.lock().expect("ENV 测试锁失败");

        let scope = "plugin:env_demo";
        let key = "api_token";
        let var_name = env_key(scope, key);

        // Rust 2024 中修改进程环境变量是 unsafe，测试中通过互斥锁串行化规避竞态。
        unsafe {
            std::env::set_var(&var_name, "from-env");
        }

        let store = EnvSecretStore::new();
        let value = store.get(scope, key)?;
        assert_eq!(value.as_str(), "from-env");

        unsafe {
            std::env::remove_var(&var_name);
        }

        Ok(())
    }

    #[test]
    fn env_store_set_delete_overlay() -> SecretResult<()> {
        let store = EnvSecretStore::new();

        store.set("system", "admin_token", "token-1")?;
        let value = store.get("system", "admin_token")?;
        assert_eq!(value.as_str(), "token-1");

        let keys = store.list_keys("system")?;
        assert!(keys.contains(&"admin_token".to_string()));

        store.delete("system", "admin_token")?;
        let error = store.get("system", "admin_token").unwrap_err();
        assert_eq!(error.code(), "E_SECRET_MISSING");

        Ok(())
    }

    #[test]
    fn file_store_encrypts_and_handles_wrong_password() -> SecretResult<()> {
        let file_path = unique_secret_file_path();
        let scope = "plugin:file_demo";
        let key = "password";

        let store = FileSecretStore::new(&file_path, "correct-passphrase")?;
        store.set(scope, key, "secret-value")?;

        let roundtrip = store.get(scope, key)?;
        assert_eq!(roundtrip.as_str(), "secret-value");

        let keys = store.list_keys(scope)?;
        assert_eq!(keys, vec![key.to_string()]);

        drop(store);

        let reopened = FileSecretStore::new(&file_path, "correct-passphrase")?;
        let persisted = reopened.get(scope, key)?;
        assert_eq!(persisted.as_str(), "secret-value");

        let wrong_password_store = FileSecretStore::new(&file_path, "wrong-passphrase")?;
        let error = wrong_password_store.get(scope, key).unwrap_err();
        assert!(matches!(error, SecretError::SecretMissing(_)));
        assert_eq!(error.code(), "E_SECRET_MISSING");

        cleanup_secret_file(&file_path);
        Ok(())
    }

    #[test]
    fn keyring_store_roundtrip_when_backend_available() -> SecretResult<()> {
        let store = KeyringSecretStore::new();
        let unique = unique_scope_suffix();
        let scope = format!("plugin:keyring_test_{unique}");
        let key = "password";

        if let Err(error) = store.set(&scope, key, "from-keyring") {
            eprintln!("跳过 keyring 可用性测试：{error}");
            return Ok(());
        }

        let value = match store.get(&scope, key) {
            Ok(value) => value,
            Err(SecretError::SecretMissing(_)) => {
                eprintln!("跳过 keyring 可用性测试：当前环境无法回读写入项");
                let _ = store.delete(&scope, key);
                return Ok(());
            }
            Err(error) => return Err(error),
        };
        assert_eq!(value.as_str(), "from-keyring");

        let keys = store.list_keys(&scope)?;
        assert!(keys.contains(&key.to_string()));

        store.delete(&scope, key)?;
        let error = store.get(&scope, key).unwrap_err();
        assert_eq!(error.code(), "E_SECRET_MISSING");

        Ok(())
    }

    #[test]
    fn redacted_secret_never_prints_plaintext() {
        let masked = redact_secret("plain-text-secret");
        assert_eq!(format!("{masked:?}"), "***REDACTED***");
        assert_eq!(format!("{masked}"), "***REDACTED***");
    }

    #[test]
    fn zeroizing_clears_wrapped_value_on_drop() {
        #[derive(Default)]
        struct Probe {
            flag: std::sync::Arc<AtomicBool>,
            bytes: [u8; 16],
        }

        impl Zeroize for Probe {
            fn zeroize(&mut self) {
                self.bytes.zeroize();
                self.flag.store(true, Ordering::SeqCst);
            }
        }

        let flag = std::sync::Arc::new(AtomicBool::new(false));
        let probe = Probe {
            flag: std::sync::Arc::clone(&flag),
            bytes: [42_u8; 16],
        };

        {
            let _wrapped = zeroize::Zeroizing::new(probe);
        }

        assert!(flag.load(Ordering::SeqCst));
    }

    fn unique_scope_suffix() -> String {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("系统时间异常")
            .as_nanos()
            .to_string()
    }

    fn unique_secret_file_path() -> std::path::PathBuf {
        let nanos = unique_scope_suffix();
        std::env::temp_dir().join(format!("subforge-secrets-{nanos}.enc"))
    }

    fn cleanup_secret_file(path: &std::path::Path) {
        let _ = std::fs::remove_file(path);
    }
}
