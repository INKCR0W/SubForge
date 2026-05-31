use super::*;

pub(crate) async fn create_profile_handler(
    State(state): State<ServerContext>,
    Json(payload): Json<CreateProfileRequest>,
) -> ApiResult<ProfileResponse> {
    let name = payload.name.trim();
    if name.is_empty() {
        return Err(config_error_response("profile.name 不能为空"));
    }
    validate_source_ids_exist(state.database.as_ref(), &payload.source_ids)?;
    let routing_template_source_id =
        normalize_routing_template_source_id(payload.routing_template_source_id.as_deref())?;
    ensure_routing_template_source_in_scope(
        state.database.as_ref(),
        &payload.source_ids,
        routing_template_source_id.as_deref(),
    )?;

    let now = current_timestamp_rfc3339().map_err(|_| internal_error_response())?;
    let export_token = generate_export_token().map_err(|_| internal_error_response())?;
    let profile = Profile {
        id: format!(
            "profile-{}",
            OffsetDateTime::now_utc().unix_timestamp_nanos()
        ),
        name: name.to_string(),
        description: payload.description.map(|value| value.trim().to_string()),
        routing_template_source_id: routing_template_source_id.clone(),
        created_at: now.clone(),
        updated_at: now,
    };
    state
        .database
        .with_transaction(|tx| {
            insert_profile_in_transaction(tx, &profile)?;
            replace_profile_sources_in_transaction(tx, &profile.id, &payload.source_ids)?;
            persist_profile_routing_template_source_in_transaction(
                tx,
                &profile.id,
                routing_template_source_id.as_deref(),
            )?;
            insert_profile_export_token_in_transaction(tx, &profile.id, &export_token)?;
            Ok(())
        })
        .map_err(storage_error_to_response)?;

    emit_event(
        &state,
        "profile:created",
        format!("Profile 创建成功：{}", profile.id),
        None,
    );
    Ok((
        StatusCode::CREATED,
        Json(ProfileResponse {
            profile: ProfileDto {
                profile,
                source_ids: payload.source_ids,
                export_token: Some(export_token),
            },
        }),
    ))
}

pub(crate) async fn update_profile_handler(
    State(state): State<ServerContext>,
    AxumPath(id): AxumPath<String>,
    Json(payload): Json<UpdateProfileRequest>,
) -> ApiResult<ProfileResponse> {
    let repository = ProfileRepository::new(state.database.as_ref());
    let UpdateProfileRequest {
        name,
        description,
        source_ids: requested_source_ids,
        routing_template_source_id: requested_routing_template_source_id,
    } = payload;
    let mut profile = repository
        .get_by_id(&id)
        .map_err(storage_error_to_response)?
        .ok_or_else(|| not_found_error_response("Profile 不存在"))?;
    profile.routing_template_source_id =
        resolve_profile_routing_template_source(state.database.as_ref(), &profile.id)
            .map_err(storage_error_to_response)?;
    let replace_sources = requested_source_ids.is_some();
    let source_ids = if let Some(source_ids) = requested_source_ids {
        validate_source_ids_exist(state.database.as_ref(), &source_ids)?;
        source_ids
    } else {
        list_profile_source_ids(state.database.as_ref(), &id).map_err(storage_error_to_response)?
    };
    let mut routing_template_source_id = profile.routing_template_source_id.clone();
    if let Some(value) = requested_routing_template_source_id {
        routing_template_source_id = normalize_routing_template_source_id(value.as_deref())?;
    }
    ensure_routing_template_source_in_scope(
        state.database.as_ref(),
        &source_ids,
        routing_template_source_id.as_deref(),
    )?;

    if let Some(name) = name {
        let name = name.trim();
        if name.is_empty() {
            return Err(config_error_response("profile.name 不能为空"));
        }
        profile.name = name.to_string();
    }
    if let Some(description) = description {
        profile.description = description.map(|value| value.trim().to_string());
    }
    profile.routing_template_source_id = routing_template_source_id.clone();
    profile.updated_at = current_timestamp_rfc3339().map_err(|_| internal_error_response())?;
    state
        .database
        .with_transaction(|tx| {
            update_profile_in_transaction(tx, &profile)?;
            if replace_sources {
                replace_profile_sources_in_transaction(tx, &id, &source_ids)?;
            }
            persist_profile_routing_template_source_in_transaction(
                tx,
                &id,
                routing_template_source_id.as_deref(),
            )?;
            Ok(())
        })
        .map_err(storage_error_to_response)?;

    state.profile_cache.invalidate(&id);
    let profile_dto = build_profile_dto(state.database.as_ref(), profile, source_ids)?;
    emit_event(
        &state,
        "profile:updated",
        format!("Profile 更新成功：{id}"),
        None,
    );
    Ok((
        StatusCode::OK,
        Json(ProfileResponse {
            profile: profile_dto,
        }),
    ))
}

