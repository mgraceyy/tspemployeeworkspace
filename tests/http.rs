mod common;

use axum::http::StatusCode;
use dtr::models::UserRole;
use dtr::services::employees::create_employee;
use dtr::services::payroll_controls::close_pay_period;
use dtr::services::requirements::{create_type, list_for_employee};
use dtr::services::timezone::company_date_now;
use uuid::Uuid;

use common::{
    extract_csrf_token, get, get_bytes, get_with_headers, header_value, login_as, post_form,
    post_multipart, test_app, test_pool,
};

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

async fn cleanup_requirement_type(pool: &sqlx::PgPool, type_id: Uuid) {
    let _ = sqlx::query("DELETE FROM employee_requirements WHERE requirement_type_id = $1")
        .bind(type_id)
        .execute(pool)
        .await;
    let _ = sqlx::query("DELETE FROM requirement_types WHERE id = $1")
        .bind(type_id)
        .execute(pool)
        .await;
}

#[tokio::test]
async fn health_returns_ok_when_database_is_up() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let mut app = test_app(pool).await;
    let (status, body, _) = get(&mut app, "/health", "").await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("\"status\":\"ok\""));
    assert!(body.contains("\"database\":\"ok\""));
}

#[tokio::test]
async fn unauthenticated_home_redirects_to_login() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let mut app = test_app(pool).await;
    let (status, _, _) = get(&mut app, "/", "").await;

    assert_eq!(status, StatusCode::SEE_OTHER);
}

#[tokio::test]
async fn login_page_is_public() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let mut app = test_app(pool).await;
    let (status, body, _) = get(&mut app, "/login", "").await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Sign in with your employee code and PIN"));
}

#[tokio::test]
async fn post_without_csrf_is_rejected() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let mut app = test_app(pool).await;
    let (_, _, cookies) = get(&mut app, "/login", "").await;

    let (status, body, _) =
        post_form(&mut app, "/login", &cookies, "employee_code=ADMIN&pin=1234").await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("CSRF"));
}

#[tokio::test]
async fn employee_cannot_access_admin_pages() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let code = unique_code("HTTP");
    let employee = create_employee(
        &pool,
        &code,
        "HTTP Test User",
        TEST_PIN,
        UserRole::Employee,
        None,
    )
    .await
    .expect("create employee");

    let mut app = test_app(pool.clone()).await;
    let cookies = login_as(&mut app, &code, TEST_PIN).await;

    let (status, _, _) = get(&mut app, "/admin/employees", &cookies).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    cleanup_employee(&pool, &code).await;
    let _ = employee;
}

#[tokio::test]
async fn login_with_valid_credentials_redirects_home() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let code = unique_code("LOGN");
    let _employee = create_employee(
        &pool,
        &code,
        "Login HTTP Test",
        TEST_PIN,
        UserRole::Employee,
        None,
    )
    .await
    .expect("create employee");

    let mut app = test_app(pool.clone()).await;
    let cookies = login_as(&mut app, &code, TEST_PIN).await;

    let (status, body, _) = get(&mut app, "/", &cookies).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Clock In / Out"));

    cleanup_employee(&pool, &code).await;
}

#[tokio::test]
async fn employee_cannot_download_admin_payroll_export() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let code = unique_code("EXPR");
    create_employee(
        &pool,
        &code,
        "Export Block Test",
        TEST_PIN,
        UserRole::Employee,
        None,
    )
    .await
    .expect("create employee");

    let mut app = test_app(pool.clone()).await;
    let cookies = login_as(&mut app, &code, TEST_PIN).await;
    let (status, _, _) = get(&mut app, "/admin/reports/export.csv", &cookies).await;

    assert_eq!(status, StatusCode::FORBIDDEN);

    cleanup_employee(&pool, &code).await;
}

#[tokio::test]
async fn admin_payroll_export_returns_csv() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let code = unique_code("ADMN");
    create_employee(
        &pool,
        &code,
        "Admin Export Test",
        TEST_PIN,
        UserRole::Admin,
        None,
    )
    .await
    .expect("create admin");

    let mut app = test_app(pool.clone()).await;
    let cookies = login_as(&mut app, &code, TEST_PIN).await;
    let (status, body, _) = get(&mut app, "/admin/reports/export.csv", &cookies).await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Employee Code"));
    assert!(body.contains("Regular Hours"));

    cleanup_employee(&pool, &code).await;
}

