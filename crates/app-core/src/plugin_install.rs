use std::fs;
use std::path::{Path, PathBuf};

use app_common::Plugin;
use app_plugin_runtime::PluginLoader;
use app_storage::{Database, PluginRepository};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::utils::{atomic_install_dir_paths, copy_dir_recursive};
use crate::{CoreError, CoreResult};

#[derive(Debug)]
pub struct PluginInstallService<'a> {
    db: &'a Database,
    loader: PluginLoader,
    plugins_dir: PathBuf,
}

impl<'a> PluginInstallService<'a> {
    pub fn new(db: &'a Database, plugins_dir: impl Into<PathBuf>) -> Self {
        Self {
            db,
            loader: PluginLoader::new(),
            plugins_dir: plugins_dir.into(),
        }
    }

    pub fn install_from_dir(&self, source_dir: impl AsRef<Path>) -> CoreResult<Plugin> {
        let loaded = self.loader.load_from_dir(source_dir)?;
        let repository = PluginRepository::new(self.db);
        let existing_plugin = repository.get_by_plugin_id(&loaded.manifest.plugin_id)?;

        fs::create_dir_all(&self.plugins_dir)?;
        let nonce = OffsetDateTime::now_utc().unix_timestamp_nanos().to_string();
        let (target_dir, temp_dir, backup_dir) =
            atomic_install_dir_paths(&self.plugins_dir, &loaded.manifest.plugin_id, &nonce);
        if let Some(existing) = &existing_plugin {
            if existing.version == loaded.manifest.version {
                return Err(CoreError::PluginAlreadyInstalled(
                    loaded.manifest.plugin_id.clone(),
                ));
            }
        } else if target_dir.exists() {
            return Err(CoreError::PluginAlreadyInstalled(
                loaded.manifest.plugin_id.clone(),
            ));
        }

        cleanup_staging_dir(&temp_dir)?;
        cleanup_staging_dir(&backup_dir)?;
        copy_dir_recursive(&loaded.root_dir, &temp_dir).inspect_err(|_| {
            let _ = fs::remove_dir_all(&temp_dir);
        })?;
        self.loader.load_from_dir(&temp_dir).inspect_err(|_| {
            let _ = fs::remove_dir_all(&temp_dir);
        })?;

        let now = OffsetDateTime::now_utc().format(&Rfc3339)?;
        let plugin = Plugin {
            id: format!(
                "{}-{}",
                loaded.manifest.plugin_id,
                OffsetDateTime::now_utc().unix_timestamp_nanos()
            ),
            plugin_id: loaded.manifest.plugin_id,
            name: loaded.manifest.name,
            version: loaded.manifest.version,
            spec_version: loaded.manifest.spec_version,
            plugin_type: loaded.manifest.plugin_type.as_str().to_string(),
            status: "installed".to_string(),
            installed_at: now.clone(),
            updated_at: now,
        };

        if let Some(existing) = &existing_plugin {
            atomic_upgrade_plugin_dir(
                &target_dir,
                &temp_dir,
                &backup_dir,
                || {
                    repository.replace_by_plugin_id(&plugin)?;
                    Ok(())
                },
                || {
                    repository.replace_by_plugin_id(existing)?;
                    Ok(())
                },
            )
        } else {
            atomic_install_new_plugin_dir(&target_dir, &temp_dir, || {
                repository.insert(&plugin)?;
                Ok(())
            })
        }?;

        Ok(plugin)
    }
}

fn cleanup_staging_dir(path: &Path) -> CoreResult<()> {
    if path.exists() {
        fs::remove_dir_all(path)?;
    }
    Ok(())
}

fn atomic_install_new_plugin_dir(
    target_dir: &Path,
    temp_dir: &Path,
    update_database: impl FnOnce() -> CoreResult<()>,
) -> CoreResult<()> {
    if target_dir.exists() {
        let _ = fs::remove_dir_all(temp_dir);
        return Err(CoreError::PluginAlreadyInstalled(
            target_dir
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .to_string(),
        ));
    }

    fs::rename(temp_dir, target_dir)?;
    if let Err(error) = update_database() {
        let _ = fs::remove_dir_all(target_dir);
        return Err(error);
    }
    Ok(())
}

fn atomic_upgrade_plugin_dir(
    target_dir: &Path,
    temp_dir: &Path,
    backup_dir: &Path,
    update_database: impl FnOnce() -> CoreResult<()>,
    rollback_database: impl FnOnce() -> CoreResult<()>,
) -> CoreResult<()> {
    if let Err(error) = fs::rename(target_dir, backup_dir) {
        let _ = fs::remove_dir_all(temp_dir);
        return Err(error.into());
    }

    if let Err(error) = fs::rename(temp_dir, target_dir) {
        let _ = fs::remove_dir_all(temp_dir);
        let restore_result = fs::rename(backup_dir, target_dir);
        if let Err(restore_error) = restore_result {
            return Err(CoreError::Io(std::io::Error::new(
                restore_error.kind(),
                format!(
                    "插件目录升级失败且旧目录恢复失败；升级错误：{error}；恢复错误：{restore_error}"
                ),
            )));
        }
        return Err(error.into());
    }

    if let Err(error) = update_database() {
        let _ = fs::remove_dir_all(target_dir);
        restore_backup_dir(target_dir, backup_dir)?;
        return Err(error);
    }

    if let Err(error) = fs::remove_dir_all(backup_dir) {
        let rollback_database_result = rollback_database();
        let _ = fs::remove_dir_all(target_dir);
        let restore_result = restore_backup_dir(target_dir, backup_dir);
        rollback_database_result?;
        restore_result?;
        return Err(error.into());
    }

    Ok(())
}

fn restore_backup_dir(target_dir: &Path, backup_dir: &Path) -> CoreResult<()> {
    if target_dir.exists() {
        fs::remove_dir_all(target_dir)?;
    }
    if backup_dir.exists() {
        fs::rename(backup_dir, target_dir)?;
    }
    Ok(())
}
