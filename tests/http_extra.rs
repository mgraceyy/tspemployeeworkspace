mod common;

use axum::http::StatusCode;
use dtr::models::UserRole;
use dtr::services::employees::{reset_employee_pin, set_employee_active, update_employee};
use uuid::Uuid;

use common::{
    create_ready_employee, extract_csrf_token, get, get_bytes, get_with_extra_headers,
    get_with_headers, header_value, login_as, post_form, post_with_body_and_headers, test_app,
    test_app_with_config, test_pool, TestAppConfig,
};
use dtr::services::clock::clock_in;
use dtr::services::settings::get_settings;
use dtr::services::timezone::{company_date_now, now_company};

const TEST_PIN: &str = "482915";

fn unique_code(prefix: &str) -> String {
    format!("{prefix}{}", &Uuid::new_v4().simple().to_string()[..8])
}

async fn cleanup_employee(pool: &sqlx::PgPool, code: &str) {
    let _ = sqlx::query("DELETE FROM employees WHERE employee_code = $1")
        .bind(code)
        .execute(pool)
        .await;
}

#[tokio::test]
async fn deactivated_employee_session_is_cleared_on_next_request() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let code = unique_code("DEAC");
    let employee = create_ready_employee(
        &pool,
        &code,
        "Deactivate Test",
        TEST_PIN,
        UserRole::Employee,
        None,
    )
    .await
    .expect("create employee");

    let mut app = test_app(pool.clone()).await;
    let cookies = login_as(&mut app, &code, TEST_PIN).await;

    set_employee_active(&pool, employee.id, false)
        .await
        .expect("deactivate");

    let (status, _, _) = get(&mut app, "/", &cookies).await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    cleanup_employee(&pool, &code).await;
}

#[tokio::test]
async fn reset_pin_forces_change_pin_on_next_request() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let code = unique_code("RSPN");
    let employee = create_ready_employee(
        &pool,
        &code,
        "Reset PIN Test",
        TEST_PIN,
        UserRole::Employee,
        None,
    )
    .await
    .expect("create employee");

    let mut app = test_app(pool.clone()).await;
    let cookies = login_as(&mut app, &code, TEST_PIN).await;

    reset_employee_pin(&pool, employee.id, "593847")
        .await
        .expect("reset pin");

    let (status, _, _) = get(&mut app, "/", &cookies).await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    cleanup_employee(&pool, &code).await;
}

#[tokio::test]
async fn demoted_admin_loses_admin_access_immediately() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let code = unique_code("DEMT");
    let admin = create_ready_employee(&pool, &code, "Demote Test", TEST_PIN, UserRole::Admin, None)
        .await
        .expect("create admin");

    let mut app = test_app(pool.clone()).await;
    let cookies = login_as(&mut app, &code, TEST_PIN).await;

    update_employee(
        &pool,
        admin.id,
        &code,
        "Demote Test",
        UserRole::Employee,
        None,
    )
    .await
    .expect("demote");

    let (status, _, _) = get(&mut app, "/admin/reports", &cookies).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    cleanup_employee(&pool, &code).await;
}

#[tokio::test]
async fn manager_cannot_view_other_managers_team_member() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let mgr_a = unique_code("MGRA");
    let mgr_b = unique_code("MGRB");
    let emp = unique_code("EMPA");
    let manager_a = create_ready_employee(
        &pool,
        &mgr_a,
        "Manager A",
        TEST_PIN,
        UserRole::Manager,
        None,
    )
    .await
    .expect("manager a");
    create_ready_employee(
        &pool,
        &mgr_b,
        "Manager B",
        TEST_PIN,
        UserRole::Manager,
        None,
    )
    .await
    .expect("manager b");
    let employee = create_ready_employee(
        &pool,
        &emp,
        "Team Member",
        TEST_PIN,
        UserRole::Employee,
        Some(manager_a.id),
    )
    .await
    .expect("employee");

    let mut app = test_app(pool.clone()).await;
    let cookies = login_as(&mut app, &mgr_b, TEST_PIN).await;
    let path = format!("/manager/team/{}", employee.id);
    let (status, _, _) = get(&mut app, &path, &cookies).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    cleanup_employee(&pool, &emp).await;
    cleanup_employee(&pool, &mgr_a).await;
    cleanup_employee(&pool, &mgr_b).await;
}

#[tokio::test]
async fn health_returns_service_unavailable_when_database_is_closed() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let mut app = test_app(pool.clone()).await;
    pool.close().await;

    let (status, body, _) = get(&mut app, "/health", "").await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert!(body.contains("degraded"));
}

