use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use zeroize::Zeroizing;

use crate::constants::REDACTED;
use crate::crypto::{decrypt_payload, encrypt_payload};
use crate::io_utils::{ensure_file_permission, lock_mutex, write_atomic};
use crate::keys::{storage_key, validate_key, validate_scope};
use crate::{SecretError, SecretResult, SecretStore};
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
