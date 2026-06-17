use axum::{
    http::StatusCode,
    response::{IntoResponse, Redirect, Response},
};
use minijinja::Error as TemplateError;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("unauthorized")]
    Unauthorized,
    #[error("forbidden")]
    Forbidden,
    #[error("pin change required")]
    PinChangeRequired,
    #[error("not found")]
    NotFound,
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("too many requests: {0}")]
    TooManyRequests(String),
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

impl AppError {
    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self::BadRequest(msg.into())
    }

    pub fn too_many_requests(msg: impl Into<String>) -> Self {
        Self::TooManyRequests(msg.into())
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        match self {
            AppError::Unauthorized => Redirect::to("/login").into_response(),
            AppError::PinChangeRequired => Redirect::to("/change-pin").into_response(),
            AppError::Forbidden => (StatusCode::FORBIDDEN, "Forbidden").into_response(),
            AppError::NotFound => (StatusCode::NOT_FOUND, "Not found").into_response(),
            AppError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg).into_response(),
            AppError::TooManyRequests(msg) => (StatusCode::TOO_MANY_REQUESTS, msg).into_response(),
            AppError::Internal(err) => {
                tracing::error!(error = %err, "internal error");
                (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error").into_response()
            }
        }
    }
}

pub type AppResult<T> = Result<T, AppError>;

impl From<TemplateError> for AppError {
    fn from(value: TemplateError) -> Self {
        AppError::Internal(value.into())
    }
}
