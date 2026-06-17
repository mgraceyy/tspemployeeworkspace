use axum::http::request::Parts;
use axum::{
    extract::{FromRequestParts, Request, State},
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::auth::session::{get_active_session_from_db, UserSession};
use crate::error::AppError;
use crate::state::AppState;

#[derive(Clone)]
pub struct AuthUser(pub UserSession);

impl std::ops::Deref for AuthUser {
    type Target = UserSession;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<S> FromRequestParts<S> for AuthUser
where
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<UserSession>()
            .cloned()
            .map(AuthUser)
            .ok_or(AppError::Unauthorized)
    }
}

pub async fn inject_active_session(
    State(state): State<AppState>,
    session: tower_sessions::Session,
    mut request: Request,
    next: Next,
) -> Response {
    match get_active_session_from_db(&state.pool, &session).await {
        Ok(user) => {
            request.extensions_mut().insert(user);
            next.run(request).await
        }
        Err(error) => error.into_response(),
    }
}

pub async fn require_manager_role(request: Request, next: Next) -> Response {
    match request.extensions().get::<UserSession>() {
        Some(user) if user.role.is_manager_or_admin() => next.run(request).await,
        Some(_) => AppError::Forbidden.into_response(),
        None => AppError::Unauthorized.into_response(),
    }
}

pub async fn require_admin_role(request: Request, next: Next) -> Response {
    match request.extensions().get::<UserSession>() {
        Some(user) if user.role.is_admin() => next.run(request).await,
        Some(_) => AppError::Forbidden.into_response(),
        None => AppError::Unauthorized.into_response(),
    }
}
