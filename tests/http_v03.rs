mod common;

use axum::http::StatusCode;
use dtr::models::UserRole;
use dtr::services::compensation::UpsertProfileInput;
use dtr::services::employees::create_employee;
use dtr::services::payroll_controls::close_pay_period;
use dtr::services::reports::current_pay_period;
use dtr::services::settings::get_settings;
use dtr::services::timezone::format_date;
use time::{Date, Month};
use uuid::Uuid;

use common::{
    extract_csrf_token, get, get_bytes, login_as, post_form, post_multipart_field, test_app,
    test_pool,
};

const TEST_PIN: &str = "482915";

const MINI_PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
    0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f,
    0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0a, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x00,
    0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0d, 0x0a, 0x2d, 0xb4, 0x00, 0x00, 0x00, 0x00, 0x49,
    0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

fn unique_code(prefix: &str) -> String {
    format!("{prefix}{}", &Uuid::new_v4().simple().to_string()[..8])
}

async fn cleanup_employee(pool: &sqlx::PgPool, code: &str) {
    let _ = sqlx::query("DELETE FROM payroll_lines WHERE run_id IN (SELECT id FROM payroll_runs WHERE created_by IN (SELECT id FROM employees WHERE employee_code = $1))")
        .bind(code)
        .execute(pool)
        .await;
    let _ = sqlx::query("DELETE FROM payroll_runs WHERE created_by IN (SELECT id FROM employees WHERE employee_code = $1)")
        .bind(code)
        .execute(pool)
        .await;
    let _ = sqlx::query("DELETE FROM employees WHERE employee_code = $1")
        .bind(code)
        .execute(pool)
        .await;
}

async fn cleanup_payroll_period(pool: &sqlx::PgPool, period_start: Date, period_end: Date) {
    let _ = sqlx::query("DELETE FROM payroll_lines WHERE run_id IN (SELECT id FROM payroll_runs WHERE period_start = $1 AND period_end = $2)")
        .bind(period_start)
        .bind(period_end)
        .execute(pool)
        .await;
    let _ = sqlx::query("DELETE FROM payroll_runs WHERE period_start = $1 AND period_end = $2")
        .bind(period_start)
        .bind(period_end)
        .execute(pool)
        .await;
    let _ =
        sqlx::query("DELETE FROM closed_pay_periods WHERE period_start = $1 AND period_end = $2")
            .bind(period_start)
            .bind(period_end)
            .execute(pool)
            .await;
}

async fn ensure_all_active_have_compensation(pool: &sqlx::PgPool, admin_id: Uuid, effective: Date) {
    let ids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT e.id FROM employees e
         LEFT JOIN compensation_profiles c ON c.employee_id = e.id
         WHERE e.is_active = TRUE AND c.employee_id IS NULL",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();
    for id in ids {
        let _ = dtr::services::compensation::upsert_profile(
            pool,
            &UpsertProfileInput::new(id, 1_000_000, effective, admin_id),
        )
        .await;
    }
}

fn isolated_payroll_period(settings: &dtr::models::CompanySettings) -> (Date, Date) {
    let anchor = Date::from_calendar_date(2099, Month::June, 10).unwrap();
    let (start, end, _) =
        current_pay_period(anchor, settings.pay_period, settings.pay_period_anchor);
    (start, end)
}

