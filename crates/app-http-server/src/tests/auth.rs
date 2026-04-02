use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header::HOST};
use tower::ServiceExt;

use super::*;

#[tokio::test]
async fn plugins_api_requires_admin_token() {
    let app = build_router(build_test_state());
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/plugins")
                .header(HOST, "127.0.0.1:18118")
                .body(Body::empty())
                .expect("创建请求失败"),
        )
        .await
        .expect("请求执行失败");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn plugins_api_rejects_query_admin_token() {
    let app = build_router(build_test_state());
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/plugins?token=test-admin-token")
                .header(HOST, "127.0.0.1:18118")
                .body(Body::empty())
                .expect("创建请求失败"),
        )
        .await
        .expect("请求执行失败");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn plugins_api_accepts_admin_header() {
    let app = build_router(build_test_state());
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/plugins")
                .header(HOST, "127.0.0.1:18118")
                .header("authorization", "Bearer test-admin-token")
                .body(Body::empty())
                .expect("创建请求失败"),
        )
        .await
        .expect("请求执行失败");
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 1024 * 64)
        .await
        .expect("读取响应体失败");
    let raw = String::from_utf8(body.to_vec()).expect("响应体不是 UTF-8");
    assert!(raw.contains("\"plugins\""));
}

#[tokio::test]
async fn options_preflight_returns_204_without_cors_header() {
    let app = build_router(build_test_state());
    let response = app
        .oneshot(
            Request::builder()
                .method("OPTIONS")
                .uri("/api/plugins")
                .header(HOST, "127.0.0.1:18118")
                .body(Body::empty())
                .expect("创建请求失败"),
        )
        .await
        .expect("请求执行失败");
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert!(
        response
            .headers()
            .get("access-control-allow-origin")
            .is_none()
    );
}
