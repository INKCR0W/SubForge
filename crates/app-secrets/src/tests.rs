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
