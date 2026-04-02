use std::fmt;

use zeroize::Zeroizing;

use crate::SecretResult;
/// 密钥存储统一接口。返回值使用 `Zeroizing<String>` 保证 drop 时清零。
pub trait SecretStore: Send + Sync + fmt::Debug {
    fn set(&self, scope: &str, key: &str, value: &str) -> SecretResult<()>;
    fn get(&self, scope: &str, key: &str) -> SecretResult<Zeroizing<String>>;
    fn delete(&self, scope: &str, key: &str) -> SecretResult<()>;
    fn list_keys(&self, scope: &str) -> SecretResult<Vec<String>>;
}
