pub mod app;
pub mod auth;
pub mod config;
pub mod db;
pub mod display;
pub mod error;
pub mod handlers;
pub mod metrics;
pub mod middleware;
pub mod models;
pub mod services;
pub mod state;
pub mod templates;

use std::sync::Arc;

use tokio::{signal, task::AbortHandle};
use tower_sessions::{
    cookie::{Key, SameSite},
    session_store::ExpiredDeletion,
    Expiry, SessionManagerLayer,
};
use tower_sessions_sqlx_store::PostgresStore;

use std::net::SocketAddr;

use crate::auth::login_limiter::LoginLimiter;
use crate::auth::post_limiter::PostRateLimiter;
use crate::auth::session::SESSION_IDLE_HOURS;
use crate::config::Config;
use crate::services::employees::{seed_admin_if_empty, seed_e2e_fixtures};
use crate::services::uploads::normalize_upload_dir;
use crate::state::AppState;
use crate::templates::engine;

pub async fn run() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    let config = Config::from_env()?;
    init_tracing(config.log_json);
    config.warn_if_insecure();
    let pool =
        db::connect_with_options(&config.database_url, config.database_max_connections).await?;
    db::migrate(&pool).await?;

    seed_admin_if_empty(&pool, config.seed_default_admin).await?;
    seed_e2e_fixtures(&pool, config.seed_e2e_fixtures).await?;

    let session_store = PostgresStore::new(pool.clone());
    session_store.migrate().await?;

    let deletion_task = tokio::spawn(
        session_store
            .clone()
            .continuously_delete_expired(tokio::time::Duration::from_secs(60)),
    );

    let session_key = Key::try_from(config.session_secret.as_bytes()).map_err(|_| {
        anyhow::anyhow!("SESSION_SECRET must be at least 64 bytes for signed session cookies")
    })?;
    let session_layer = SessionManagerLayer::new(session_store)
        .with_secure(config.session_secure)
        .with_same_site(SameSite::Lax)
        .with_expiry(Expiry::OnInactivity(time::Duration::hours(
            SESSION_IDLE_HOURS as i64,
        )))
        .with_signed(session_key);

    let upload_dir = normalize_upload_dir(&config.upload_dir);
    tokio::fs::create_dir_all(&upload_dir).await?;

    let (login_limiter, post_limiter) = if config.shared_rate_limits {
        let pool_for_limits = pool.clone();
        let cleanup_pool = pool_for_limits.clone();
        tokio::spawn(async move {
            let interval = tokio::time::Duration::from_secs(60);
            loop {
                tokio::time::sleep(interval).await;
                if let Err(err) =
                    crate::auth::rate_limit_store::RateLimitStore::cleanup_expired_postgres(
                        &cleanup_pool,
                    )
                    .await
                {
                    tracing::warn!(error = %err, "rate limit cleanup failed");
                }
            }
        });
        (
            LoginLimiter::postgres(pool_for_limits.clone()),
            PostRateLimiter::postgres(pool_for_limits),
        )
    } else {
        (LoginLimiter::in_memory(), PostRateLimiter::in_memory())
    };

    let state = AppState {
        pool,
        templates: engine(),
        login_limiter: Arc::new(login_limiter),
        post_limiter: Arc::new(post_limiter),
        metrics: Arc::new(crate::metrics::AppMetrics::default()),
        metrics_token: config.metrics_token.clone(),
        trust_proxy_headers: config.trust_proxy_headers,
        upload_dir,
        max_upload_bytes: config.max_upload_bytes,
    };

    let app = app::create_app(state, session_layer);

    let listen_addr = format!("{}:{}", config.bind_addr, config.port);
    let listener = tokio::net::TcpListener::bind(&listen_addr).await?;
    tracing::info!(
        secure_cookies = config.session_secure,
        trust_proxy_headers = config.trust_proxy_headers,
        shared_rate_limits = config.shared_rate_limits,
        listen = %listen_addr,
        "TalaSora Prime DTR running"
    );
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal(deletion_task.abort_handle()))
    .await?;

    deletion_task.await??;

    Ok(())
}

fn init_tracing(log_json: bool) {
    let env_filter = tracing_subscriber::EnvFilter::from_default_env();
    if log_json {
        tracing_subscriber::fmt()
            .json()
            .with_env_filter(env_filter)
            .init();
    } else {
        tracing_subscriber::fmt().with_env_filter(env_filter).init();
    }
}

async fn shutdown_signal(deletion_task_abort_handle: AbortHandle) {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => { deletion_task_abort_handle.abort(); },
        _ = terminate => { deletion_task_abort_handle.abort(); },
    }
}
