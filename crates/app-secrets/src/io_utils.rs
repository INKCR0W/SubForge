use std::fs;
use std::path::Path;
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{SecretError, SecretResult};

pub(crate) fn lock_mutex<'a, T>(
    mutex: &'a Mutex<T>,
    name: &str,
) -> SecretResult<MutexGuard<'a, T>> {
    mutex
        .lock()
        .map_err(|_| SecretError::Backend(format!("互斥锁已中毒：{name}")))
}

pub(crate) fn write_atomic(path: &Path, payload: &[u8]) -> std::io::Result<()> {
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
pub(crate) fn ensure_file_permission(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
pub(crate) fn ensure_file_permission(_path: &Path) -> std::io::Result<()> {
    Ok(())
}
