use std::collections::HashMap;

use super::*;

const DEFAULT_LOG_LIMIT: usize = 20;
const MAX_LOG_LIMIT: usize = 200;

pub(crate) async fn list_logs_handler(
    State(state): State<ServerContext>,
    Query(query): Query<LogsQuery>,
) -> ApiResult<LogsResponse> {
    let limit = query.limit.unwrap_or(DEFAULT_LOG_LIMIT);
    if limit == 0 || limit > MAX_LOG_LIMIT {
        return Err(config_error_response("limit 必须在 1..=200 之间"));
    }

    let status_filter = query
        .status
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    if let Some(status) = status_filter
        && !matches!(status, "running" | "success" | "failed")
    {
        return Err(config_error_response(
            "status 仅支持 running/success/failed",
        ));
    }

    let refresh_repository = RefreshJobRepository::new(state.database.as_ref());
    let source_repository = SourceRepository::new(state.database.as_ref());

    let refresh_jobs = if let Some(status) = status_filter {
        refresh_repository
            .list_recent_by_status(status, limit)
            .map_err(storage_error_to_response)?
    } else {
        refresh_repository
            .list_recent(limit)
            .map_err(storage_error_to_response)?
    };

    let source_names = source_repository
        .list()
        .map_err(storage_error_to_response)?
        .into_iter()
        .map(|source| (source.id, source.name))
        .collect::<HashMap<_, _>>();

    let logs = refresh_jobs
        .into_iter()
        .map(|job| RefreshLogDto {
            id: job.id,
            source_id: job.source_instance_id.clone(),
            source_name: source_names.get(&job.source_instance_id).cloned(),
            trigger_type: job.trigger_type,
            status: job.status,
            started_at: job.started_at,
            finished_at: job.finished_at,
            node_count: job.node_count,
            error_code: job.error_code,
            error_message: job.error_message,
        })
        .collect();

    Ok((StatusCode::OK, Json(LogsResponse { logs })))
}
