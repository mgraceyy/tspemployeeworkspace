#![allow(dead_code)]

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use tokio::sync::Mutex;

use axum::{
    body::Body,
    http::{header, HeaderMap, Request, StatusCode},
    Router,
};
use dtr::app::create_app;
use dtr::auth::login_limiter::LoginLimiter;
use dtr::auth::post_limiter::PostRateLimiter;
use dtr::error::AppResult;
use dtr::metrics::AppMetrics;
use dtr::models::UserRole;
use dtr::services::employees::create_employee;
use dtr::state::AppState;
use dtr::templates::engine;
use sqlx::{postgres::PgPoolOptions, PgPool};
use tower::ServiceExt;
use tower_sessions::{cookie::SameSite, Expiry, MemoryStore, SessionManagerLayer};
use uuid::Uuid;

#[derive(Clone)]
pub struct TestAppConfig {
    pub shared_rate_limits: bool,
    pub trust_proxy_headers: bool,
    pub max_upload_bytes: usize,
    pub metrics_token: Option<String>,
}

impl Default for TestAppConfig {
    fn default() -> Self {
        Self {
            // Integration tests opt in via TestAppConfig { shared_rate_limits: true, .. }.
            shared_rate_limits: false,
            trust_proxy_headers: false,
            max_upload_bytes: 5 * 1024 * 1024,
            metrics_token: None,
        }
    }
}

static MIGRATIONS_DONE: OnceLock<()> = OnceLock::new();
static TEST_DB_POOL: OnceLock<PgPool> = OnceLock::new();
static TEST_DB_RESET_LOCK: Mutex<()> = Mutex::const_new(());

/// One process-wide pool for setup, HTTP handlers, and reset. Keeps total Postgres
/// checkouts bounded so sequential integration tests do not exhaust connections.
const TEST_POOL_MAX_CONNECTIONS: u32 = 3;
const TEST_POOL_ACQUIRE_TIMEOUT: Duration = Duration::from_secs(10);
const RESET_MAX_ATTEMPTS: u32 = 3;

fn test_pool_options(max_connections: u32, acquire_timeout: Duration) -> PgPoolOptions {
    PgPoolOptions::new()
        .max_connections(max_connections)
        .acquire_timeout(acquire_timeout)
        .idle_timeout(Some(Duration::from_secs(5)))
        .before_acquire(|conn, _meta| {
            Box::pin(async move {
                sqlx::query("DISCARD ALL").execute(&mut *conn).await?;
                Ok(true)
            })
        })
}

async fn connect_pool(
    max_connections: u32,
    acquire_timeout: Duration,
    label: &str,
) -> Result<PgPool, sqlx::Error> {
    let url = std::env::var("DATABASE_URL").map_err(|e| sqlx::Error::Configuration(e.into()))?;
    test_pool_options(max_connections, acquire_timeout)
        .connect(&url)
        .await
        .map_err(|e| {
            eprintln!("{label} connection failed: {e}");
            e
        })
}

async fn shared_test_pool() -> Result<PgPool, sqlx::Error> {
    if let Some(pool) = TEST_DB_POOL.get() {
        return Ok(pool.clone());
    }
    let pool = connect_pool(
        TEST_POOL_MAX_CONNECTIONS,
        TEST_POOL_ACQUIRE_TIMEOUT,
        "test database",
    )
    .await?;
    let _ = TEST_DB_POOL.set(pool);
    Ok(TEST_DB_POOL.get().expect("test database pool").clone())
}

async fn try_reset_shared_test_state(pool: &PgPool) -> Result<(), sqlx::Error> {
    let _guard = TEST_DB_RESET_LOCK.lock().await;
    let mut conn = pool.acquire().await?;
    sqlx::query("DELETE FROM closed_pay_periods")
        .execute(&mut *conn)
        .await?;
    sqlx::query("DELETE FROM rate_limit_events")
        .execute(&mut *conn)
        .await?;
    Ok(())
}

