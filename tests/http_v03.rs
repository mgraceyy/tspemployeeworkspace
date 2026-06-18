mod common;

use axum::http::StatusCode;
use dtr::models::{AttendanceStatus, UserRole};
use dtr::services::attendance::mark_absence_for_employee;
use dtr::services::compensation::UpsertProfileInput;
use dtr::services::employees::find_by_id;
use dtr::services::payroll::list_deduction_types;
use dtr::services::payroll_controls::close_pay_period;
use dtr::services::profile::get_profile;
use dtr::services::reports::current_pay_period;
use dtr::services::settings::get_settings;
use dtr::services::timezone::format_date;
use time::{Date, Month};
use uuid::Uuid;

use common::{
    create_ready_employee, extract_csrf_token, get, get_bytes, login_as, post_form,
    post_multipart_field, test_app, test_pool,
};

const TEST_PIN: &str = "482915";
const TEMP_PIN: &str = "887766";

const MINI_PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
    0x89, 0x00, 0x00, 0x00, 0x0a, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0d, 0x0a, 0x2d, 0xb4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae,
    0x42, 0x60, 0x82,
];

fn unique_code(prefix: &str) -> String {
    format!("{prefix}{}", &Uuid::new_v4().simple().to_string()[..8]).to_uppercase()
}

