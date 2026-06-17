use std::net::SocketAddr;
use std::time::Duration;

use axum::{
    extract::{ConnectInfo, FromRequestParts, Request, State},
    http::{request::Parts, Method},
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::auth::client_ip::client_ip;
use crate::auth::rate_limit_store::RateLimitStore;
use crate::error::AppError;
use crate::state::AppState;

const MAX_POSTS: usize = 120;
const WINDOW: Duration = Duration::from_secs(60);

#[derive(Clone)]
pub struct PostRateLimiter {
    store: RateLimitStore,
}

impl PostRateLimiter {
    pub fn in_memory() -> Self {
        Self {
            store: RateLimitStore::in_memory(),
        }
    }

    pub fn postgres(pool: sqlx::PgPool) -> Self {
        Self {
            store: RateLimitStore::postgres(pool),
        }
    }

    pub async fn is_limited(&self, key: &str) -> Result<bool, AppError> {
        Ok(self.store.count_recent(key, WINDOW).await? >= MAX_POSTS)
    }

    pub async fn record(&self, key: &str) -> Result<(), AppError> {
        self.store.record(key, WINDOW).await
    }
}

pub struct ClientIp(pub String);

impl FromRequestParts<crate::state::AppState> for ClientIp {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &crate::state::AppState,
    ) -> Result<Self, Self::Rejection> {
        let connect_info = parts.extensions.get::<ConnectInfo<SocketAddr>>().cloned();
        Ok(ClientIp(client_ip(
            state.trust_proxy_headers,
            connect_info,
            &parts.headers,
        )))
    }
}

pub async fn limit_post_requests(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    if request.method() != Method::POST {
        return next.run(request).await;
    }

    let path = request.uri().path();
    if path == "/health" {
        return next.run(request).await;
    }

    let connect_info = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .cloned();
    let key = client_ip(state.trust_proxy_headers, connect_info, request.headers());

    match state.post_limiter.is_limited(&key).await {
        Ok(true) => {
            return AppError::too_many_requests(
                "Too many requests. Please wait a moment and try again.",
            )
            .into_response();
        }
        Ok(false) => {}
        Err(error) => return error.into_response(),
    }

    if let Err(error) = state.post_limiter.record(&key).await {
        return error.into_response();
    }

    next.run(request).await
}

pub use crate::auth::client_ip::client_ip as resolve_client_ip;