pub async fn reset_shared_test_state(pool: &PgPool) {
    for attempt in 1..=RESET_MAX_ATTEMPTS {
        match try_reset_shared_test_state(pool).await {
            Ok(()) => return,
            Err(error) if attempt < RESET_MAX_ATTEMPTS => {
                eprintln!("reset shared test state attempt {attempt} failed: {error}");
                tokio::time::sleep(Duration::from_millis(250 * u64::from(attempt))).await;
            }
            Err(error) => panic!(
                "reset shared test state failed after {RESET_MAX_ATTEMPTS} attempts: {error}"
            ),
        }
    }
}

pub fn url_encode(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char);
            }
            b' ' => encoded.push_str("%20"),
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

pub async fn clear_must_change_pin(pool: &PgPool, employee_id: Uuid) -> AppResult<()> {
    sqlx::query("UPDATE employees SET must_change_pin = FALSE WHERE id = $1")
        .bind(employee_id)
        .execute(pool)
        .await
        .map_err(|e| dtr::error::AppError::Internal(e.into()))?;
    Ok(())
}

pub async fn create_ready_employee(
    pool: &PgPool,
    employee_code: &str,
    full_name: &str,
    pin: &str,
    role: UserRole,
    manager_id: Option<Uuid>,
) -> AppResult<dtr::models::EmployeeSummary> {
    let employee = create_employee(pool, employee_code, full_name, pin, role, manager_id).await?;
    clear_must_change_pin(pool, employee.id).await?;
    Ok(employee)
}

async fn ensure_migrations(pool: &PgPool) {
    if MIGRATIONS_DONE.get().is_some() {
        return;
    }
    dtr::db::migrate(pool)
        .await
        .expect("test database migrations");
    let _ = MIGRATIONS_DONE.set(());
}

pub async fn test_pool() -> Option<PgPool> {
    dotenvy::dotenv().ok();
    std::env::var("DATABASE_URL").ok()?;

    let pool = match shared_test_pool().await {
        Ok(pool) => pool,
        Err(e) => {
            if std::env::var_os("CI").is_some() {
                panic!("test database pool connection failed in CI: {e}");
            }
            return None;
        }
    };

    ensure_migrations(&pool).await;
    reset_shared_test_state(&pool).await;
    Some(pool)
}

pub async fn test_app(pool: PgPool) -> Router {
    test_app_with_config(pool, TestAppConfig::default()).await
}

pub async fn test_app_with_config(pool: PgPool, config: TestAppConfig) -> Router {
    // Fresh in-memory store per app so sessions never leak across tests.
    let session_store = MemoryStore::default();

    // Plaintext session cookies: our oneshot client replays raw Cookie headers and
    // cannot round-trip signed cookie values the way a browser would.
    let session_layer = SessionManagerLayer::new(session_store)
        .with_secure(false)
        .with_same_site(SameSite::Lax)
        .with_expiry(Expiry::OnInactivity(time::Duration::hours(8)))
        .with_always_save(true);

    let (login_limiter, post_limiter) = if config.shared_rate_limits {
        (
            LoginLimiter::postgres(pool.clone()),
            PostRateLimiter::postgres(pool.clone()),
        )
    } else {
        (LoginLimiter::in_memory(), PostRateLimiter::in_memory())
    };

    let state = AppState {
        pool,
        templates: engine(),
        login_limiter: Arc::new(login_limiter),
        post_limiter: Arc::new(post_limiter),
        metrics: Arc::new(AppMetrics::default()),
        metrics_token: config.metrics_token.clone(),
        trust_proxy_headers: config.trust_proxy_headers,
        upload_dir: std::env::temp_dir().join("dtr-http-test-uploads"),
        max_upload_bytes: config.max_upload_bytes,
    };

    create_app(state, session_layer)
}

pub fn cookie_header(set_cookie: Option<&header::HeaderValue>) -> String {
    let value = set_cookie.and_then(|v| v.to_str().ok()).unwrap_or_default();
    value.split(';').next().unwrap_or_default().to_string()
}

