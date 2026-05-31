use app_common::{ClashRoutingTemplate, ErrorResponse};
use axum::Json;
use axum::http::StatusCode;

use super::{config_error_response, validate_source_ids_exist};

pub(super) fn profile_routing_template_source_key(profile_id: &str) -> String {
    format!("profile.{profile_id}.clash_template_source_id")
}

fn source_routing_template_key(source_id: &str) -> String {
    format!("source.{source_id}.clash_routing_template")
}

pub(super) fn normalize_routing_template_source_id(
    value: Option<&str>,
) -> Result<Option<String>, (StatusCode, Json<ErrorResponse>)> {
    let Some(value) = value else {
        return Ok(None);
    };
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    Ok(Some(trimmed.to_string()))
}

pub(super) fn ensure_routing_template_source_in_scope(
    database: &app_storage::Database,
    source_ids: &[String],
    routing_template_source_id: Option<&str>,
) -> Result<(), (StatusCode, Json<ErrorResponse>)> {
    let Some(template_source_id) = routing_template_source_id else {
        return Ok(());
    };
    validate_source_ids_exist(database, &[template_source_id.to_string()])?;
    if !source_ids
        .iter()
        .any(|source_id| source_id == template_source_id)
    {
        return Err(config_error_response(
            "routing_template_source_id 必须包含在 profile.source_ids 中",
        ));
    }
    Ok(())
}

pub(super) fn resolve_profile_routing_template_source(
    database: &app_storage::Database,
    profile_id: &str,
) -> app_storage::StorageResult<Option<String>> {
    let repository = app_storage::SettingsRepository::new(database);
    let key = profile_routing_template_source_key(profile_id);
    let setting = repository.get(&key)?;
    Ok(setting
        .map(|item| item.value.trim().to_string())
        .filter(|value| !value.is_empty()))
}

pub(super) fn load_clash_routing_template_for_profile(
    database: &app_storage::Database,
    routing_template_source_id: Option<&str>,
) -> app_storage::StorageResult<Option<ClashRoutingTemplate>> {
    let Some(source_id) = routing_template_source_id else {
        return Ok(None);
    };
    let repository = app_storage::SettingsRepository::new(database);
    let key = source_routing_template_key(source_id);
    let Some(setting) = repository.get(&key)? else {
        return Ok(None);
    };
    match serde_json::from_str::<ClashRoutingTemplate>(&setting.value) {
        Ok(parsed) => Ok(Some(parsed)),
        Err(error) => {
            eprintln!(
                "WARN: 反序列化 ClashRoutingTemplate 失败，模板将被忽略 source_id={source_id} error={error}"
            );
            Ok(None)
        }
    }
}
