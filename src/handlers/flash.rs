use axum::response::Redirect;
use tower_sessions::Session;

use crate::auth::session::set_flash;
use crate::error::{AppError, AppResult};

pub async fn redirect_with_flash(
    session: &Session,
    url: &str,
    kind: &str,
    message: &str,
) -> AppResult<Redirect> {
    set_flash(session, kind, message).await?;
    Ok(Redirect::to(url))
}

fn user_error_flash(err: &AppError) -> Option<(&'static str, String)> {
    match err {
        AppError::BadRequest(msg) => {
            if msg == "Overtime is not pending approval" {
                Some(("info", "This overtime was already reviewed".into()))
            } else {
                Some(("error", msg.clone()))
            }
        }
        AppError::Forbidden => Some((
            "error",
            "You don't have permission for this action".into(),
        )),
        AppError::NotFound => Some(("error", "That record was not found".into())),
        AppError::TooManyRequests(msg) => Some(("error", msg.clone())),
        _ => None,
    }
}

/// Redirect with a success flash, or map common user-facing errors to an error/info flash.
pub async fn redirect_with_flash_from_result(
    session: &Session,
    url: &str,
    success_message: &str,
    result: AppResult<()>,
) -> AppResult<Redirect> {
    redirect_with_flash_from_result_urls(session, url, url, success_message, result).await
}

/// Like [`redirect_with_flash_from_result`], but allows a different redirect on user-facing errors.
pub async fn redirect_with_flash_from_result_urls(
    session: &Session,
    success_url: &str,
    error_url: &str,
    success_message: &str,
    result: AppResult<()>,
) -> AppResult<Redirect> {
    match result {
        Ok(()) => redirect_with_flash(session, success_url, "success", success_message).await,
        Err(err) => {
            if let Some((kind, message)) = user_error_flash(&err) {
                redirect_with_flash(session, error_url, kind, &message).await
            } else {
                Err(err)
            }
        }
    }
}