fn cookie_name(pair: &str) -> Option<&str> {
    let name = pair.trim().split('=').next()?;
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

fn is_clearing_set_cookie(header_value: &str) -> bool {
    let lower = header_value.to_ascii_lowercase();
    if lower.contains("max-age=0") {
        return true;
    }
    let pair = header_value.split(';').next().unwrap_or_default().trim();
    pair.split_once('=')
        .is_some_and(|(_, value)| value.is_empty())
}

pub fn merge_cookies(existing: &str, set_cookie: Option<&header::HeaderValue>) -> String {
    let raw = set_cookie.and_then(|v| v.to_str().ok()).unwrap_or_default();
    if raw.is_empty() || is_clearing_set_cookie(raw) {
        return existing.to_string();
    }
    let new_cookie = cookie_header(set_cookie);
    if new_cookie.is_empty() {
        return existing.to_string();
    }
    let new_name = cookie_name(&new_cookie);
    let kept: Vec<&str> = existing
        .split(';')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .filter(|part| cookie_name(part) != new_name)
        .collect();
    if kept.is_empty() {
        new_cookie
    } else {
        format!("{}; {new_cookie}", kept.join("; "))
    }
}

pub fn merge_cookies_from_headers(existing: &str, headers: &HeaderMap) -> String {
    let mut cookies = existing.to_string();
    for value in headers.get_all(header::SET_COOKIE) {
        cookies = merge_cookies(&cookies, Some(value));
    }
    cookies
}

pub fn expect_csrf_token(path: &str, status: StatusCode, html: &str) -> String {
    extract_csrf_token(html).unwrap_or_else(|| {
        panic!(
            "csrf token missing for GET {path}: status={status}; body: {}",
            html.chars().take(300).collect::<String>()
        )
    })
}

pub fn extract_csrf_token(html: &str) -> Option<String> {
    let marker = "name=\"csrf_token\" value=\"";
    let start = html.find(marker)? + marker.len();
    let end = html[start..].find('"')? + start;
    Some(html[start..end].to_string())
}

pub async fn response_body(response: axum::response::Response) -> String {
    let bytes = response_bytes(response).await;
    String::from_utf8_lossy(&bytes).into_owned()
}

pub async fn response_bytes(response: axum::response::Response) -> Vec<u8> {
    axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body")
        .to_vec()
}

pub async fn get_bytes(
    app: &mut Router,
    path: &str,
    cookies: &str,
) -> (StatusCode, Vec<u8>, HeaderMap) {
    let mut builder = Request::builder().method("GET").uri(path);
    if !cookies.is_empty() {
        builder = builder.header(header::COOKIE, cookies);
    }
    let response = app
        .oneshot(builder.body(Body::empty()).unwrap())
        .await
        .expect("request");
    let status = response.status();
    let headers = response.headers().clone();
    let body = response_bytes(response).await;
    (status, body, headers)
}

pub async fn get(app: &mut Router, path: &str, cookies: &str) -> (StatusCode, String, String) {
    let (status, body, set_cookie, _) = get_with_headers(app, path, cookies).await;
    (status, body, set_cookie)
}

pub async fn get_with_headers(
    app: &mut Router,
    path: &str,
    cookies: &str,
) -> (StatusCode, String, String, HeaderMap) {
    get_with_extra_headers(app, path, cookies, &[]).await
}

pub async fn get_with_extra_headers(
    app: &mut Router,
    path: &str,
    cookies: &str,
    extra_headers: &[(&str, &str)],
) -> (StatusCode, String, String, HeaderMap) {
    let mut builder = Request::builder().method("GET").uri(path);
    for (name, value) in extra_headers {
        builder = builder.header(*name, *value);
    }
    if !cookies.is_empty() {
        builder = builder.header(header::COOKIE, cookies);
    }
    let response = app
        .oneshot(builder.body(Body::empty()).unwrap())
        .await
        .expect("request");
    let status = response.status();
    let headers = response.headers().clone();
    let set_cookie = merge_cookies_from_headers(cookies, &headers);
    let body = response_body(response).await;
    (status, body, set_cookie, headers)
}

pub async fn login_as(app: &mut Router, code: &str, pin: &str) -> String {
    let (status, login_html, cookies) = get(app, "/login", "").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "GET /login failed: {status}; body: {}",
        login_html.chars().take(200).collect::<String>()
    );
    let csrf = expect_csrf_token("/login", status, &login_html);
    let body = format!(
        "employee_code={}&pin={}&csrf_token={csrf}",
        url_encode(code),
        url_encode(pin)
    );
    let (status, response, cookies, headers) =
        post_form_with_headers(app, "/login", &cookies, &body).await;
    let location = header_value(&headers, "location").unwrap_or_default();
    assert_eq!(
        status,
        StatusCode::SEE_OTHER,
        "login failed for {code}: got {status}; location={location}; body: {}",
        response.chars().take(200).collect::<String>()
    );
    cookies
}

