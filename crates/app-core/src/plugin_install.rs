use std::fs;
use std::path::{Path, PathBuf};

use app_common::Plugin;
use app_plugin_runtime::PluginLoader;
use app_storage::{Database, PluginRepository};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::utils::copy_dir_recursive;
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
        let target_dir = self.plugins_dir.join(&loaded.manifest.plugin_id);
        if let Some(existing) = existing_plugin {
            if existing.version == loaded.manifest.version {
                return Err(CoreError::PluginAlreadyInstalled(
                    loaded.manifest.plugin_id.clone(),
                ));
            }

            if target_dir.exists() {
                fs::remove_dir_all(&target_dir)?;
            }
            repository.delete(&existing.id)?;
        }

        if target_dir.exists() {
            return Err(CoreError::PluginAlreadyInstalled(
                loaded.manifest.plugin_id.clone(),
            ));
        }
        copy_dir_recursive(&loaded.root_dir, &target_dir)?;

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

        if let Err(error) = repository.insert(&plugin) {
            let _ = fs::remove_dir_all(&target_dir);
            return Err(error.into());
        }

        Ok(plugin)
    }
}
