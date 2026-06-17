use std::time::Instant;

use axum::{extract::Request, middleware::Next, response::Response};

use crate::state::AppState;

pub async fn record_request_metrics(
    axum::extract::State(state): axum::extract::State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let started = Instant::now();
    state.metrics.record_request();
    let response = next.run(request).await;
    state
        .metrics
        .record_request_duration(started.elapsed().as_secs_f64());
    if response.status().is_server_error() {
        state.metrics.record_error();
    }
    response
}
