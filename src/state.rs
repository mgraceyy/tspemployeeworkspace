use std::path::PathBuf;
use std::sync::Arc;

use sqlx::PgPool;

use crate::auth::login_limiter::LoginLimiter;
use crate::auth::post_limiter::PostRateLimiter;
use crate::metrics::AppMetrics;
use crate::templates::TemplateEngine;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub templates: Arc<TemplateEngine>,
    pub login_limiter: Arc<LoginLimiter>,
    pub post_limiter: Arc<PostRateLimiter>,
    pub metrics: Arc<AppMetrics>,
    pub metrics_token: Option<String>,
    pub trust_proxy_headers: bool,
    pub upload_dir: PathBuf,
    pub max_upload_bytes: usize,
}