#[tokio::test]
async fn shared_postgres_rate_limits_apply_across_app_instances() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let _ = sqlx::query("DELETE FROM rate_limit_events")
        .execute(&pool)
        .await;

    let config = TestAppConfig {
        shared_rate_limits: true,
        ..TestAppConfig::default()
    };
    let mut app_a = test_app_with_config(pool.clone(), config.clone()).await;
    let mut app_b = test_app_with_config(pool, config).await;

    let (_, login_html, mut cookies) = get(&mut app_a, "/login", "").await;
    let csrf = extract_csrf_token(&login_html).expect("csrf token");

    for i in 0..20 {
        let code = format!("FAKE{i:02}");
        let body = format!("employee_code={code}&pin=000000&csrf_token={csrf}");
        let (status, response, updated_cookies) =
            post_form(&mut app_a, "/login", &cookies, &body).await;
        assert_eq!(status, StatusCode::OK);
        assert!(response.contains("Invalid employee code or PIN"));
        cookies = updated_cookies;
    }

    let (_, login_html_b, cookies_b) = get(&mut app_b, "/login", "").await;
    let csrf_b = extract_csrf_token(&login_html_b).expect("csrf token");
    let body = format!("employee_code=FAKE99&pin=000000&csrf_token={csrf_b}");
    let (_, response, _) = post_form(&mut app_b, "/login", &cookies_b, &body).await;
    assert!(response.contains("Too many login attempts from this address"));
}

#[tokio::test]
async fn employee_timesheet_export_returns_csv() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let code = unique_code("TSCV");
    create_ready_employee(
        &pool,
        &code,
        "Timesheet Export",
        TEST_PIN,
        UserRole::Employee,
        None,
    )
    .await
    .expect("create employee");

    let mut app = test_app(pool.clone()).await;
    let cookies = login_as(&mut app, &code, TEST_PIN).await;
    let (status, body, headers) = get_bytes(&mut app, "/me/timesheet/export.csv", &cookies).await;

    assert_eq!(status, StatusCode::OK);
    let content_type = header_value(&headers, "content-type").unwrap_or_default();
    assert!(content_type.contains("text/csv") || content_type.contains("octet-stream"));
    let text = String::from_utf8_lossy(&body);
    assert!(text.contains("work_date") || text.contains("Work Date") || !text.is_empty());

    cleanup_employee(&pool, &code).await;
}

#[tokio::test]
async fn admin_detail_export_returns_csv() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let code = unique_code("DTLC");
    create_ready_employee(
        &pool,
        &code,
        "Detail Export",
        TEST_PIN,
        UserRole::Admin,
        None,
    )
    .await
    .expect("create admin");

    let mut app = test_app(pool.clone()).await;
    let cookies = login_as(&mut app, &code, TEST_PIN).await;
    let (status, body, headers) =
        get_bytes(&mut app, "/admin/reports/export-detail.csv", &cookies).await;

    assert_eq!(status, StatusCode::OK);
    let content_type = header_value(&headers, "content-type").unwrap_or_default();
    assert!(content_type.contains("text/csv") || content_type.contains("octet-stream"));
    assert!(!body.is_empty());

    cleanup_employee(&pool, &code).await;
}

#[tokio::test]
async fn manager_eod_export_returns_csv() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let code = unique_code("EODX");
    create_ready_employee(
        &pool,
        &code,
        "EOD Export Manager",
        TEST_PIN,
        UserRole::Manager,
        None,
    )
    .await
    .expect("create manager");

    let mut app = test_app(pool.clone()).await;
    let cookies = login_as(&mut app, &code, TEST_PIN).await;
    let (status, _body, headers) = get_bytes(&mut app, "/manager/eod/export.csv", &cookies).await;

    assert_eq!(status, StatusCode::OK);
    let content_type = header_value(&headers, "content-type").unwrap_or_default();
    assert!(content_type.contains("text/csv") || content_type.contains("octet-stream"));

    cleanup_employee(&pool, &code).await;
}

#[tokio::test]
async fn static_assets_include_cache_control() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let mut app = test_app(pool).await;
    let (status, _, headers) = get_bytes(&mut app, "/static/style.css", "").await;
    assert_eq!(status, StatusCode::OK);
    let cache = header_value(&headers, "cache-control").unwrap_or_default();
    assert!(cache.contains("max-age"));
}

#[tokio::test]
async fn employee_leave_page_is_accessible() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let code = unique_code("LEAV");
    create_ready_employee(
        &pool,
        &code,
        "Leave Page",
        TEST_PIN,
        UserRole::Employee,
        None,
    )
    .await
    .expect("create employee");

    let mut app = test_app(pool.clone()).await;
    let cookies = login_as(&mut app, &code, TEST_PIN).await;
    let (status, body, _) = get(&mut app, "/me/leave", &cookies).await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Leave") || body.contains("leave"));

    cleanup_employee(&pool, &code).await;
}

