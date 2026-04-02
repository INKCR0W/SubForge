use std::collections::BTreeSet;
use std::fmt;

use keyring::Entry;
use zeroize::Zeroizing;

use crate::constants::{KEYRING_INDEX_KEY, KEYRING_SERVICE};
use crate::keys::{storage_key, validate_key, validate_scope};
use crate::{SecretError, SecretResult, SecretStore};
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
