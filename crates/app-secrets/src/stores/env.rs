use std::collections::{BTreeSet, HashMap};
use std::fmt;
use std::sync::Mutex;

use zeroize::Zeroizing;

use crate::constants::REDACTED;
use crate::io_utils::lock_mutex;
use crate::keys::{env_key, normalize_for_env, storage_key, validate_key, validate_scope};
use crate::{SecretError, SecretResult, SecretStore};
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
