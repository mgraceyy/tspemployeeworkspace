use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;

use crate::state::AppState;

#[derive(Serialize)]
pub struct HealthResponse {
    status: &'static str,
    database: &'static str,
}

pub async fn health(State(state): State<AppState>) -> Response {
    let database_ok = sqlx::query("SELECT 1").execute(&state.pool).await.is_ok();

    let (status, body) = if database_ok {
        (
            StatusCode::OK,
            HealthResponse {
                status: "ok",
                database: "ok",
            },
        )
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            HealthResponse {
                status: "degraded",
                database: "error",
            },
        )
    };

    (status, Json(body)).into_response()
}
