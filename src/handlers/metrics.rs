use axum::{
    extract::{Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::Deserialize;

use crate::state::AppState;

#[derive(Deserialize)]
pub struct MetricsQuery {
    token: Option<String>,
}

fn metrics_authorized(headers: &HeaderMap, query: &MetricsQuery, expected: &str) -> bool {
    if let Some(auth) = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
    {
        if let Some(token) = auth.strip_prefix("Bearer ") {
            if token == expected {
                return true;
            }
        }
    }

    query
        .token
        .as_deref()
        .is_some_and(|token| token == expected)
}

pub async fn prometheus_metrics(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<MetricsQuery>,
) -> Response {
    if let Some(expected) = state.metrics_token.as_deref() {
        if !metrics_authorized(&headers, &query, expected) {
            return StatusCode::UNAUTHORIZED.into_response();
        }
    }

    let body = state
        .metrics
        .render_prometheus(state.pool.size(), state.pool.num_idle() as u32);
    (
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        body,
    )
        .into_response()
}