async fn cleanup_employee(pool: &sqlx::PgPool, code: &str) {
    let _ = sqlx::query(
        "DELETE FROM pin_reset_requests WHERE employee_id IN (SELECT id FROM employees WHERE employee_code = $1)",
    )
    .bind(code)
    .execute(pool)
    .await;
    let _ = sqlx::query(
        "DELETE FROM employee_deduction_defaults WHERE employee_id IN (SELECT id FROM employees WHERE employee_code = $1)",
    )
    .bind(code)
    .execute(pool)
    .await;
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
    let admin = create_ready_employee(
        &pool,
        &admin_code,
        "Import HTTP Admin",
        TEST_PIN,
        UserRole::Admin,
        None,
    )
    .await
    .expect("admin");
    let _employee = create_ready_employee(
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
    let type_code = format!(
        "E2E_{}",
        &Uuid::new_v4().simple().to_string()[..6].to_uppercase()
    );
    let _admin = create_ready_employee(
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
    let _employee = create_ready_employee(
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
    assert!(headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .contains("image/"));

    cleanup_employee(&pool, &code).await;
}

#[tokio::test]
async fn logout_everywhere_invalidates_session_via_http() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let code = unique_code("LOUT");
    let _employee = create_ready_employee(
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
    let admin = create_ready_employee(
        &pool,
        &admin_code,
        "Employee PDF Admin",
        TEST_PIN,
        UserRole::Admin,
        None,
    )
    .await
    .expect("admin");
    let _employee = create_ready_employee(
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
    assert!(headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .contains("application/pdf"));

    cleanup_payroll_period(&pool, period_start, period_end).await;
    cleanup_employee(&pool, &emp_code).await;
    cleanup_employee(&pool, &admin_code).await;
}

#[tokio::test]
async fn manager_can_approve_pin_reset_via_http() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let mgr_code = unique_code("PRMG");
    let emp_code = unique_code("PREM");
    let manager = create_ready_employee(
        &pool,
        &mgr_code,
        "PIN Reset HTTP Manager",
        TEST_PIN,
        UserRole::Manager,
        None,
    )
    .await
    .expect("manager");
    let employee = create_ready_employee(
        &pool,
        &emp_code,
        "PIN Reset HTTP Employee",
        TEST_PIN,
        UserRole::Employee,
        Some(manager.id),
    )
    .await
    .expect("employee");

    let mut app = test_app(pool.clone()).await;
    let emp_cookies = login_as(&mut app, &emp_code, TEST_PIN).await;
    let (_, profile_html, emp_cookies) = get(&mut app, "/me/profile", &emp_cookies).await;
    let csrf = extract_csrf_token(&profile_html).expect("csrf");
    let (status, _, _) = post_form(
        &mut app,
        "/me/profile/request-pin-reset",
        &emp_cookies,
        &format!("reason=Forgot+PIN&csrf_token={csrf}"),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let request_id: Uuid = sqlx::query_scalar(
        "SELECT id FROM pin_reset_requests WHERE employee_id = $1 AND status = 'pending'",
    )
    .bind(employee.id)
    .fetch_one(&pool)
    .await
    .expect("pending request");

    let mgr_cookies = login_as(&mut app, &mgr_code, TEST_PIN).await;
    let (_, pin_resets_html, mgr_cookies) =
        get(&mut app, "/manager/pin-resets", &mgr_cookies).await;
    assert!(pin_resets_html.contains(&emp_code.to_uppercase()));
    let csrf = extract_csrf_token(&pin_resets_html).expect("csrf");
    let (status, _, _) = post_form(
        &mut app,
        &format!("/manager/pin-resets/{request_id}/approve"),
        &mgr_cookies,
        &format!("temp_pin={TEMP_PIN}&csrf_token={csrf}"),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let row = find_by_id(&pool, employee.id)
        .await
        .expect("find employee")
        .expect("employee row");
    assert!(row.must_change_pin);
    assert_eq!(row.session_version, 1);

    cleanup_employee(&pool, &emp_code).await;
    cleanup_employee(&pool, &mgr_code).await;
}

#[tokio::test]
async fn manager_can_deny_pin_reset_via_http() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let mgr_code = unique_code("PRDN");
    let emp_code = unique_code("PRDE");
    let manager = create_ready_employee(
        &pool,
        &mgr_code,
        "PIN Deny HTTP Manager",
        TEST_PIN,
        UserRole::Manager,
        None,
    )
    .await
    .expect("manager");
    let employee = create_ready_employee(
        &pool,
        &emp_code,
        "PIN Deny HTTP Employee",
        TEST_PIN,
        UserRole::Employee,
        Some(manager.id),
    )
    .await
    .expect("employee");

    let mut app = test_app(pool.clone()).await;
    let emp_cookies = login_as(&mut app, &emp_code, TEST_PIN).await;
    let (_, profile_html, emp_cookies) = get(&mut app, "/me/profile", &emp_cookies).await;
    let csrf = extract_csrf_token(&profile_html).expect("csrf");
    let (status, _, _) = post_form(
        &mut app,
        "/me/profile/request-pin-reset",
        &emp_cookies,
        &format!("reason=Not+needed&csrf_token={csrf}"),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let request_id: Uuid = sqlx::query_scalar(
        "SELECT id FROM pin_reset_requests WHERE employee_id = $1 AND status = 'pending'",
    )
    .bind(employee.id)
    .fetch_one(&pool)
    .await
    .expect("pending request");

    let mgr_cookies = login_as(&mut app, &mgr_code, TEST_PIN).await;
    let (_, pin_resets_html, mgr_cookies) =
        get(&mut app, "/manager/pin-resets", &mgr_cookies).await;
    let csrf = extract_csrf_token(&pin_resets_html).expect("csrf");
    let (status, _, _) = post_form(
        &mut app,
        &format!("/manager/pin-resets/{request_id}/deny"),
        &mgr_cookies,
        &format!("review_note=Verified+with+employee&csrf_token={csrf}"),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let status: String =
        sqlx::query_scalar("SELECT status::text FROM pin_reset_requests WHERE id = $1")
            .bind(request_id)
            .fetch_one(&pool)
            .await
            .expect("request status");
    assert_eq!(status, "denied");

    let row = find_by_id(&pool, employee.id)
        .await
        .expect("find employee")
        .expect("employee row");
    assert!(!row.must_change_pin);

    cleanup_employee(&pool, &emp_code).await;
    cleanup_employee(&pool, &mgr_code).await;
}

#[tokio::test]
async fn admin_can_save_deduction_defaults_via_http() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let admin_code = unique_code("DDAD");
    let emp_code = unique_code("DDEM");
    let _admin = create_ready_employee(
        &pool,
        &admin_code,
        "Deduction Defaults HTTP Admin",
        TEST_PIN,
        UserRole::Admin,
        None,
    )
    .await
    .expect("admin");
    let employee = create_ready_employee(
        &pool,
        &emp_code,
        "Deduction Defaults HTTP Employee",
        TEST_PIN,
        UserRole::Employee,
        None,
    )
    .await
    .expect("employee");

    let types = list_deduction_types(&pool).await.expect("types");
    let sss = types.iter().find(|t| t.code == "SSS").expect("sss type");

    let mut app = test_app(pool.clone()).await;
    let cookies = login_as(&mut app, &admin_code, TEST_PIN).await;
    let comp_path = format!("/admin/employees/{}/compensation", employee.id);
    let (_, comp_html, cookies) = get(&mut app, &comp_path, &cookies).await;
    assert!(comp_html.contains("Default payroll deductions"));
    let csrf = extract_csrf_token(&comp_html).expect("csrf");

    let defaults_path = format!(
        "/admin/employees/{}/compensation/deduction-defaults",
        employee.id
    );
    let body = format!("default_sss=250.00&csrf_token={csrf}");
    let (status, _, cookies) = post_form(&mut app, &defaults_path, &cookies, &body).await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let (_, saved_html, _) = get(&mut app, &comp_path, &cookies).await;
    assert!(saved_html.contains("250.00"));

    let amount: i64 = sqlx::query_scalar(
        "SELECT amount_cents FROM employee_deduction_defaults
         WHERE employee_id = $1 AND deduction_type_id = $2",
    )
    .bind(employee.id)
    .bind(sss.id)
    .fetch_one(&pool)
    .await
    .expect("default amount");
    assert_eq!(amount, 25_000);

    cleanup_employee(&pool, &emp_code).await;
    cleanup_employee(&pool, &admin_code).await;
}

#[tokio::test]
async fn admin_can_save_payroll_identity_fields_via_http() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let admin_code = unique_code("IDAD");
    let emp_code = unique_code("IDEM");
    let _admin = create_ready_employee(
        &pool,
        &admin_code,
        "Identity HTTP Admin",
        TEST_PIN,
        UserRole::Admin,
        None,
    )
    .await
    .expect("admin");
    let employee = create_ready_employee(
        &pool,
        &emp_code,
        "Identity HTTP Employee",
        TEST_PIN,
        UserRole::Employee,
        None,
    )
    .await
    .expect("employee");

    let mut app = test_app(pool.clone()).await;
    let cookies = login_as(&mut app, &admin_code, TEST_PIN).await;
    let profile_path = format!("/admin/employees/{}/profile", employee.id);
    let (_, profile_html, cookies) = get(&mut app, &profile_path, &cookies).await;
    assert!(profile_html.contains("Payroll Identity"));
    let csrf = extract_csrf_token(&profile_html).expect("csrf");

    let body = format!(
        "bank_account=1234567890&tin=123-456-789&sss_number=01-2345678-9&philhealth_number=12-345678901-2&csrf_token={csrf}"
    );
    let (status, _, _) = post_form(&mut app, &profile_path, &cookies, &body).await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let profile = get_profile(&pool, employee.id).await.expect("profile");
    assert_eq!(profile.bank_account.as_deref(), Some("1234567890"));
    assert_eq!(profile.tin.as_deref(), Some("123-456-789"));
    assert_eq!(profile.sss_number.as_deref(), Some("01-2345678-9"));
    assert_eq!(profile.philhealth_number.as_deref(), Some("12-345678901-2"));

    cleanup_employee(&pool, &emp_code).await;
    cleanup_employee(&pool, &admin_code).await;
}

#[tokio::test]
async fn draft_payroll_run_shows_stale_attendance_warning_via_http() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let admin_code = unique_code("STAD");
    let emp_code = unique_code("STEM");
    let admin = create_ready_employee(
        &pool,
        &admin_code,
        "Stale Warning HTTP Admin",
        TEST_PIN,
        UserRole::Admin,
        None,
    )
    .await
    .expect("admin");
    let employee = create_ready_employee(
        &pool,
        &emp_code,
        "Stale Warning HTTP Employee",
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
        Some("stale warning http"),
    )
    .await
    .expect("close");

    let mut app = test_app(pool.clone()).await;
    let cookies = login_as(&mut app, &admin_code, TEST_PIN).await;
    let (_, payroll_html, cookies) = get(&mut app, "/admin/payroll", &cookies).await;
    let csrf = extract_csrf_token(&payroll_html).expect("csrf");
    let body = format!(
        "period_start={}&period_end={}&csrf_token={csrf}",
        format_date(period_start),
        format_date(period_end)
    );
    let (status, _, cookies) = post_form(&mut app, "/admin/payroll", &cookies, &body).await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let run_id: Uuid = sqlx::query_scalar(
        "SELECT id FROM payroll_runs WHERE period_start = $1 AND period_end = $2 ORDER BY created_at DESC LIMIT 1",
    )
    .bind(period_start)
    .bind(period_end)
    .fetch_one(&pool)
    .await
    .expect("run");

    let (_, fresh_html, cookies) =
        get(&mut app, &format!("/admin/payroll/{run_id}"), &cookies).await;
    assert!(
        !fresh_html.contains("Attendance changed"),
        "fresh draft should not show stale warning"
    );

    mark_absence_for_employee(
        &pool,
        employee.id,
        period_start,
        AttendanceStatus::NoShow,
        admin.id,
        true,
        admin.id,
    )
    .await
    .expect("mark no-show");

    let (_, stale_html, _) = get(&mut app, &format!("/admin/payroll/{run_id}"), &cookies).await;
    assert!(stale_html.contains("Attendance changed"));

    cleanup_payroll_period(&pool, period_start, period_end).await;
    cleanup_employee(&pool, &emp_code).await;
    cleanup_employee(&pool, &admin_code).await;
}
