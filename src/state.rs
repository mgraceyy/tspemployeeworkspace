use std::sync::Arc;

use sqlx::PgPool;

use crate::auth::login_limiter::LoginLimiter;
use crate::templates::TemplateEngine;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub templates: Arc<TemplateEngine>,
    pub login_limiter: Arc<LoginLimiter>,
}