#[tokio::test]
async fn closed_pay_period_blocks_clock_in_via_http() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let code = unique_code("CLSD");
    let employee = create_employee(
        &pool,
        &code,
        "Closed Period Test",
        TEST_PIN,
        UserRole::Employee,
        None,
    )
    .await
    .expect("create employee");

    let settings = dtr::services::settings::get_settings(&pool)
        .await
        .expect("settings");
    let today = company_date_now(&settings).expect("today");
    close_pay_period(&pool, today, today, employee.id, Some("test close"))
        .await
        .expect("close period");

    let mut app = test_app(pool.clone()).await;
    let cookies = login_as(&mut app, &code, TEST_PIN).await;
    let (_, home_html, cookies) = get(&mut app, "/", &cookies).await;
    let csrf = extract_csrf_token(&home_html).expect("csrf token");
    let (status, body, _) = post_form(
        &mut app,
        "/clock/in",
        &cookies,
        &format!("csrf_token={csrf}"),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("closed pay period"));

    let _ = sqlx::query("DELETE FROM closed_pay_periods WHERE period_start = $1")
        .bind(today)
        .execute(&pool)
        .await;
    cleanup_employee(&pool, &code).await;
}

#[tokio::test]
async fn unauthenticated_requirement_download_redirects_to_login() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let mut app = test_app(pool).await;
    let fake_id = Uuid::new_v4();
    let (status, _, _) = get(&mut app, &format!("/me/requirements/{fake_id}/file"), "").await;

    assert_eq!(status, StatusCode::SEE_OTHER);
}

#[tokio::test]
async fn manager_cannot_access_admin_pages_or_exports() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let code = unique_code("MNGR");
    create_employee(
        &pool,
        &code,
        "Manager Boundary Test",
        TEST_PIN,
        UserRole::Manager,
        None,
    )
    .await
    .expect("create manager");

    let mut app = test_app(pool.clone()).await;
    let cookies = login_as(&mut app, &code, TEST_PIN).await;

    let (employees_status, _, _) = get(&mut app, "/admin/employees", &cookies).await;
    assert_eq!(employees_status, StatusCode::FORBIDDEN);

    let (export_status, _, _) = get(&mut app, "/admin/reports/export.csv", &cookies).await;
    assert_eq!(export_status, StatusCode::FORBIDDEN);

    let (manager_status, body, _) = get(&mut app, "/manager", &cookies).await;
    assert_eq!(manager_status, StatusCode::OK);
    assert!(body.contains("Manager") || body.contains("Dashboard"));

    cleanup_employee(&pool, &code).await;
}

#[tokio::test]
async fn login_locks_account_after_repeated_failures() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let code = unique_code("LOCK");
    create_employee(
        &pool,
        &code,
        "Login Lock Test",
        TEST_PIN,
        UserRole::Employee,
        None,
    )
    .await
    .expect("create employee");

    let mut app = test_app(pool.clone()).await;
    let (_, login_html, mut cookies) = get(&mut app, "/login", "").await;
    let csrf = extract_csrf_token(&login_html).expect("csrf token");

    for _ in 0..5 {
        let body = format!("employee_code={code}&pin=000000&csrf_token={csrf}");
        let (status, response, updated_cookies) =
            post_form(&mut app, "/login", &cookies, &body).await;
        assert_eq!(status, StatusCode::OK);
        assert!(response.contains("Invalid employee code or PIN"));
        cookies = updated_cookies;
    }

    let body = format!("employee_code={code}&pin={TEST_PIN}&csrf_token={csrf}");
    let (_, response, _) = post_form(&mut app, "/login", &cookies, &body).await;
    assert!(response.contains("Too many failed attempts"));

    cleanup_employee(&pool, &code).await;
}

