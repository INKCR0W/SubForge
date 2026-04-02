use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header::CONTENT_TYPE, header::HOST};
use serde_json::{Value, json};
use tower::ServiceExt;

use super::*;

#[tokio::test]
async fn e2e_import_source_refresh_and_raw_profile_output() {
    let state = build_test_state();
    let mut event_receiver = state.event_sender.subscribe();
    let app = build_router(state.clone());

    let (upstream_base, server_task) = start_fixture_server(
        BASE64_SUBSCRIPTION_FIXTURE.trim().to_string(),
        "text/plain; charset=utf-8",
    )
    .await;

    let boundary = "----subforge-e2e-boundary";
    let plugin_zip = build_builtin_plugin_zip_bytes();
    let import_body = build_multipart_plugin_body(boundary, &plugin_zip, "builtin-static.zip");
    let import_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/plugins/import")
                .header(HOST, "127.0.0.1:18118")
                .header("authorization", "Bearer test-admin-token")
                .header(
                    CONTENT_TYPE,
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from(import_body))
                .expect("构建导入插件请求失败"),
        )
        .await
        .expect("导入插件请求执行失败");
    assert_eq!(import_response.status(), StatusCode::CREATED);

    let source_response = app
        .clone()
        .oneshot(admin_json_request(
            Method::POST,
            "/api/sources",
            &json!({
                "plugin_id": "subforge.builtin.static",
                "name": "E2E Source",
                "config": {
                    "url": format!("{upstream_base}/sub")
                }
            }),
        ))
        .await
        .expect("创建来源请求执行失败");
    assert_eq!(source_response.status(), StatusCode::CREATED);
    let source_payload = read_json(source_response).await;
    let source_id = source_payload
        .pointer("/source/source/id")
        .and_then(Value::as_str)
        .expect("来源响应缺少 source.id")
        .to_string();

    let profile_response = app
        .clone()
        .oneshot(admin_json_request(
            Method::POST,
            "/api/profiles",
            &json!({
                "name": "E2E Profile",
                "source_ids": [source_id.clone()]
            }),
        ))
        .await
        .expect("创建 Profile 请求执行失败");
    assert_eq!(profile_response.status(), StatusCode::CREATED);
    let profile_payload = read_json(profile_response).await;
    let profile_id = profile_payload
        .pointer("/profile/profile/id")
        .and_then(Value::as_str)
        .expect("Profile 响应缺少 id")
        .to_string();

    let export_token_repository = ExportTokenRepository::new(state.database.as_ref());
    let export_token = export_token_repository
        .get_active_token(&profile_id)
        .expect("读取 export_token 失败")
        .expect("创建 Profile 后应自动生成 export_token")
        .token;

    let refresh_response = app
        .clone()
        .oneshot(admin_request(
            Method::POST,
            &format!("/api/sources/{source_id}/refresh"),
            Body::empty(),
        ))
        .await
        .expect("刷新来源请求执行失败");
    assert_eq!(refresh_response.status(), StatusCode::OK);
    let refresh_payload = read_json(refresh_response).await;
    assert_eq!(
        refresh_payload.get("source_id").and_then(Value::as_str),
        Some(source_id.as_str())
    );
    assert_eq!(
        refresh_payload.get("node_count").and_then(Value::as_u64),
        Some(3)
    );

    let refresh_repository = RefreshJobRepository::new(state.database.as_ref());
    let refresh_jobs = refresh_repository
        .list_by_source(&source_id)
        .expect("读取 refresh_jobs 失败");
    assert_eq!(refresh_jobs.len(), 1);
    assert_eq!(refresh_jobs[0].status, "success");
    assert_eq!(refresh_jobs[0].node_count, Some(3));

    let event = wait_refresh_complete_event(&mut event_receiver, &source_id).await;
    assert_eq!(event.event, "refresh:complete");
    assert_eq!(event.source_id.as_deref(), Some(source_id.as_str()));

    let raw_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri(format!(
                    "/api/profiles/{profile_id}/raw?token={export_token}"
                ))
                .header(HOST, "127.0.0.1:18118")
                .body(Body::empty())
                .expect("构建 raw 请求失败"),
        )
        .await
        .expect("读取 raw 订阅请求执行失败");
    assert_eq!(raw_response.status(), StatusCode::OK);
    let raw_payload = read_json(raw_response).await;
    assert_eq!(
        raw_payload.get("profile_id").and_then(Value::as_str),
        Some(profile_id.as_str())
    );
    assert_eq!(
        raw_payload.get("node_count").and_then(Value::as_u64),
        Some(3)
    );
    assert_eq!(
        raw_payload
            .get("nodes")
            .and_then(Value::as_array)
            .map(|items| items.len()),
        Some(3)
    );

    server_task.abort();
}

