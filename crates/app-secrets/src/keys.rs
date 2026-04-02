use crate::{SecretError, SecretResult};

pub(crate) fn validate_scope(scope: &str) -> SecretResult<()> {
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

pub(crate) fn validate_key(key: &str) -> SecretResult<()> {
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

pub(crate) fn storage_key(scope: &str, key: &str) -> String {
    format!("subforge:{scope}:{key}")
}

pub(crate) fn env_key(scope: &str, key: &str) -> String {
    format!(
        "SUBFORGE_{}_{}",
        normalize_for_env(scope),
        normalize_for_env(key)
    )
}

pub(crate) fn normalize_for_env(input: &str) -> String {
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
