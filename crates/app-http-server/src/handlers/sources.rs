use super::*;

pub(crate) async fn list_sources_handler(
    State(state): State<ServerContext>,
) -> ApiResult<SourceListResponse> {
    let service = SourceService::new(
        state.database.as_ref(),
        &state.plugins_dir,
        state.secret_store.as_ref(),
    );
    let sources = service
        .list_sources()
        .map_err(core_error_to_response)?
        .into_iter()
        .map(source_with_config_to_dto)
        .collect();
    Ok((StatusCode::OK, Json(SourceListResponse { sources })))
}

pub(crate) async fn create_source_handler(
    State(state): State<ServerContext>,
    Json(payload): Json<CreateSourceRequest>,
) -> ApiResult<SourceResponse> {
    let service = SourceService::new(
        state.database.as_ref(),
        &state.plugins_dir,
        state.secret_store.as_ref(),
    );
    let source = service
        .create_source(&payload.plugin_id, &payload.name, payload.config)
        .map_err(core_error_to_response)?;

    emit_event(
        &state,
        "source:created",
        format!("来源创建成功：{}", source.source.id),
        Some(source.source.id.clone()),
    );
    Ok((
        StatusCode::CREATED,
        Json(SourceResponse {
            source: source_with_config_to_dto(source),
        }),
    ))
}

pub(crate) async fn update_source_handler(
    State(state): State<ServerContext>,
    AxumPath(id): AxumPath<String>,
    Json(payload): Json<UpdateSourceRequest>,
) -> ApiResult<SourceResponse> {
    if payload.name.as_deref().map(str::trim).is_none() && payload.config.is_none() {
        return Err(config_error_response("至少提供 name 或 config 之一"));
    }

    let service = SourceService::new(
        state.database.as_ref(),
        &state.plugins_dir,
        state.secret_store.as_ref(),
    );

    let mut source = if let Some(config) = payload.config {
        service
            .update_source_config(&id, config)
            .map_err(core_error_to_response)?
    } else {
        service
            .get_source(&id)
            .map_err(core_error_to_response)?
            .ok_or_else(|| not_found_error_response("来源不存在"))?
    };

    if let Some(name) = payload.name {
        let name = name.trim();
        if name.is_empty() {
            return Err(config_error_response("name 不能为空"));
        }
        source.source.name = name.to_string();
        source.source.updated_at =
            current_timestamp_rfc3339().map_err(|_| internal_error_response())?;
        let source_repository = SourceRepository::new(state.database.as_ref());
        source_repository
            .update(&source.source)
            .map_err(storage_error_to_response)?;
    }

    emit_event(
        &state,
        "source:updated",
        format!("来源更新成功：{id}"),
        Some(id.clone()),
    );
    let profile_ids = list_profile_ids_by_source(state.database.as_ref(), &id)
        .map_err(storage_error_to_response)?;
    state.profile_cache.invalidate_many(&profile_ids);
    Ok((
        StatusCode::OK,
        Json(SourceResponse {
            source: source_with_config_to_dto(source),
        }),
    ))
}

pub(crate) async fn delete_source_handler(
    State(state): State<ServerContext>,
    AxumPath(id): AxumPath<String>,
) -> ApiResult<Value> {
    let related_profiles = list_profile_ids_by_source(state.database.as_ref(), &id)
        .map_err(storage_error_to_response)?;
    let service = SourceService::new(
        state.database.as_ref(),
        &state.plugins_dir,
        state.secret_store.as_ref(),
    );
    service.delete_source(&id).map_err(core_error_to_response)?;
    let settings_repository = SettingsRepository::new(state.database.as_ref());
    for profile_id in &related_profiles {
        let key = format!("profile.{profile_id}.clash_template_source_id");
        if let Ok(Some(setting)) = settings_repository.get(&key)
            && setting.value.trim() == id
        {
            let _ = settings_repository.delete(&key);
        }
    }
    state.profile_cache.invalidate_many(&related_profiles);
    state.source_userinfo_cache.set(&id, None);
    let _ = settings_repository.delete(&format!("source.{id}.clash_routing_template"));
    emit_event(
        &state,
        "source:deleted",
        format!("来源已删除：{id}"),
        Some(id.clone()),
    );
    Ok((StatusCode::OK, Json(json!({ "deleted": true, "id": id }))))
}

pub(crate) async fn refresh_source_handler(
    State(state): State<ServerContext>,
    AxumPath(id): AxumPath<String>,
) -> ApiResult<RefreshSourceResponse> {
    let source_id = id.clone();
    let database = Arc::clone(&state.database);
    let plugins_dir = state.plugins_dir.clone();
    let secret_store = Arc::clone(&state.secret_store);
    let task_result = tokio::spawn(async move {
        let engine = Engine::new(database.as_ref(), &plugins_dir, secret_store);
        engine.refresh_source(&source_id, "manual").await
    })
    .await;

    let result = match task_result {
        Ok(result) => result,
        Err(error) => {
            let finished_at = current_timestamp_rfc3339().unwrap_or_else(|_| {
                OffsetDateTime::now_utc()
                    .format(&time::format_description::well_known::Rfc3339)
                    .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
            });
            let refresh_repository = RefreshJobRepository::new(state.database.as_ref());
            let _ = refresh_repository.mark_running_failed_by_source(
                &id,
                &finished_at,
                "E_INTERNAL",
                "来源刷新任务异常中止，请重试",
            );
            emit_event(
                &state,
                "refresh:failed",
                format!("来源刷新异常中止：{id}，{error}"),
                Some(id),
            );
            return Err(internal_error_response());
        }
    };

    match result {
        Ok(refresh_result) => {
            state
                .source_userinfo_cache
                .set(&id, refresh_result.subscription_userinfo.clone());
            let profile_ids = list_profile_ids_by_source(state.database.as_ref(), &id)
                .map_err(storage_error_to_response)?;
            state.profile_cache.invalidate_many(&profile_ids);
            emit_event(
                &state,
                "refresh:complete",
                format!("来源刷新成功：{id}，节点 {} 条", refresh_result.node_count),
                Some(id.clone()),
            );
            Ok((
                StatusCode::OK,
                Json(RefreshSourceResponse {
                    source_id: id,
                    node_count: refresh_result.node_count,
                }),
            ))
        }
        Err(error) => {
            emit_event(
                &state,
                "refresh:failed",
                format!("来源刷新失败：{id}，{error}"),
                Some(id),
            );
            Err(core_error_to_response(error))
        }
    }
}