#[tokio::test]
async fn e2e_script_source_refresh_via_management_api() {
    let state = build_test_state();
    let mut event_receiver = state.event_sender.subscribe();
    let app = build_router(state.clone());

    let (upstream_base, server_task) = start_fixture_server(
        BASE64_SUBSCRIPTION_FIXTURE.trim().to_string(),
        "text/plain; charset=utf-8",
    )
    .await;

    let boundary = "----subforge-e2e-script-boundary";
    let plugin_zip = build_script_mock_plugin_zip_bytes();
    let import_body = build_multipart_plugin_body(boundary, &plugin_zip, "script-mock.zip");
    let import_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/plugins/import")
                .header(HOST, "127.0.0.1:18118")
                .header("authorization", "Bearer test-admin-token")
                .header(
                    CONTENT_TYPE,
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from(import_body))
                .expect("构建脚本插件导入请求失败"),
        )
        .await
        .expect("导入脚本插件请求执行失败");
    assert_eq!(import_response.status(), StatusCode::CREATED);

    let source_response = app
        .clone()
        .oneshot(admin_json_request(
            Method::POST,
            "/api/sources",
            &json!({
                "plugin_id": "vendor.example.script-mock",
                "name": "Script E2E Source",
                "config": {
                    "subscription_url": format!("{upstream_base}/sub"),
                    "username": "alice",
                    "password": "wonderland"
                }
            }),
        ))
        .await
        .expect("创建脚本来源请求执行失败");
    assert_eq!(source_response.status(), StatusCode::CREATED);
    let source_payload = read_json(source_response).await;
    let source_id = source_payload
        .pointer("/source/source/id")
        .and_then(Value::as_str)
        .expect("脚本来源响应缺少 source.id")
        .to_string();

    let profile_response = app
        .clone()
        .oneshot(admin_json_request(
            Method::POST,
            "/api/profiles",
            &json!({
                "name": "Script E2E Profile",
                "source_ids": [source_id.clone()]
            }),
        ))
        .await
        .expect("创建脚本 Profile 请求执行失败");
    assert_eq!(profile_response.status(), StatusCode::CREATED);
    let profile_payload = read_json(profile_response).await;
    let profile_id = profile_payload
        .pointer("/profile/profile/id")
        .and_then(Value::as_str)
        .expect("脚本 Profile 响应缺少 id")
        .to_string();

    let export_token_repository = ExportTokenRepository::new(state.database.as_ref());
    let export_token = export_token_repository
        .get_active_token(&profile_id)
        .expect("读取脚本 Profile export_token 失败")
        .expect("创建脚本 Profile 后应自动生成 export_token")
        .token;

    let refresh_response = app
        .clone()
        .oneshot(admin_request(
            Method::POST,
            &format!("/api/sources/{source_id}/refresh"),
            Body::empty(),
        ))
        .await
        .expect("刷新脚本来源请求执行失败");
    let refresh_status = refresh_response.status();
    let refresh_payload = read_json(refresh_response).await;
    assert_eq!(
        refresh_status,
        StatusCode::OK,
        "脚本来源刷新应成功，实际返回：{refresh_payload:?}"
    );
    assert_eq!(
        refresh_payload.get("source_id").and_then(Value::as_str),
        Some(source_id.as_str())
    );
    assert_eq!(
        refresh_payload.get("node_count").and_then(Value::as_u64),
        Some(3)
    );

    let refresh_repository = RefreshJobRepository::new(state.database.as_ref());
    let refresh_jobs = refresh_repository
        .list_by_source(&source_id)
        .expect("读取脚本 refresh_jobs 失败");
    assert_eq!(refresh_jobs.len(), 1);
    assert_eq!(refresh_jobs[0].status, "success");
    assert_eq!(refresh_jobs[0].node_count, Some(3));

    let event = wait_refresh_complete_event(&mut event_receiver, &source_id).await;
    assert_eq!(event.event, "refresh:complete");
    assert_eq!(event.source_id.as_deref(), Some(source_id.as_str()));

    let raw_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri(format!(
                    "/api/profiles/{profile_id}/raw?token={export_token}"
                ))
                .header(HOST, "127.0.0.1:18118")
                .body(Body::empty())
                .expect("构建脚本 raw 请求失败"),
        )
        .await
        .expect("读取脚本 raw 订阅请求执行失败");
    assert_eq!(raw_response.status(), StatusCode::OK);
    let raw_payload = read_json(raw_response).await;
    assert_eq!(
        raw_payload.get("profile_id").and_then(Value::as_str),
        Some(profile_id.as_str())
    );
    assert_eq!(
        raw_payload.get("node_count").and_then(Value::as_u64),
        Some(3)
    );

    let source_repository = app_storage::SourceRepository::new(state.database.as_ref());
    let persisted_state_raw = source_repository
        .get_by_id(&source_id)
        .expect("读取脚本来源失败")
        .and_then(|source| source.state_json)
        .expect("脚本来源刷新后应写入 state_json");
    let persisted_state: Value =
        serde_json::from_str(&persisted_state_raw).expect("state_json 必须是合法 JSON");
    assert_eq!(
        persisted_state.get("counter").and_then(Value::as_u64),
        Some(3)
    );

    server_task.abort();
}
