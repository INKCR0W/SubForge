use std::collections::BTreeMap;

use app_common::SourceInstance;
use app_plugin_runtime::LoadedPlugin;
use app_storage::{SourceConfigRepository, SourceRepository, Transaction, params};
use serde_json::Value;
use time::OffsetDateTime;

use crate::source_service::{PreparedConfig, SourceService};
use crate::utils::{masked_config, now_rfc3339, source_scope};
use crate::{CoreError, CoreResult, SourceWithConfig};

impl<'a> SourceService<'a> {
    pub fn create_source(
        &self,
        plugin_id: &str,
        name: &str,
        config: BTreeMap<String, Value>,
    ) -> CoreResult<SourceWithConfig> {
        if name.trim().is_empty() {
            return Err(CoreError::ConfigInvalid("name 不能为空".to_string()));
        }

        let loaded = self.load_installed_plugin(plugin_id)?;
        let prepared = self.validate_and_split_config(&loaded, &config)?;
        let now = now_rfc3339()?;
        let source = SourceInstance {
            id: format!(
                "source-{}-{}",
                plugin_id.replace('.', "-"),
                OffsetDateTime::now_utc().unix_timestamp_nanos()
            ),
            plugin_id: plugin_id.to_string(),
            name: name.to_string(),
            status: "healthy".to_string(),
            state_json: None,
            created_at: now.clone(),
            updated_at: now,
        };

        let source_repository = SourceRepository::new(self.db);
        let config_repository = SourceConfigRepository::new(self.db);
        source_repository.insert(&source)?;

        if let Err(error) =
            self.persist_source_config(&source, &loaded, &prepared, &config_repository, false)
        {
            let _ = source_repository.delete(&source.id);
            for key in prepared.secret.keys() {
                let _ = self
                    .secret_store
                    .delete(&source_scope(&source.id), key.as_str());
            }
            return Err(error);
        }

        Ok(SourceWithConfig {
            source,
            config: masked_config(&loaded, &prepared.normalized),
        })
    }

    pub fn get_source(&self, source_id: &str) -> CoreResult<Option<SourceWithConfig>> {
        let source_repository = SourceRepository::new(self.db);
        let source = match source_repository.get_by_id(source_id)? {
            Some(source) => source,
            None => return Ok(None),
        };
        let loaded = self.load_installed_plugin(&source.plugin_id)?;
        let config_repository = SourceConfigRepository::new(self.db);
        let stored = config_repository.get_all(&source.id)?;
        let masked = self.inflate_source_config(&source, &loaded, &stored, true)?;

        Ok(Some(SourceWithConfig {
            source,
            config: masked,
        }))
    }

    pub fn list_sources(&self) -> CoreResult<Vec<SourceWithConfig>> {
        let source_repository = SourceRepository::new(self.db);
        let sources = source_repository.list()?;
        let config_repository = SourceConfigRepository::new(self.db);
        let mut result = Vec::with_capacity(sources.len());

        for source in sources {
            let loaded = self.load_installed_plugin(&source.plugin_id)?;
            let stored = config_repository.get_all(&source.id)?;
            let masked = self.inflate_source_config(&source, &loaded, &stored, true)?;
            result.push(SourceWithConfig {
                source,
                config: masked,
            });
        }

        Ok(result)
    }

    pub(crate) fn get_source_for_runtime(
        &self,
        source_id: &str,
    ) -> CoreResult<Option<SourceWithConfig>> {
        let source_repository = SourceRepository::new(self.db);
        let source = match source_repository.get_by_id(source_id)? {
            Some(source) => source,
            None => return Ok(None),
        };
        let loaded = self.load_installed_plugin(&source.plugin_id)?;
        let config_repository = SourceConfigRepository::new(self.db);
        let stored = config_repository.get_all(&source.id)?;
        let config = self.inflate_source_config(&source, &loaded, &stored, false)?;

        Ok(Some(SourceWithConfig { source, config }))
    }

    pub fn update_source_config(
        &self,
        source_id: &str,
        config: BTreeMap<String, Value>,
    ) -> CoreResult<SourceWithConfig> {
        self.update_source(source_id, None, Some(config))
    }

