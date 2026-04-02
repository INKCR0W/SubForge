use std::collections::HashMap;
use std::fmt;
use std::sync::Mutex;

use zeroize::Zeroizing;

use crate::constants::REDACTED;
use crate::io_utils::lock_mutex;
use crate::keys::{storage_key, validate_key, validate_scope};
use crate::{SecretError, SecretResult, SecretStore};
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