#[tokio::test]
async fn oversize_upload_is_rejected() {
    use dtr::services::requirements::{create_type, list_for_employee};

    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let config = TestAppConfig {
        max_upload_bytes: 512,
        ..TestAppConfig::default()
    };
    let code = unique_code("BIGU");
    let employee = create_ready_employee(
        &pool,
        &code,
        "Big Upload",
        TEST_PIN,
        UserRole::Employee,
        None,
    )
    .await
    .expect("create employee");

    let req_type = create_type(
        &pool,
        "Oversize Test Doc",
        "upload limit test",
        true,
        true,
        1,
        None,
    )
    .await
    .expect("create type");
    let reqs = list_for_employee(&pool, employee.id)
        .await
        .expect("list requirements");
    let row = reqs
        .iter()
        .find(|r| r.requirement_type_id == req_type.id)
        .expect("seeded requirement");

    let mut app = test_app_with_config(pool.clone(), config).await;
    let cookies = login_as(&mut app, &code, TEST_PIN).await;
    let (_, requirements_html, cookies, _) =
        get_with_headers(&mut app, "/me/requirements", &cookies).await;
    let csrf = extract_csrf_token(&requirements_html).expect("csrf");

    let huge = vec![b'%'; 2048];
    let pdf_header = b"%PDF-1.4 oversize test content";
    let mut file_bytes = pdf_header.to_vec();
    file_bytes.extend(huge);
    let path = format!("/me/requirements/{}/submit", row.id);
    let (status, response, _) = common::post_multipart(
        &mut app,
        &path,
        &cookies,
        "bigboundary",
        &csrf,
        Some(("big.pdf", &file_bytes, "application/pdf")),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(response.contains("too large") || response.contains("large"));

    let _ = sqlx::query("DELETE FROM requirement_types WHERE id = $1")
        .bind(req_type.id)
        .execute(&pool)
        .await;
    cleanup_employee(&pool, &code).await;
}

fn metrics_request_count(body: &str) -> u64 {
    body.lines()
        .find(|line| line.starts_with("dtr_http_requests_total "))
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|value| value.parse().ok())
        .unwrap_or(0)
}

#[tokio::test]
async fn metrics_endpoint_counts_all_routes() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let mut app = test_app(pool).await;
    let _ = get(&mut app, "/login", "").await;
    let _ = get(&mut app, "/health", "").await;
    let _ = get_bytes(&mut app, "/static/style.css", "").await;
    let (status, body, _) = get(&mut app, "/metrics", "").await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("dtr_http_requests_total"));
    assert!(body.contains("dtr_db_pool_connections"));
    assert!(body.contains("dtr_payroll_runs_created_total"));
    assert!(
        metrics_request_count(&body) >= 4,
        "expected health/static/metrics to be counted: {body}"
    );
}

#[tokio::test]
async fn metrics_requires_token_when_configured() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let config = TestAppConfig {
        metrics_token: Some("secret-metrics-token".into()),
        ..TestAppConfig::default()
    };
    let mut app = test_app_with_config(pool, config).await;

    let (status, _, _) = get(&mut app, "/metrics", "").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, body, _, _) = get_with_extra_headers(
        &mut app,
        "/metrics",
        "",
        &[("Authorization", "Bearer secret-metrics-token")],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("dtr_http_requests_total"));

    let (status, body, _) = get(&mut app, "/metrics?token=secret-metrics-token", "").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("dtr_http_requests_total"));
}

#[tokio::test]
async fn notification_dismiss_via_http() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let code = unique_code("NTFD");
    let employee = create_ready_employee(
        &pool,
        &code,
        "Notify Dismiss",
        TEST_PIN,
        UserRole::Employee,
        None,
    )
    .await
    .expect("create employee");

    clock_in(&pool, employee.id).await.expect("clock in");
    let settings = get_settings(&pool).await.expect("settings");
    let today = company_date_now(&settings).expect("today");

    let mut app = test_app(pool.clone()).await;
    let cookies = login_as(&mut app, &code, TEST_PIN).await;
    let (_, notifications_html, cookies) = get(&mut app, "/notifications", &cookies).await;
    assert!(notifications_html.contains("EOD") || notifications_html.contains("eod"));

    let csrf = extract_csrf_token(&notifications_html).expect("csrf");
    let key = format!("missing_eod:{today}");
    let body = format!("key={key}&csrf_token={csrf}");
    let (status, _, _) = post_form(&mut app, "/notifications/dismiss", &cookies, &body).await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    cleanup_employee(&pool, &code).await;
}