pub async fn post_form(
    app: &mut Router,
    path: &str,
    cookies: &str,
    body: &str,
) -> (StatusCode, String, String) {
    let (status, response, cookies, _) = post_form_with_headers(app, path, cookies, body).await;
    (status, response, cookies)
}

pub async fn post_form_with_headers(
    app: &mut Router,
    path: &str,
    cookies: &str,
    body: &str,
) -> (StatusCode, String, String, HeaderMap) {
    post_with_body_and_headers(
        app,
        path,
        cookies,
        "application/x-www-form-urlencoded",
        body.as_bytes(),
        &[],
    )
    .await
}

pub async fn post_with_body(
    app: &mut Router,
    path: &str,
    cookies: &str,
    content_type: &str,
    body: &[u8],
) -> (StatusCode, String, String) {
    let (status, response, cookies, _) =
        post_with_body_and_headers(app, path, cookies, content_type, body, &[]).await;
    (status, response, cookies)
}

pub async fn post_with_body_and_headers(
    app: &mut Router,
    path: &str,
    cookies: &str,
    content_type: &str,
    body: &[u8],
    extra_headers: &[(&str, &str)],
) -> (StatusCode, String, String, HeaderMap) {
    let mut builder = Request::builder()
        .method("POST")
        .uri(path)
        .header(header::CONTENT_TYPE, content_type);
    for (name, value) in extra_headers {
        builder = builder.header(*name, *value);
    }
    if !cookies.is_empty() {
        builder = builder.header(header::COOKIE, cookies);
    }
    let response = app
        .oneshot(builder.body(Body::from(body.to_vec())).unwrap())
        .await
        .expect("request");
    let status = response.status();
    let headers = response.headers().clone();
    let set_cookie = merge_cookies_from_headers(cookies, &headers);
    let response_body = response_body(response).await;
    (status, response_body, set_cookie, headers)
}

pub fn build_multipart_body(
    boundary: &str,
    csrf_token: &str,
    file_field: &str,
    file_name: Option<(&str, &[u8], &str)>,
) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(b"Content-Disposition: form-data; name=\"csrf_token\"\r\n\r\n");
    body.extend_from_slice(csrf_token.as_bytes());
    body.extend_from_slice(b"\r\n");

    if let Some((name, bytes, mime)) = file_name {
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        body.extend_from_slice(
            format!(
                "Content-Disposition: form-data; name=\"{file_field}\"; filename=\"{name}\"\r\n"
            )
            .as_bytes(),
        );
        body.extend_from_slice(format!("Content-Type: {mime}\r\n\r\n").as_bytes());
        body.extend_from_slice(bytes);
        body.extend_from_slice(b"\r\n");
    }

    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    body
}

pub async fn post_multipart(
    app: &mut Router,
    path: &str,
    cookies: &str,
    boundary: &str,
    csrf_token: &str,
    file_name: Option<(&str, &[u8], &str)>,
) -> (StatusCode, String, String) {
    post_multipart_field(app, path, cookies, boundary, csrf_token, "file", file_name).await
}

pub async fn post_multipart_field(
    app: &mut Router,
    path: &str,
    cookies: &str,
    boundary: &str,
    csrf_token: &str,
    file_field: &str,
    file_name: Option<(&str, &[u8], &str)>,
) -> (StatusCode, String, String) {
    let body = build_multipart_body(boundary, csrf_token, file_field, file_name);
    let content_type = format!("multipart/form-data; boundary={boundary}");
    post_with_body(app, path, cookies, &content_type, &body).await
}

pub fn header_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
}
