use app_common::SourceInstance;
use app_core::CoreError;
use app_plugin_runtime::PluginRuntimeError;
use app_storage::{
    RefreshJob, RefreshJobRepository, ScriptLog, ScriptLogRepository, SourceRepository,
    StorageError,
};
use axum::Json;
use axum::body::Body;
use axum::http::Method;
use tower::ServiceExt;

use crate::build_router;
use crate::helpers::{core_error_to_response, emit_event, storage_error_to_response};

use super::{admin_request, build_test_state, read_json};

#[test]
fn storage_error_response_does_not_expose_internal_details() {
    let io_error = std::io::Error::other("open C:\\secret\\subforge.db failed");
    let (_, Json(payload)) = storage_error_to_response(StorageError::Io(io_error));
    assert_eq!(payload.code, "E_INTERNAL");
    assert_eq!(payload.message, "Internal server error");
}

#[test]
fn core_internal_error_response_does_not_expose_internal_details() {
    let io_error = std::io::Error::other("permission denied: /var/lib/subforge/subforge.db");
    let (_, Json(payload)) = core_error_to_response(CoreError::Storage(StorageError::Io(io_error)));
    assert_eq!(payload.code, "E_INTERNAL");
    assert_eq!(payload.message, "Internal server error");
}

#[test]
fn core_script_runtime_error_keeps_runtime_message() {
    let (_, Json(payload)) = core_error_to_response(CoreError::PluginRuntime(
        PluginRuntimeError::ScriptRuntime("script failed".to_string()),
    ));
    assert_eq!(payload.code, "E_SCRIPT_RUNTIME");
    assert_eq!(payload.message, "script failed");
}

#[test]
fn core_script_runtime_error_redacts_sensitive_message() {
    let (_, Json(payload)) =
        core_error_to_response(CoreError::PluginRuntime(PluginRuntimeError::ScriptRuntime(
            "script failed: Bearer secret-token token=abc password=hunter2".to_string(),
        )));

    assert_eq!(payload.code, "E_SCRIPT_RUNTIME");
    assert!(payload.message.contains("Bearer ***"));
    assert!(payload.message.contains("token=***"));
    assert!(payload.message.contains("password=***"));
    assert!(!payload.message.contains("secret-token"));
    assert!(!payload.message.contains("abc"));
    assert!(!payload.message.contains("hunter2"));
}

#[test]
fn core_plugin_invalid_error_maps_to_bad_request_message() {
    let (status, Json(payload)) = core_error_to_response(CoreError::PluginRuntime(
        PluginRuntimeError::Invalid("缺少入口脚本 fetch".to_string()),
    ));
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
    assert_eq!(payload.code, "E_PLUGIN_INVALID");
    assert_eq!(payload.message, "缺少入口脚本 fetch");
}

#[tokio::test]
async fn list_logs_handler_redacts_persisted_refresh_and_script_messages() {
    let state = build_test_state();
    SourceRepository::new(state.database.as_ref())
        .insert(&SourceInstance {
            id: "source-sensitive".to_string(),
            plugin_id: "vendor.example.script".to_string(),
            name: "Sensitive Source".to_string(),
            status: "healthy".to_string(),
            state_json: None,
            created_at: "2026-05-30T00:00:00Z".to_string(),
            updated_at: "2026-05-30T00:00:00Z".to_string(),
        })
        .expect("插入测试来源失败");
    RefreshJobRepository::new(state.database.as_ref())
        .insert(&RefreshJob {
            id: "refresh-sensitive".to_string(),
            source_instance_id: "source-sensitive".to_string(),
            trigger_type: "manual".to_string(),
            status: "failed".to_string(),
            started_at: Some("2026-05-30T00:00:00Z".to_string()),
            finished_at: Some("2026-05-30T00:00:01Z".to_string()),
            node_count: None,
            error_code: Some("E_SCRIPT_RUNTIME".to_string()),
            error_message: Some(
                "fetch failed: Bearer refresh-token password=refresh-password".to_string(),
            ),
        })
        .expect("插入测试 refresh_job 失败");
    ScriptLogRepository::new(state.database.as_ref())
        .insert(&ScriptLog {
            id: "script-log-sensitive".to_string(),
            refresh_job_id: "refresh-sensitive".to_string(),
            source_instance_id: "source-sensitive".to_string(),
            plugin_id: "vendor.example.script".to_string(),
            level: "error".to_string(),
            message: "script log token=script-token api_key=script-key".to_string(),
            created_at: "2026-05-30T00:00:01Z".to_string(),
        })
        .expect("插入测试 script_log 失败");

    let response = build_router(state)
        .oneshot(admin_request(
            Method::GET,
            "/api/logs?source_id=source-sensitive&limit=5&include_script_logs=true",
            Body::empty(),
        ))
        .await
        .expect("读取日志请求执行失败");
    let payload = read_json(response).await;
    let serialized = serde_json::to_string(&payload).expect("日志响应应可序列化");

    assert!(serialized.contains("Bearer ***"));
    assert!(serialized.contains("password=***"));
    assert!(serialized.contains("token=***"));
    assert!(serialized.contains("api_key=***"));
    assert!(!serialized.contains("refresh-token"));
    assert!(!serialized.contains("refresh-password"));
    assert!(!serialized.contains("script-token"));
    assert!(!serialized.contains("script-key"));
}

#[tokio::test]
async fn emit_event_redacts_sensitive_message_before_broadcast() {
    let state = build_test_state();
    let mut receiver = state.event_sender.subscribe();

    emit_event(
        &state,
        "refresh:failed",
        "failed with Bearer event-token secret=event-secret".to_string(),
        Some("source-sensitive".to_string()),
    );

    let event = receiver.recv().await.expect("应收到测试事件");
    assert_eq!(event.event, "refresh:failed");
    assert!(event.message.contains("Bearer ***"));
    assert!(event.message.contains("secret=***"));
    assert!(!event.message.contains("event-token"));
    assert!(!event.message.contains("event-secret"));
}
