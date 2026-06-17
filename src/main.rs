mod app;
mod auth;
mod config;
mod db;
mod display;
mod error;
mod handlers;
mod models;
mod services;
mod state;
mod templates;

use std::sync::Arc;

use tokio::{signal, task::AbortHandle};
use tower_sessions::{
    cookie::{Key, SameSite},
    session_store::ExpiredDeletion, Expiry, SessionManagerLayer,
};
use tower_sessions_sqlx_store::PostgresStore;

use crate::auth::login_limiter::LoginLimiter;
use crate::config::Config;
use crate::services::employees::seed_admin_if_empty;
use crate::state::AppState;
use crate::templates::engine;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let config = Config::from_env()?;
    let pool = db::connect(&config.database_url).await?;
    db::migrate(&pool).await?;

    seed_admin_if_empty(&pool, config.seed_default_admin).await?;

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
        .with_secure(false)
        .with_same_site(SameSite::Lax)
        .with_expiry(Expiry::OnInactivity(time::Duration::hours(12)))
        .with_signed(session_key);

    let state = AppState {
        pool,
        templates: engine(),
        login_limiter: Arc::new(LoginLimiter::new()),
    };

    let app = app::create_app(state, session_layer);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
    tracing::info!("TalaSora Prime DTR running on http://0.0.0.0:8080");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal(deletion_task.abort_handle()))
        .await?;

    deletion_task.await??;

    Ok(())
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