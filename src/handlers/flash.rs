use axum::response::Redirect;
use tower_sessions::Session;

use crate::auth::session::set_flash;
use crate::error::AppResult;

pub async fn redirect_with_flash(
    session: &Session,
    url: &str,
    kind: &str,
    message: &str,
) -> AppResult<Redirect> {
    set_flash(session, kind, message).await?;
    Ok(Redirect::to(url))
}
