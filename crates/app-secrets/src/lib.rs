//! app-secrets：敏感信息存取抽象与后端实现。

mod constants;
mod crypto;
mod error;
mod io_utils;
mod keys;
mod redaction;
mod stores;
mod traits;

#[cfg(test)]
mod tests;

pub use error::{SecretError, SecretResult};
pub use redaction::{RedactedSecret, redact_secret};
pub use stores::{EnvSecretStore, FileSecretStore, KeyringSecretStore, MemorySecretStore};
pub use traits::SecretStore;

#[cfg(test)]
pub(crate) use keys::env_key;
