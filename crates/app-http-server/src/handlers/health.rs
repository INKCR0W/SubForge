use super::*;

pub(crate) async fn health_handler() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(HealthResponse {
            status: "ok",
            version: APP_VERSION,
        }),
    )
}
