use super::*;

pub(crate) async fn get_system_settings_handler(
    State(state): State<ServerContext>,
) -> ApiResult<SettingsResponse> {
    let repository = SettingsRepository::new(state.database.as_ref());
    let settings = repository.get_all().map_err(storage_error_to_response)?;
    Ok((
        StatusCode::OK,
        Json(SettingsResponse {
            settings: map_settings(settings),
        }),
    ))
}

pub(crate) async fn update_system_settings_handler(
    State(state): State<ServerContext>,
    Json(payload): Json<UpdateSettingsRequest>,
) -> ApiResult<SettingsResponse> {
    if payload.settings.is_empty() {
        return Err(config_error_response("请求体 settings 不能为空"));
    }
    let updated_at = current_timestamp_rfc3339().map_err(|_| internal_error_response())?;
    let repository = SettingsRepository::new(state.database.as_ref());
    for (key, value) in payload.settings {
        if key.trim().is_empty() {
            return Err(config_error_response("设置键不能为空"));
        }
        repository
            .set(&AppSetting {
                key,
                value,
                updated_at: updated_at.clone(),
            })
            .map_err(storage_error_to_response)?;
    }

    let settings = repository.get_all().map_err(storage_error_to_response)?;
    Ok((
        StatusCode::OK,
        Json(SettingsResponse {
            settings: map_settings(settings),
        }),
    ))
}