    pub fn update_source(
        &self,
        source_id: &str,
        name: Option<&str>,
        config: Option<BTreeMap<String, Value>>,
    ) -> CoreResult<SourceWithConfig> {
        let has_config_update = config.is_some();
        let source_repository = SourceRepository::new(self.db);
        let source = source_repository
            .get_by_id(source_id)?
            .ok_or_else(|| CoreError::SourceNotFound(source_id.to_string()))?;
        let loaded = self.load_installed_plugin(&source.plugin_id)?;
        let config_repository = SourceConfigRepository::new(self.db);
        let stored_config = config_repository.get_all(&source.id)?;
        let scope = source_scope(&source.id);
        let previous_source_secret =
            self.snapshot_secret_values(&scope, &loaded.manifest.secret_fields)?;
        let previous_effective_secret =
            self.snapshot_source_secret_values(&source, &loaded.manifest.secret_fields)?;

        let (prepared, response_config) = match config {
            Some(config) => {
                let effective_config =
                    self.apply_secret_placeholders(&loaded, config, &previous_effective_secret)?;
                let prepared = self.validate_and_split_config(&loaded, &effective_config)?;
                let response_config = masked_config(&loaded, &prepared.normalized);
                (Some(prepared), response_config)
            }
            None => {
                let response_config =
                    self.inflate_source_config(&source, &loaded, &stored_config, true)?;
                (None, response_config)
            }
        };

        let mut updated_source = source.clone();
        if let Some(name) = name {
            let name = name.trim();
            if name.is_empty() {
                return Err(CoreError::ConfigInvalid("name 不能为空".to_string()));
            }
            updated_source.name = name.to_string();
        }
        updated_source.updated_at = now_rfc3339()?;

        if let Err(error) = self.db.with_transaction(|tx| {
            if let Some(prepared) = prepared.as_ref() {
                replace_source_config_in_transaction(tx, &source.id, &prepared.non_secret)?;
            }
            update_source_in_transaction(tx, &updated_source)?;
            Ok(())
        }) {
            return Err(error.into());
        }

        if let Some(prepared) = prepared.as_ref() {
            if let Err(error) =
                self.persist_source_secrets(&source, &loaded, prepared, has_config_update)
            {
                let _ = self.db.with_transaction(|tx| {
                    replace_source_config_in_transaction(tx, &source.id, &stored_config)?;
                    update_source_in_transaction(tx, &source)?;
                    Ok(())
                });
                let _ = self.restore_secret_values(
                    &scope,
                    &loaded.manifest.secret_fields,
                    &previous_source_secret,
                );
                return Err(error);
            }
        }

        Ok(SourceWithConfig {
            source: updated_source,
            config: response_config,
        })
    }

    pub fn delete_source(&self, source_id: &str) -> CoreResult<()> {
        let source_repository = SourceRepository::new(self.db);
        let source = source_repository
            .get_by_id(source_id)?
            .ok_or_else(|| CoreError::SourceNotFound(source_id.to_string()))?;
        let loaded = self.load_installed_plugin(&source.plugin_id)?;
        let scope = source_scope(&source.id);
        let previous_secret =
            self.snapshot_secret_values(&scope, &loaded.manifest.secret_fields)?;

        for secret_key in &loaded.manifest.secret_fields {
            if let Err(error) = self.secret_store.delete(&scope, secret_key) {
                let _ = self.restore_secret_values(
                    &scope,
                    &loaded.manifest.secret_fields,
                    &previous_secret,
                );
                return Err(error.into());
            }
        }

        if let Err(error) = source_repository.delete(source_id) {
            let _ = self.restore_secret_values(
                &scope,
                &loaded.manifest.secret_fields,
                &previous_secret,
            );
            return Err(error.into());
        }
        Ok(())
    }

    fn persist_source_config(
        &self,
        source: &SourceInstance,
        loaded: &LoadedPlugin,
        prepared: &PreparedConfig,
        config_repository: &SourceConfigRepository<'_>,
        prune_secret: bool,
    ) -> CoreResult<()> {
        config_repository.replace_all(&source.id, &prepared.non_secret)?;

        let scope = source_scope(&source.id);
        for (key, value) in &prepared.secret {
            self.secret_store.set(&scope, key, value)?;
        }
        if prune_secret {
            for key in &loaded.manifest.secret_fields {
                if !prepared.secret.contains_key(key) {
                    self.secret_store.delete(&scope, key)?;
                }
            }
        }
        Ok(())
    }

    fn persist_source_secrets(
        &self,
        source: &SourceInstance,
        loaded: &LoadedPlugin,
        prepared: &PreparedConfig,
        prune_secret: bool,
    ) -> CoreResult<()> {
        let scope = source_scope(&source.id);
        for (key, value) in &prepared.secret {
            self.secret_store.set(&scope, key, value)?;
        }
        if prune_secret {
            for key in &loaded.manifest.secret_fields {
                if !prepared.secret.contains_key(key) {
                    self.secret_store.delete(&scope, key)?;
                }
            }
        }
        Ok(())
    }
}

fn replace_source_config_in_transaction(
    tx: &Transaction<'_>,
    source_instance_id: &str,
    values: &BTreeMap<String, String>,
) -> app_storage::StorageResult<()> {
    tx.execute(
        "DELETE FROM source_instance_config WHERE source_instance_id = ?1",
        [source_instance_id],
    )?;
    for (key, value) in values {
        tx.execute(
            "INSERT INTO source_instance_config (id, source_instance_id, key, value)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                format!("{source_instance_id}:{key}"),
                source_instance_id,
                key,
                value
            ],
        )?;
    }
    Ok(())
}

fn update_source_in_transaction(
    tx: &Transaction<'_>,
    source: &SourceInstance,
) -> app_storage::StorageResult<usize> {
    let affected = tx.execute(
        "UPDATE source_instances
         SET plugin_id = ?1, name = ?2, status = ?3, state_json = ?4, updated_at = ?5
         WHERE id = ?6",
        params![
            source.plugin_id,
            source.name,
            source.status,
            source.state_json,
            source.updated_at,
            source.id
        ],
    )?;
    Ok(affected)
}
