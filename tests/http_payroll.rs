mod common;

use axum::http::StatusCode;
use dtr::models::UserRole;

use dtr::services::payroll_controls::close_pay_period;
use dtr::services::reports::current_pay_period;
use dtr::services::settings::get_settings;
use dtr::services::timezone::format_date;
use time::{Date, Month};
use uuid::Uuid;

use common::{
    create_ready_employee, extract_csrf_token, get, login_as, post_form, test_app, test_pool,
};

const TEST_PIN: &str = "482915";

fn unique_code(prefix: &str) -> String {
    format!("{prefix}{}", &Uuid::new_v4().simple().to_string()[..8]).to_uppercase()
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
            &dtr::services::compensation::UpsertProfileInput::new(
                id, 1_000_000, effective, admin_id,
            ),
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
async fn employee_and_manager_forbidden_on_admin_payroll() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let emp_code = unique_code("PYEF");
    let mgr_code = unique_code("PYMF");
    create_ready_employee(
        &pool,
        &emp_code,
        "Payroll Forbidden Employee",
        TEST_PIN,
        UserRole::Employee,
        None,
    )
    .await
    .expect("employee");
    create_ready_employee(
        &pool,
        &mgr_code,
        "Payroll Forbidden Manager",
        TEST_PIN,
        UserRole::Manager,
        None,
    )
    .await
    .expect("manager");

    let mut app = test_app(pool.clone()).await;
    let emp_cookies = login_as(&mut app, &emp_code, TEST_PIN).await;
    let (status, body, _) = get(&mut app, "/admin/payroll", &emp_cookies).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(body.contains("Forbidden") || body.contains("forbidden"));

    let mgr_cookies = login_as(&mut app, &mgr_code, TEST_PIN).await;
    let (status, _, _) = get(&mut app, "/admin/payroll", &mgr_cookies).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    cleanup_employee(&pool, &emp_code).await;
    cleanup_employee(&pool, &mgr_code).await;
}

#[tokio::test]
async fn admin_can_void_payroll_run_via_http() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let admin_code = unique_code("PYVD");
    let admin = create_ready_employee(
        &pool,
        &admin_code,
        "Void HTTP Admin",
        TEST_PIN,
        UserRole::Admin,
        None,
    )
    .await
    .expect("admin");
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
        Some("void http test"),
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

    let run_path = format!("/admin/payroll/{run_id}");
    let (_, run_html, cookies) = get(&mut app, &run_path, &cookies).await;
    let csrf = extract_csrf_token(&run_html).expect("csrf");
    let (status, _, _) = post_form(
        &mut app,
        &format!("/admin/payroll/{run_id}/void"),
        &cookies,
        &format!("csrf_token={csrf}"),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let status: String = sqlx::query_scalar("SELECT status::text FROM payroll_runs WHERE id = $1")
        .bind(run_id)
        .fetch_one(&pool)
        .await
        .expect("status");
    assert_eq!(status, "voided");

    cleanup_payroll_period(&pool, period_start, period_end).await;
    cleanup_employee(&pool, &admin_code).await;
}

#[tokio::test]
async fn reopen_blocked_by_draft_payroll_via_http() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let admin_code = unique_code("PYRB");
    let admin = create_ready_employee(
        &pool,
        &admin_code,
        "Reopen Block HTTP Admin",
        TEST_PIN,
        UserRole::Admin,
        None,
    )
    .await
    .expect("admin");
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
        Some("reopen block http"),
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

    let reports_url = format!(
        "/admin/reports?start={}&end={}",
        format_date(period_start),
        format_date(period_end)
    );
    let (_, reports_html, cookies) = get(&mut app, &reports_url, &cookies).await;
    assert!(reports_html.contains("Void the draft payroll run"));

    let csrf = extract_csrf_token(&reports_html).expect("csrf");
    let body = format!(
        "start={}&end={}&csrf_token={csrf}",
        format_date(period_start),
        format_date(period_end)
    );
    let (status, body, _) =
        post_form(&mut app, "/admin/reports/reopen-period", &cookies, &body).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body.contains("draft payroll run") || body.contains("Cannot reopen"),
        "expected reopen blocked message, got: {body}"
    );

    cleanup_payroll_period(&pool, period_start, period_end).await;
    cleanup_employee(&pool, &admin_code).await;
}

