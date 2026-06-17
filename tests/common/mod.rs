#![allow(dead_code)]

use std::sync::Arc;

use axum::{
    body::Body,
    http::{header, HeaderMap, Request, StatusCode},
    Router,
};
use dtr::app::create_app;
use dtr::auth::login_limiter::LoginLimiter;
use dtr::auth::post_limiter::PostRateLimiter;
use dtr::metrics::AppMetrics;
use dtr::state::AppState;
use dtr::templates::engine;
use sqlx::PgPool;
use tower::ServiceExt;
use tower_sessions::{
    cookie::{Key, SameSite},
    Expiry, SessionManagerLayer,
};
use tower_sessions_sqlx_store::PostgresStore;

const TEST_SESSION_SECRET: &[u8] =
    b"test-session-secret-at-least-64-characters-long-for-signed-cookies";

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
            shared_rate_limits: false,
            trust_proxy_headers: false,
            max_upload_bytes: 5 * 1024 * 1024,
            metrics_token: None,
        }
    }
}

pub async fn test_pool() -> Option<PgPool> {
    dotenvy::dotenv().ok();
    let url = std::env::var("DATABASE_URL").ok()?;
    let pool = dtr::db::connect_with_options(&url, 10).await.ok()?;
    dtr::db::migrate(&pool).await.ok()?;
    Some(pool)
}

pub async fn test_app(pool: PgPool) -> Router {
    test_app_with_config(pool, TestAppConfig::default()).await
}

pub async fn test_app_with_config(pool: PgPool, config: TestAppConfig) -> Router {
    let session_store = PostgresStore::new(pool.clone());
    session_store.migrate().await.expect("session migrate");

    let session_key = Key::try_from(TEST_SESSION_SECRET).expect("session key");
    let session_layer = SessionManagerLayer::new(session_store)
        .with_secure(false)
        .with_same_site(SameSite::Lax)
        .with_expiry(Expiry::OnInactivity(time::Duration::hours(8)))
        .with_signed(session_key);

    let (login_limiter, post_limiter) = if config.shared_rate_limits {
        let limit_pool = pool.clone();
        (
            LoginLimiter::postgres(limit_pool.clone()),
            PostRateLimiter::postgres(limit_pool),
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

pub fn merge_cookies(existing: &str, set_cookie: Option<&header::HeaderValue>) -> String {
    let new_cookie = cookie_header(set_cookie);
    if existing.is_empty() {
        new_cookie
    } else if new_cookie.is_empty() {
        existing.to_string()
    } else {
        format!("{existing}; {new_cookie}")
    }
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
    let set_cookie = merge_cookies(cookies, response.headers().get(header::SET_COOKIE));
    let body = response_body(response).await;
    (status, body, set_cookie, headers)
}

pub async fn login_as(app: &mut Router, code: &str, pin: &str) -> String {
    let (_, login_html, cookies) = get(app, "/login", "").await;
    let csrf = extract_csrf_token(&login_html).expect("csrf token");
    let body = format!("employee_code={code}&pin={pin}&csrf_token={csrf}");
    let (status, _, cookies) = post_form(app, "/login", &cookies, &body).await;
    assert_eq!(status, StatusCode::SEE_OTHER, "login failed");
    cookies
}

pub async fn post_form(
    app: &mut Router,
    path: &str,
    cookies: &str,
    body: &str,
) -> (StatusCode, String, String) {
    post_with_body(
        app,
        path,
        cookies,
        "application/x-www-form-urlencoded",
        body.as_bytes(),
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
    post_with_body_and_headers(app, path, cookies, content_type, body, &[]).await
}

pub async fn post_with_body_and_headers(
    app: &mut Router,
    path: &str,
    cookies: &str,
    content_type: &str,
    body: &[u8],
    extra_headers: &[(&str, &str)],
) -> (StatusCode, String, String) {
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
    let set_cookie = merge_cookies(cookies, response.headers().get(header::SET_COOKIE));
    let response_body = response_body(response).await;
    (status, response_body, set_cookie)
}

pub fn build_multipart_body(
    boundary: &str,
    csrf_token: &str,
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
            format!("Content-Disposition: form-data; name=\"file\"; filename=\"{name}\"\r\n")
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
    let body = build_multipart_body(boundary, csrf_token, file_name);
    let content_type = format!("multipart/form-data; boundary={boundary}");
    post_with_body(app, path, cookies, &content_type, &body).await
}

pub fn header_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
}