pub(crate) async fn delete_profile_handler(
    State(state): State<ServerContext>,
    AxumPath(id): AxumPath<String>,
) -> ApiResult<Value> {
    let repository = ProfileRepository::new(state.database.as_ref());
    let affected = repository.delete(&id).map_err(storage_error_to_response)?;
    if affected == 0 {
        return Err(not_found_error_response("Profile 不存在"));
    }
    let settings_repository = SettingsRepository::new(state.database.as_ref());
    let _ = settings_repository.delete(&profile_routing_template_source_key(&id));
    state.profile_cache.invalidate(&id);
    emit_event(
        &state,
        "profile:deleted",
        format!("Profile 已删除：{id}"),
        None,
    );
    Ok((StatusCode::OK, Json(json!({ "deleted": true, "id": id }))))
}

pub(crate) async fn refresh_profile_handler(
    State(state): State<ServerContext>,
    AxumPath(id): AxumPath<String>,
) -> ApiResult<RefreshProfileResponse> {
    let profile_repository = ProfileRepository::new(state.database.as_ref());
    let profile = profile_repository
        .get_by_id(&id)
        .map_err(storage_error_to_response)?
        .ok_or_else(|| not_found_error_response("Profile 不存在"))?;
    let source_ids =
        list_profile_source_ids(state.database.as_ref(), &id).map_err(storage_error_to_response)?;

    let engine = Engine::with_refresh_registry(
        state.database.as_ref(),
        &state.plugins_dir,
        Arc::clone(&state.secret_store),
        state.refresh_registry.clone(),
    );
    state.profile_cache.invalidate(&id);
    let mut node_count = 0usize;
    for source_id in &source_ids {
        let result = engine
            .refresh_source(source_id, "manual-profile")
            .await
            .map_err(core_error_to_response)?;
        node_count = node_count.saturating_add(result.node_count);
        state
            .source_userinfo_cache
            .set(source_id, result.subscription_userinfo);
    }

    emit_event(
        &state,
        "profile:refreshed",
        format!(
            "Profile 刷新完成：{}（来源 {} 个）",
            profile.id,
            source_ids.len()
        ),
        None,
    );
    Ok((
        StatusCode::OK,
        Json(RefreshProfileResponse {
            profile_id: id,
            refreshed_sources: source_ids.len(),
            node_count,
        }),
    ))
}

fn generate_export_token() -> Result<String, getrandom::Error> {
    use base64::Engine as _;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;

    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes)?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

fn insert_profile_in_transaction(
    tx: &rusqlite::Transaction<'_>,
    profile: &Profile,
) -> app_storage::StorageResult<()> {
    tx.execute(
        "INSERT INTO profiles
         (id, name, description, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![
            profile.id,
            profile.name,
            profile.description,
            profile.created_at,
            profile.updated_at
        ],
    )?;
    Ok(())
}

fn update_profile_in_transaction(
    tx: &rusqlite::Transaction<'_>,
    profile: &Profile,
) -> app_storage::StorageResult<usize> {
    let affected = tx.execute(
        "UPDATE profiles
         SET name = ?1, description = ?2, updated_at = ?3
         WHERE id = ?4",
        rusqlite::params![
            profile.name,
            profile.description,
            profile.updated_at,
            profile.id
        ],
    )?;
    Ok(affected)
}

fn replace_profile_sources_in_transaction(
    tx: &rusqlite::Transaction<'_>,
    profile_id: &str,
    source_ids: &[String],
) -> app_storage::StorageResult<()> {
    tx.execute(
        "DELETE FROM profile_sources WHERE profile_id = ?1",
        [profile_id],
    )?;
    for (index, source_id) in source_ids.iter().enumerate() {
        tx.execute(
            "INSERT INTO profile_sources (profile_id, source_instance_id, priority)
             VALUES (?1, ?2, ?3)",
            rusqlite::params![profile_id, source_id, index as i64],
        )?;
    }
    Ok(())
}

fn persist_profile_routing_template_source_in_transaction(
    tx: &rusqlite::Transaction<'_>,
    profile_id: &str,
    routing_template_source_id: Option<&str>,
) -> app_storage::StorageResult<()> {
    let key = profile_routing_template_source_key(profile_id);
    match routing_template_source_id {
        Some(source_id) => {
            tx.execute(
                "INSERT INTO app_settings (key, value, updated_at)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(key)
                 DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
                rusqlite::params![
                    key,
                    source_id,
                    current_timestamp_rfc3339()
                        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
                ],
            )?;
        }
        None => {
            tx.execute("DELETE FROM app_settings WHERE key = ?1", [key])?;
        }
    }
    Ok(())
}

fn insert_profile_export_token_in_transaction(
    tx: &rusqlite::Transaction<'_>,
    profile_id: &str,
    token: &str,
) -> app_storage::StorageResult<()> {
    let created_at =
        current_timestamp_rfc3339().unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string());
    let id = format!(
        "export-token-{}",
        OffsetDateTime::now_utc().unix_timestamp_nanos()
    );
    tx.execute(
        "INSERT INTO export_tokens (id, profile_id, token, token_type, created_at, expires_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![
            id,
            profile_id,
            token,
            "primary",
            created_at,
            Option::<String>::None
        ],
    )?;
    Ok(())
}