#[tokio::test]
async fn admin_can_save_report_preset_via_http() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let code = unique_code("PRST");
    create_ready_employee(
        &pool,
        &code,
        "Preset Admin",
        TEST_PIN,
        UserRole::Admin,
        None,
    )
    .await
    .expect("create admin");

    let preset_name = format!("Preset {}", &Uuid::new_v4().simple().to_string()[..6]);
    let mut app = test_app(pool.clone()).await;
    let cookies = login_as(&mut app, &code, TEST_PIN).await;
    let (_, reports_html, cookies) = get(&mut app, "/admin/reports", &cookies).await;
    let csrf = extract_csrf_token(&reports_html).expect("csrf");
    let body = format!("preset_name={preset_name}&csrf_token={csrf}");
    let (status, _, _) = post_form(&mut app, "/admin/reports/presets", &cookies, &body).await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM report_presets WHERE name = $1)")
            .bind(&preset_name)
            .fetch_one(&pool)
            .await
            .expect("check preset");
    assert!(exists);

    let _ = sqlx::query("DELETE FROM report_presets WHERE name = $1")
        .bind(&preset_name)
        .execute(&pool)
        .await;
    cleanup_employee(&pool, &code).await;
}

#[tokio::test]
async fn admin_shifts_page_is_accessible() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let admin_code = unique_code("SHAD");
    let emp_code = unique_code("SHEM");
    create_ready_employee(
        &pool,
        &admin_code,
        "Shift Admin",
        TEST_PIN,
        UserRole::Admin,
        None,
    )
    .await
    .expect("create admin");
    let employee = create_ready_employee(
        &pool,
        &emp_code,
        "Shift Employee",
        TEST_PIN,
        UserRole::Employee,
        None,
    )
    .await
    .expect("create employee");

    let mut app = test_app(pool.clone()).await;
    let cookies = login_as(&mut app, &admin_code, TEST_PIN).await;
    let path = format!("/admin/shifts/{}", employee.id);
    let (status, body, _) = get(&mut app, &path, &cookies).await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Shift") || body.contains("shift"));

    cleanup_employee(&pool, &emp_code).await;
    cleanup_employee(&pool, &admin_code).await;
}

#[tokio::test]
async fn trust_proxy_headers_isolate_rate_limits_by_forwarded_ip() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let config = TestAppConfig {
        trust_proxy_headers: true,
        ..TestAppConfig::default()
    };
    let mut app = test_app_with_config(pool, config).await;
    let (_, login_html, mut cookies) = get(&mut app, "/login", "").await;
    let csrf = extract_csrf_token(&login_html).expect("csrf");

    for i in 0..20 {
        let code = format!("FAKE{i:02}");
        let body = format!("employee_code={code}&pin=000000&csrf_token={csrf}");
        let (status, response, updated_cookies) = post_with_body_and_headers(
            &mut app,
            "/login",
            &cookies,
            "application/x-www-form-urlencoded",
            body.as_bytes(),
            &[("X-Forwarded-For", "203.0.113.77")],
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(response.contains("Invalid employee code or PIN"));
        cookies = updated_cookies;
    }

    let body = format!("employee_code=FAKE99&pin=000000&csrf_token={csrf}");
    let (status, response, _) = post_with_body_and_headers(
        &mut app,
        "/login",
        "",
        "application/x-www-form-urlencoded",
        body.as_bytes(),
        &[("X-Forwarded-For", "203.0.113.88")],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(response.contains("Invalid employee code or PIN"));
    assert!(!response.contains("Too many login attempts"));
}

#[tokio::test]
async fn clock_out_requires_ot_reason_via_http() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let code = unique_code("CLKO");
    let employee = create_ready_employee(
        &pool,
        &code,
        "Clock Out HTTP",
        TEST_PIN,
        UserRole::Employee,
        None,
    )
    .await
    .expect("create employee");

    let settings = get_settings(&pool).await.expect("settings");
    let today = company_date_now(&settings).expect("today");
    let clock_in_time = now_company(&settings).expect("now") - time::Duration::hours(10);

    sqlx::query(
        "INSERT INTO time_entries (employee_id, work_date, clock_in, attendance)
         VALUES ($1, $2, $3, 'on_time')",
    )
    .bind(employee.id)
    .bind(today)
    .bind(clock_in_time)
    .execute(&pool)
    .await
    .expect("insert entry");

    let mut app = test_app(pool.clone()).await;
    let cookies = login_as(&mut app, &code, TEST_PIN).await;
    let (_, home_html, cookies) = get(&mut app, "/", &cookies).await;
    let csrf = extract_csrf_token(&home_html).expect("csrf");
    let body = format!("csrf_token={csrf}");
    let (status, _, cookies) = post_form(&mut app, "/clock/out", &cookies, &body).await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let (_, home_after, _) = get(&mut app, "/", &cookies).await;
    assert!(home_after.contains("reason for overtime") || home_after.contains("overtime"));

    cleanup_employee(&pool, &code).await;
}