#[tokio::test]
async fn change_pin_rejects_weak_pin_via_http() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let code = unique_code("WPIN");
    create_employee(
        &pool,
        &code,
        "Weak PIN Test",
        TEST_PIN,
        UserRole::Employee,
        None,
    )
    .await
    .expect("create employee");

    let mut app = test_app(pool.clone()).await;
    let cookies = login_as(&mut app, &code, TEST_PIN).await;
    let (_, change_html, cookies) = get(&mut app, "/change-pin", &cookies).await;
    let csrf = extract_csrf_token(&change_html).expect("csrf token");
    let body = format!("current_pin={TEST_PIN}&new_pin=1234&confirm_pin=1234&csrf_token={csrf}");
    let (status, response, _) = post_form(&mut app, "/change-pin", &cookies, &body).await;

    assert_eq!(status, StatusCode::OK);
    assert!(response.contains("too easy to guess"));

    cleanup_employee(&pool, &code).await;
}

#[tokio::test]
async fn html_responses_include_security_headers() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let mut app = test_app(pool).await;
    let (_, _, _, headers) = get_with_headers(&mut app, "/login", "").await;

    let csp = header_value(&headers, "content-security-policy").unwrap_or_default();
    assert!(csp.contains("default-src 'self'"));
    assert_eq!(
        header_value(&headers, "x-content-type-options").as_deref(),
        Some("nosniff")
    );
    assert_eq!(
        header_value(&headers, "x-frame-options").as_deref(),
        Some("DENY")
    );
    assert_eq!(
        header_value(&headers, "referrer-policy").as_deref(),
        Some("strict-origin-when-cross-origin")
    );
    assert!(header_value(&headers, "permissions-policy")
        .unwrap_or_default()
        .contains("camera=()"));
}

#[tokio::test]
async fn post_rate_limit_blocks_excessive_requests() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let mut app = test_app(pool).await;
    let (_, _, cookies, _) = get_with_headers(&mut app, "/login", "").await;

    let mut last_status = StatusCode::OK;
    for i in 0..121 {
        let (status, _, _) = post_form(&mut app, "/login", &cookies, "employee_code=X&pin=1").await;
        last_status = status;
        if i < 120 {
            assert_ne!(status, StatusCode::TOO_MANY_REQUESTS);
        }
    }

    assert_eq!(last_status, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn requirement_upload_rejects_mismatched_content_via_http() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let code = unique_code("UPLD");
    let employee = create_employee(
        &pool,
        &code,
        "Upload HTTP Test",
        TEST_PIN,
        UserRole::Employee,
        None,
    )
    .await
    .expect("create employee");

    let req_type = create_type(
        &pool,
        "HTTP Upload Doc",
        "Integration upload test",
        true,
        false,
        1,
        None,
    )
    .await
    .expect("create requirement type");

    let reqs = list_for_employee(&pool, employee.id)
        .await
        .expect("list requirements");
    let row = reqs
        .iter()
        .find(|r| r.requirement_type_id == req_type.id)
        .expect("seeded requirement");

    let mut app = test_app(pool.clone()).await;
    let cookies = login_as(&mut app, &code, TEST_PIN).await;
    let (_, requirements_html, cookies, _) =
        get_with_headers(&mut app, "/me/requirements", &cookies).await;
    let csrf = extract_csrf_token(&requirements_html).expect("csrf token");

    let path = format!("/me/requirements/{}/submit", row.id);
    let (status, body, _) = post_multipart(
        &mut app,
        &path,
        &cookies,
        "testboundary",
        &csrf,
        Some(("fake.pdf", b"NOTPDF", "application/pdf")),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("does not match") || body.contains("Unrecognized"));

    cleanup_requirement_type(&pool, req_type.id).await;
    cleanup_employee(&pool, &code).await;
    let _ = employee;
}

#[tokio::test]
async fn login_locks_ip_after_repeated_failures() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let mut app = test_app(pool).await;
    let (_, login_html, mut cookies) = get(&mut app, "/login", "").await;
    let csrf = extract_csrf_token(&login_html).expect("csrf token");

    for i in 0..20 {
        let code = format!("FAKE{i:02}");
        let body = format!("employee_code={code}&pin=000000&csrf_token={csrf}");
        let (status, response, updated_cookies) =
            post_form(&mut app, "/login", &cookies, &body).await;
        assert_eq!(status, StatusCode::OK);
        assert!(response.contains("Invalid employee code or PIN"));
        cookies = updated_cookies;
    }

    let body = format!("employee_code=FAKE99&pin=000000&csrf_token={csrf}");
    let (_, response, _) = post_form(&mut app, "/login", &cookies, &body).await;
    assert!(response.contains("Too many login attempts from this address"));
}