#[tokio::test]
async fn compensation_import_preview_and_apply_via_http() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let admin_code = unique_code("IMAD");
    let emp_code = unique_code("IMEM");
    let admin = create_employee(
        &pool,
        &admin_code,
        "Import HTTP Admin",
        TEST_PIN,
        UserRole::Admin,
        None,
    )
    .await
    .expect("admin");
    let _employee = create_employee(
        &pool,
        &emp_code,
        "Import HTTP Employee",
        TEST_PIN,
        UserRole::Employee,
        None,
    )
    .await
    .expect("employee");

    let effective = Date::from_calendar_date(2026, Month::January, 1).unwrap();
    ensure_all_active_have_compensation(&pool, admin.id, effective).await;

    let csv = format!(
        "employee_code,monthly_salary,ot_rate_percent,transport_allowance,meal_allowance,effective_from\n{emp_code},28500,132,800,400,2026-01-01\n"
    );

    let mut app = test_app(pool.clone()).await;
    let cookies = login_as(&mut app, &admin_code, TEST_PIN).await;
    let (_, import_html, cookies) = get(&mut app, "/admin/compensation/import", &cookies).await;
    let csrf = extract_csrf_token(&import_html).expect("csrf");

    let (status, _, cookies) = post_multipart_field(
        &mut app,
        "/admin/compensation/import",
        &cookies,
        "importboundary",
        &csrf,
        "csv_file",
        Some(("import.csv", csv.as_bytes(), "text/csv")),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let (_, preview_html, cookies) = get(&mut app, "/admin/compensation/import", &cookies).await;
    assert!(preview_html.contains("28500"));
    assert!(preview_html.contains(&emp_code.to_uppercase()));

    let csrf = extract_csrf_token(&preview_html).expect("csrf");
    let (status, _, cookies) = post_form(
        &mut app,
        "/admin/compensation/import/apply",
        &cookies,
        &format!("csrf_token={csrf}"),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let (_, applied_html, _) = get(&mut app, "/admin/compensation/import", &cookies).await;
    assert!(applied_html.contains("Applied compensation"));

    cleanup_employee(&pool, &emp_code).await;
    cleanup_employee(&pool, &admin_code).await;
}

#[tokio::test]
async fn admin_can_manage_deduction_types_via_http() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let admin_code = unique_code("DTAD");
    let type_code = format!("E2E_{}", &Uuid::new_v4().simple().to_string()[..6].to_uppercase());
    let _admin = create_employee(
        &pool,
        &admin_code,
        "Deduction Type HTTP Admin",
        TEST_PIN,
        UserRole::Admin,
        None,
    )
    .await
    .expect("admin");

    let mut app = test_app(pool.clone()).await;
    let cookies = login_as(&mut app, &admin_code, TEST_PIN).await;
    let (_, page_html, cookies) = get(&mut app, "/admin/deduction-types", &cookies).await;
    assert!(page_html.contains("Deduction Types"));
    let csrf = extract_csrf_token(&page_html).expect("csrf");

    let body = format!("code={type_code}&name=E2E+Loan+Test&csrf_token={csrf}");
    let (status, _, cookies) = post_form(&mut app, "/admin/deduction-types", &cookies, &body).await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let (_, after_html, _) = get(&mut app, "/admin/deduction-types", &cookies).await;
    assert!(after_html.contains(&type_code));
    assert!(after_html.contains("E2E Loan Test"));

    let type_id: Uuid = sqlx::query_scalar("SELECT id FROM deduction_types WHERE code = $1")
        .bind(&type_code)
        .fetch_one(&pool)
        .await
        .expect("type id");

    let csrf = extract_csrf_token(&after_html).expect("csrf");
    let (status, _, _) = post_form(
        &mut app,
        &format!("/admin/deduction-types/{type_id}/toggle"),
        &cookies,
        &format!("activate=false&csrf_token={csrf}"),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let _ = sqlx::query("DELETE FROM deduction_types WHERE id = $1")
        .bind(type_id)
        .execute(&pool)
        .await;
    cleanup_employee(&pool, &admin_code).await;
}

#[tokio::test]
async fn employee_can_upload_and_view_profile_photo_via_http() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let code = unique_code("PHOT");
    let _employee = create_employee(
        &pool,
        &code,
        "Photo HTTP Employee",
        TEST_PIN,
        UserRole::Employee,
        None,
    )
    .await
    .expect("employee");

    let mut app = test_app(pool.clone()).await;
    let cookies = login_as(&mut app, &code, TEST_PIN).await;
    let (_, profile_html, cookies) = get(&mut app, "/me/profile", &cookies).await;
    let csrf = extract_csrf_token(&profile_html).expect("csrf");

    let (status, _, cookies) = post_multipart_field(
        &mut app,
        "/me/profile/photo/upload",
        &cookies,
        "photoboundary",
        &csrf,
        "photo",
        Some(("avatar.png", MINI_PNG, "image/png")),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let (_, updated_html, cookies) = get(&mut app, "/me/profile", &cookies).await;
    assert!(updated_html.contains("/me/profile/photo"));

    let (status, body, headers) = get_bytes(&mut app, "/me/profile/photo", &cookies).await;
    assert_eq!(status, StatusCode::OK);
    assert!(!body.is_empty());
    assert!(
        headers
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .contains("image/")
    );

    cleanup_employee(&pool, &code).await;
}

#[tokio::test]
async fn logout_everywhere_invalidates_session_via_http() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let code = unique_code("LOUT");
    let _employee = create_employee(
        &pool,
        &code,
        "Logout Everywhere Employee",
        TEST_PIN,
        UserRole::Employee,
        None,
    )
    .await
    .expect("employee");

    let mut app = test_app(pool.clone()).await;
    let cookies = login_as(&mut app, &code, TEST_PIN).await;
    let (_, profile_html, cookies) = get(&mut app, "/me/profile", &cookies).await;
    let csrf = extract_csrf_token(&profile_html).expect("csrf");

    let (status, _, _) = post_form(
        &mut app,
        "/me/profile/logout-everywhere",
        &cookies,
        &format!("csrf_token={csrf}"),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let (status, _, _) = get(&mut app, "/", &cookies).await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    cleanup_employee(&pool, &code).await;
}

#[tokio::test]
async fn employee_can_download_payslip_pdf_via_http() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let admin_code = unique_code("PDFA");
    let emp_code = unique_code("PDFE");
    let admin = create_employee(
        &pool,
        &admin_code,
        "Employee PDF Admin",
        TEST_PIN,
        UserRole::Admin,
        None,
    )
    .await
    .expect("admin");
    let _employee = create_employee(
        &pool,
        &emp_code,
        "Employee PDF Employee",
        TEST_PIN,
        UserRole::Employee,
        None,
    )
    .await
    .expect("employee");

    let settings = get_settings(&pool).await.expect("settings");
    let (period_start, period_end) = isolated_payroll_period(&settings);
    let effective = Date::from_calendar_date(2026, Month::January, 1).unwrap();
    ensure_all_active_have_compensation(&pool, admin.id, effective).await;
    cleanup_payroll_period(&pool, period_start, period_end).await;

    close_pay_period(
        &pool,
        period_start,
        period_end,
        admin.id,
        Some("employee pdf http"),
    )
    .await
    .expect("close");

    let mut app = test_app(pool.clone()).await;
    let admin_cookies = login_as(&mut app, &admin_code, TEST_PIN).await;
    let (_, payroll_html, admin_cookies) = get(&mut app, "/admin/payroll", &admin_cookies).await;
    let csrf = extract_csrf_token(&payroll_html).expect("csrf");
    let body = format!(
        "period_start={}&period_end={}&csrf_token={csrf}",
        format_date(period_start),
        format_date(period_end)
    );
    let (status, _, admin_cookies) =
        post_form(&mut app, "/admin/payroll", &admin_cookies, &body).await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let run_id: Uuid = sqlx::query_scalar(
        "SELECT id FROM payroll_runs WHERE period_start = $1 AND period_end = $2 ORDER BY created_at DESC LIMIT 1",
    )
    .bind(period_start)
    .bind(period_end)
    .fetch_one(&pool)
    .await
    .expect("run");

    let _ = sqlx::query("UPDATE payroll_lines SET pending_ot_minutes = 0 WHERE run_id = $1")
        .bind(run_id)
        .execute(&pool)
        .await;

    let run_path = format!("/admin/payroll/{run_id}");
    let (_, run_html, admin_cookies) = get(&mut app, &run_path, &admin_cookies).await;
    let csrf = extract_csrf_token(&run_html).expect("csrf");
    let (status, _, _) = post_form(
        &mut app,
        &format!("/admin/payroll/{run_id}/finalize"),
        &admin_cookies,
        &format!("csrf_token={csrf}"),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let line_id: Uuid = sqlx::query_scalar(
        "SELECT l.id FROM payroll_lines l
         JOIN employees e ON e.id = l.employee_id
         WHERE l.run_id = $1 AND e.employee_code = $2",
    )
    .bind(run_id)
    .bind(emp_code.to_uppercase())
    .fetch_one(&pool)
    .await
    .expect("line");

    let emp_cookies = login_as(&mut app, &emp_code, TEST_PIN).await;
    let (status, pdf_body, headers) = get_bytes(
        &mut app,
        &format!("/me/payslips/{line_id}/payslip.pdf"),
        &emp_cookies,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(pdf_body.starts_with(b"%PDF"));
    assert!(
        headers
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .contains("application/pdf")
    );

    cleanup_payroll_period(&pool, period_start, period_end).await;
    cleanup_employee(&pool, &emp_code).await;
    cleanup_employee(&pool, &admin_code).await;
}