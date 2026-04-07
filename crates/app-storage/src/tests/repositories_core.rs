use std::collections::BTreeMap;

use app_common::{AppSetting, ProfileSource};

use crate::{
    Database, PluginRepository, ProfileRepository, SettingsRepository, SourceConfigRepository,
    SourceRepository, StorageResult,
};

use super::support::{list_profile_sources, sample_plugin, sample_profile, sample_source};

#[test]
fn plugin_repository_crud_workflow() -> StorageResult<()> {
    let db = Database::open_in_memory()?;
    let repository = PluginRepository::new(&db);
    let plugin = sample_plugin("plugin-row-1", "vendor.example.static");

    repository.insert(&plugin)?;

    let by_id = repository.get_by_id(&plugin.id)?;
    assert_eq!(by_id, Some(plugin.clone()));

    let by_plugin_id = repository.get_by_plugin_id(&plugin.plugin_id)?;
    assert_eq!(by_plugin_id, Some(plugin.clone()));

    let list = repository.list()?;
    assert_eq!(list, vec![plugin.clone()]);

    let updated_at = "2026-04-02T02:00:00Z";
    assert_eq!(
        repository.update_status(&plugin.id, "disabled", updated_at)?,
        1
    );

    let updated = repository.get_by_id(&plugin.id)?.expect("插件应存在");
    assert_eq!(updated.status, "disabled");
    assert_eq!(updated.updated_at, updated_at);

    assert_eq!(repository.delete(&plugin.id)?, 1);
    assert!(repository.get_by_id(&plugin.id)?.is_none());

    Ok(())
}

#[test]
fn source_repository_crud_workflow() -> StorageResult<()> {
    let db = Database::open_in_memory()?;
    let repository = SourceRepository::new(&db);
    let source_a = sample_source("source-a", "vendor.example.static");
    let source_b = sample_source("source-b", "vendor.example.script");

    repository.insert(&source_a)?;
    repository.insert(&source_b)?;

    let by_id = repository.get_by_id(&source_a.id)?;
    assert_eq!(by_id, Some(source_a.clone()));

    let list = repository.list()?;
    assert_eq!(list.len(), 2);

    let list_by_plugin = repository.list_by_plugin(&source_a.plugin_id)?;
    assert_eq!(list_by_plugin, vec![source_a.clone()]);

    let mut updated_source = source_a.clone();
    updated_source.name = "Source A Updated".to_string();
    updated_source.status = "error".to_string();
    updated_source.state_json = Some("{\"last_error\":\"timeout\"}".to_string());
    updated_source.updated_at = "2026-04-02T02:30:00Z".to_string();
    assert_eq!(repository.update(&updated_source)?, 1);

    let loaded = repository
        .get_by_id(&updated_source.id)?
        .expect("来源应存在");
    assert_eq!(loaded, updated_source);

    assert_eq!(repository.delete(&updated_source.id)?, 1);
    assert!(repository.get_by_id(&updated_source.id)?.is_none());

    Ok(())
}

#[test]
fn source_config_repository_replace_and_delete_workflow() -> StorageResult<()> {
    let db = Database::open_in_memory()?;
    let source_repository = SourceRepository::new(&db);
    let config_repository = SourceConfigRepository::new(&db);
    let source = sample_source("source-config-1", "vendor.example.static");
    source_repository.insert(&source)?;

    let mut first = BTreeMap::new();
    first.insert("url".to_string(), "https://example.com/sub".to_string());
    first.insert("user_agent".to_string(), "SubForge/0.1".to_string());
    config_repository.replace_all(&source.id, &first)?;
    assert_eq!(config_repository.get_all(&source.id)?, first);

    let mut second = BTreeMap::new();
    second.insert("url".to_string(), "https://example.com/next".to_string());
    config_repository.replace_all(&source.id, &second)?;
    assert_eq!(config_repository.get_all(&source.id)?, second);

    assert_eq!(config_repository.delete_all(&source.id)?, 1);
    assert!(config_repository.get_all(&source.id)?.is_empty());

    Ok(())
}

#[test]
fn profile_repository_crud_and_binding_workflow() -> StorageResult<()> {
    let db = Database::open_in_memory()?;
    let profile_repository = ProfileRepository::new(&db);
    let source_repository = SourceRepository::new(&db);
    let profile = sample_profile("profile-default");
    let source = sample_source("source-for-profile", "vendor.example.static");

    source_repository.insert(&source)?;
    profile_repository.insert(&profile)?;

    let by_id = profile_repository.get_by_id(&profile.id)?;
    assert_eq!(by_id, Some(profile.clone()));

    let list = profile_repository.list()?;
    assert_eq!(list, vec![profile.clone()]);

    let mut updated_profile = profile.clone();
    updated_profile.name = "Profile Updated".to_string();
    updated_profile.description = Some("更新后的聚合配置".to_string());
    updated_profile.updated_at = "2026-04-02T03:00:00Z".to_string();
    assert_eq!(profile_repository.update(&updated_profile)?, 1);

    let loaded = profile_repository
        .get_by_id(&updated_profile.id)?
        .expect("配置应存在");
    assert_eq!(loaded, updated_profile);

    profile_repository.add_source(&updated_profile.id, &source.id, 10)?;
    profile_repository.add_source(&updated_profile.id, &source.id, 20)?;

    let profile_sources = list_profile_sources(&db, &updated_profile.id)?;
    assert_eq!(
        profile_sources,
        vec![ProfileSource {
            profile_id: updated_profile.id.clone(),
            source_instance_id: source.id.clone(),
            priority: 20,
        }]
    );

    assert_eq!(
        profile_repository.remove_source(&updated_profile.id, &source.id)?,
        1
    );
    let profile_sources = list_profile_sources(&db, &updated_profile.id)?;
    assert!(profile_sources.is_empty());

    assert_eq!(profile_repository.delete(&updated_profile.id)?, 1);
    assert!(profile_repository.get_by_id(&updated_profile.id)?.is_none());

    Ok(())
}

#[test]
fn settings_repository_supports_upsert() -> StorageResult<()> {
    let db = Database::open_in_memory()?;
    let repository = SettingsRepository::new(&db);

    assert!(repository.get("ui.theme")?.is_none());

    let setting = AppSetting {
        key: "ui.theme".to_string(),
        value: "dark".to_string(),
        updated_at: "2026-04-02T03:30:00Z".to_string(),
    };
    repository.set(&setting)?;
    assert_eq!(repository.get("ui.theme")?, Some(setting.clone()));

    let updated_setting = AppSetting {
        key: "ui.theme".to_string(),
        value: "light".to_string(),
        updated_at: "2026-04-02T03:31:00Z".to_string(),
    };
    repository.set(&updated_setting)?;
    assert_eq!(repository.get("ui.theme")?, Some(updated_setting.clone()));

    let secondary_setting = AppSetting {
        key: "core.port".to_string(),
        value: "18118".to_string(),
        updated_at: "2026-04-02T03:32:00Z".to_string(),
    };
    repository.set(&secondary_setting)?;

    let all = repository.get_all()?;
    assert_eq!(all, vec![secondary_setting, updated_setting]);

    assert_eq!(repository.delete("ui.theme")?, 1);
    assert!(repository.get("ui.theme")?.is_none());

    Ok(())
}