#[tokio::test]
async fn admin_payroll_export_returns_xlsx() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let code = unique_code("XLSX");
    create_employee(
        &pool,
        &code,
        "XLSX Export Test",
        TEST_PIN,
        UserRole::Admin,
        None,
    )
    .await
    .expect("create admin");

    let mut app = test_app(pool.clone()).await;
    let cookies = login_as(&mut app, &code, TEST_PIN).await;
    let (status, body, headers) = get_bytes(&mut app, "/admin/reports/export.xlsx", &cookies).await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.starts_with(b"PK"));
    let content_type = header_value(&headers, "content-type").unwrap_or_default();
    assert!(
        content_type.contains("spreadsheetml") || content_type.contains("octet-stream"),
        "unexpected content type: {content_type}"
    );

    cleanup_employee(&pool, &code).await;
}

#[tokio::test]
async fn requirement_upload_accepts_valid_pdf_via_http() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let code = unique_code("PDFU");
    let employee = create_employee(
        &pool,
        &code,
        "PDF Upload Test",
        TEST_PIN,
        UserRole::Employee,
        None,
    )
    .await
    .expect("create employee");

    let req_type = create_type(
        &pool,
        "HTTP PDF Doc",
        "PDF upload test",
        true,
        false,
        1,
        None,
    )
    .await
    .expect("create requirement type");

    let reqs = list_for_employee(&pool, employee.id)
        .await
        .expect("list requirements");
    let row = reqs
        .iter()
        .find(|r| r.requirement_type_id == req_type.id)
        .expect("seeded requirement");

    let mut app = test_app(pool.clone()).await;
    let cookies = login_as(&mut app, &code, TEST_PIN).await;
    let (_, requirements_html, cookies, _) =
        get_with_headers(&mut app, "/me/requirements", &cookies).await;
    let csrf = extract_csrf_token(&requirements_html).expect("csrf token");

    let path = format!("/me/requirements/{}/submit", row.id);
    let (status, _, _) = post_multipart(
        &mut app,
        &path,
        &cookies,
        "pdfboundary",
        &csrf,
        Some(("id.pdf", b"%PDF-1.4 minimal", "application/pdf")),
    )
    .await;

    assert_eq!(status, StatusCode::SEE_OTHER);

    cleanup_requirement_type(&pool, req_type.id).await;
    cleanup_employee(&pool, &code).await;
    let _ = employee;
}

#[tokio::test]
async fn requirement_upload_rejects_generic_zip_as_docx_via_http() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let code = unique_code("DOCX");
    let employee = create_employee(
        &pool,
        &code,
        "DOCX Upload Test",
        TEST_PIN,
        UserRole::Employee,
        None,
    )
    .await
    .expect("create employee");

    let req_type = create_type(
        &pool,
        "HTTP DOCX Doc",
        "DOCX upload test",
        true,
        false,
        1,
        None,
    )
    .await
    .expect("create requirement type");

    let reqs = list_for_employee(&pool, employee.id)
        .await
        .expect("list requirements");
    let row = reqs
        .iter()
        .find(|r| r.requirement_type_id == req_type.id)
        .expect("seeded requirement");

    let mut app = test_app(pool.clone()).await;
    let cookies = login_as(&mut app, &code, TEST_PIN).await;
    let (_, requirements_html, cookies, _) =
        get_with_headers(&mut app, "/me/requirements", &cookies).await;
    let csrf = extract_csrf_token(&requirements_html).expect("csrf token");

    let path = format!("/me/requirements/{}/submit", row.id);
    let fake_docx = b"PK\x03\x04\x00\x00generic zip without word document path";
    let (status, body, _) = post_multipart(
        &mut app,
        &path,
        &cookies,
        "docxboundary",
        &csrf,
        Some((
            "fake.docx",
            fake_docx,
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        )),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("does not match") || body.contains("Unrecognized"));

    cleanup_requirement_type(&pool, req_type.id).await;
    cleanup_employee(&pool, &code).await;
    let _ = employee;
}