#[tokio::test]
async fn deductions_cannot_exceed_gross_via_http() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let admin_code = unique_code("PYDC");
    let emp_code = unique_code("PYDE");
    let admin = create_ready_employee(
        &pool,
        &admin_code,
        "Deduction Cap Admin",
        TEST_PIN,
        UserRole::Admin,
        None,
    )
    .await
    .expect("admin");
    let _employee = create_ready_employee(
        &pool,
        &emp_code,
        "Deduction Cap Employee",
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
        Some("deduction cap http"),
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

    let deductions_path = format!("/admin/payroll/{run_id}/lines/{line_id}");
    let (_, deductions_html, cookies) = get(&mut app, &deductions_path, &cookies).await;
    let csrf = extract_csrf_token(&deductions_html).expect("csrf");
    let body = format!("amount_sss=999999.00&csrf_token={csrf}");
    let (status, body, _) = post_form(&mut app, &deductions_path, &cookies, &body).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body.contains("cannot exceed gross") || body.contains("Total deductions"),
        "expected deduction cap error, got: {body}"
    );

    cleanup_payroll_period(&pool, period_start, period_end).await;
    cleanup_employee(&pool, &emp_code).await;
    cleanup_employee(&pool, &admin_code).await;
}

#[tokio::test]
async fn non_canonical_closed_period_rejects_draft_via_http() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let admin_code = unique_code("PYCP");
    let admin = create_ready_employee(
        &pool,
        &admin_code,
        "Canonical HTTP Admin",
        TEST_PIN,
        UserRole::Admin,
        None,
    )
    .await
    .expect("admin");
    let settings = get_settings(&pool).await.expect("settings");
    let (period_start, _period_end) = isolated_payroll_period(&settings);
    let effective = Date::from_calendar_date(2026, Month::January, 1).unwrap();
    ensure_all_active_have_compensation(&pool, admin.id, effective).await;

    let bad_end = period_start + time::Duration::days(1);
    cleanup_payroll_period(&pool, period_start, bad_end).await;
    close_pay_period(&pool, period_start, bad_end, admin.id, Some("partial http"))
        .await
        .expect("close partial");

    let mut app = test_app(pool.clone()).await;
    let cookies = login_as(&mut app, &admin_code, TEST_PIN).await;
    let (_, payroll_html, cookies) = get(&mut app, "/admin/payroll", &cookies).await;
    let csrf = extract_csrf_token(&payroll_html).expect("csrf");
    let body = format!(
        "period_start={}&period_end={}&csrf_token={csrf}",
        format_date(period_start),
        format_date(bad_end)
    );
    let (status, body, _) = post_form(&mut app, "/admin/payroll", &cookies, &body).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body.contains("full") || body.contains("pay period"),
        "expected canonical rejection, got: {body}"
    );

    cleanup_payroll_period(&pool, period_start, bad_end).await;
    cleanup_employee(&pool, &admin_code).await;
}

#[tokio::test]
async fn finalized_run_exports_csv_bank_and_pdf_via_http() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let admin_code = unique_code("PYEX");
    let emp_code = unique_code("PYEE");
    let admin = create_ready_employee(
        &pool,
        &admin_code,
        "Export HTTP Admin",
        TEST_PIN,
        UserRole::Admin,
        None,
    )
    .await
    .expect("admin");
    let employee = create_ready_employee(
        &pool,
        &emp_code,
        "Export HTTP Employee",
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

    dtr::services::profile::update_admin(
        &pool,
        employee.id,
        admin.id,
        dtr::services::profile::AdminProfileInput {
            contact_number: None,
            personal_email: None,
            birthdate: None,
            address: None,
            emergency_contact_name: None,
            emergency_contact_phone: None,
            job_title: None,
            department: None,
            employment_type: None,
            date_hired: None,
            work_location: None,
            bank_account: Some("9876543210"),
            tin: None,
            sss_number: None,
            philhealth_number: None,
        },
    )
    .await
    .expect("bank profile");

    close_pay_period(
        &pool,
        period_start,
        period_end,
        admin.id,
        Some("export http test"),
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

    let _ = sqlx::query("UPDATE payroll_lines SET pending_ot_minutes = 0 WHERE run_id = $1")
        .bind(run_id)
        .execute(&pool)
        .await;

    let run_path = format!("/admin/payroll/{run_id}");
    let (_, run_html, cookies) = get(&mut app, &run_path, &cookies).await;
    let csrf = extract_csrf_token(&run_html).expect("csrf");
    let (status, _, cookies) = post_form(
        &mut app,
        &format!("/admin/payroll/{run_id}/finalize"),
        &cookies,
        &format!("csrf_token={csrf}"),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let (status, payroll_csv, _) = get(
        &mut app,
        &format!("/admin/payroll/{run_id}/export.csv"),
        &cookies,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(payroll_csv.contains("Allowances"));

    let (status, bank_csv, _) = get(
        &mut app,
        &format!("/admin/payroll/{run_id}/export-bank.csv"),
        &cookies,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(bank_csv.contains("9876543210"));

    let (status, journal_csv, _) = get(
        &mut app,
        &format!("/admin/payroll/{run_id}/export-journal.csv"),
        &cookies,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(journal_csv.contains("Salaries expense"));

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

    let (status, pdf_body, headers) = common::get_bytes(
        &mut app,
        &format!("/admin/payroll/{run_id}/lines/{line_id}/payslip.pdf"),
        &cookies,
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
