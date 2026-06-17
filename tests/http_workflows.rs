mod common;

use axum::http::StatusCode;
use dtr::models::{LeaveRequestStatus, UserRole};
use dtr::services::employees::create_employee;
use dtr::services::settings::get_settings;
use dtr::services::timezone::{company_date_now, format_date};
use time::{Date, Month};
use uuid::Uuid;

use common::{extract_csrf_token, get, login_as, post_form, test_app, test_pool};

const TEST_PIN: &str = "482915";

fn unique_code(prefix: &str) -> String {
    format!("{prefix}{}", &Uuid::new_v4().simple().to_string()[..8])
}

async fn cleanup_employee(pool: &sqlx::PgPool, code: &str) {
    let _ = sqlx::query("DELETE FROM leave_requests WHERE employee_id IN (SELECT id FROM employees WHERE employee_code = $1)")
        .bind(code)
        .execute(pool)
        .await;
    let _ = sqlx::query("DELETE FROM time_entries WHERE employee_id IN (SELECT id FROM employees WHERE employee_code = $1)")
        .bind(code)
        .execute(pool)
        .await;
    let _ = sqlx::query("DELETE FROM employees WHERE employee_code = $1")
        .bind(code)
        .execute(pool)
        .await;
}

async fn cleanup_closed_period(pool: &sqlx::PgPool, start: Date, end: Date) {
    let _ =
        sqlx::query("DELETE FROM closed_pay_periods WHERE period_start = $1 AND period_end = $2")
            .bind(start)
            .bind(end)
            .execute(pool)
            .await;
}

#[tokio::test]
async fn employee_can_submit_and_cancel_leave_via_http() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let code = unique_code("LVEM");
    let employee = create_employee(
        &pool,
        &code,
        "Leave Submit Test",
        TEST_PIN,
        UserRole::Employee,
        None,
    )
    .await
    .expect("create employee");

    let settings = get_settings(&pool).await.expect("settings");
    let today = company_date_now(&settings).expect("today");
    let start = format_date(today);
    let end = format_date(today);

    let mut app = test_app(pool.clone()).await;
    let cookies = login_as(&mut app, &code, TEST_PIN).await;
    let (_, leave_html, cookies) = get(&mut app, "/me/leave", &cookies).await;
    let csrf = extract_csrf_token(&leave_html).expect("csrf");
    let body = format!(
        "start_date={start}&end_date={end}&leave_type=vacation&reason=Family%20trip&csrf_token={csrf}"
    );
    let (status, _, cookies) = post_form(&mut app, "/me/leave", &cookies, &body).await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let request_id: Uuid = sqlx::query_scalar(
        "SELECT id FROM leave_requests WHERE employee_id = $1 ORDER BY created_at DESC LIMIT 1",
    )
    .bind(employee.id)
    .fetch_one(&pool)
    .await
    .expect("leave request");

    let (_, leave_after_submit, cookies) = get(&mut app, "/me/leave", &cookies).await;
    assert!(leave_after_submit.contains("Pending"));
    let csrf = extract_csrf_token(&leave_after_submit).expect("csrf");
    let cancel_path = format!("/me/leave/{request_id}/cancel");
    let (status, _, _) = post_form(
        &mut app,
        &cancel_path,
        &cookies,
        &format!("csrf_token={csrf}"),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let status: LeaveRequestStatus =
        sqlx::query_scalar("SELECT status::text FROM leave_requests WHERE id = $1")
            .bind(request_id)
            .fetch_one(&pool)
            .await
            .expect("leave status");
    assert_eq!(status, LeaveRequestStatus::Cancelled);

    cleanup_employee(&pool, &code).await;
}

#[tokio::test]
async fn manager_can_approve_leave_via_http() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let mgr_code = unique_code("LVMA");
    let emp_code = unique_code("LVMB");
    let manager = create_employee(
        &pool,
        &mgr_code,
        "Leave Manager",
        TEST_PIN,
        UserRole::Manager,
        None,
    )
    .await
    .expect("create manager");
    let employee = create_employee(
        &pool,
        &emp_code,
        "Leave Employee",
        TEST_PIN,
        UserRole::Employee,
        Some(manager.id),
    )
    .await
    .expect("create employee");

    let settings = get_settings(&pool).await.expect("settings");
    let today = company_date_now(&settings).expect("today");
    let start = format_date(today);
    let end = format_date(today);

    let mut app = test_app(pool.clone()).await;
    let emp_cookies = login_as(&mut app, &emp_code, TEST_PIN).await;
    let (_, leave_html, emp_cookies) = get(&mut app, "/me/leave", &emp_cookies).await;
    let csrf = extract_csrf_token(&leave_html).expect("csrf");
    let body = format!(
        "start_date={start}&end_date={end}&leave_type=sick_leave&reason=Flu&csrf_token={csrf}"
    );
    let (status, _, _) = post_form(&mut app, "/me/leave", &emp_cookies, &body).await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let request_id: Uuid = sqlx::query_scalar(
        "SELECT id FROM leave_requests WHERE employee_id = $1 ORDER BY created_at DESC LIMIT 1",
    )
    .bind(employee.id)
    .fetch_one(&pool)
    .await
    .expect("leave request");

    let mgr_cookies = login_as(&mut app, &mgr_code, TEST_PIN).await;
    let (_, manager_leave_html, mgr_cookies) = get(&mut app, "/manager/leave", &mgr_cookies).await;
    assert!(manager_leave_html.contains(&emp_code));
    let csrf = extract_csrf_token(&manager_leave_html).expect("csrf");
    let review_path = format!("/manager/leave/{request_id}/review");
    let body = format!("action=approve&note=Get%20well&csrf_token={csrf}");
    let (status, _, _) = post_form(&mut app, &review_path, &mgr_cookies, &body).await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let status: LeaveRequestStatus =
        sqlx::query_scalar("SELECT status::text FROM leave_requests WHERE id = $1")
            .bind(request_id)
            .fetch_one(&pool)
            .await
            .expect("leave status");
    assert_eq!(status, LeaveRequestStatus::Approved);

    cleanup_employee(&pool, &emp_code).await;
    cleanup_employee(&pool, &mgr_code).await;
}

#[tokio::test]
async fn admin_can_close_and_reopen_pay_period_via_http() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let code = unique_code("PPAD");
    create_employee(
        &pool,
        &code,
        "Pay Period Admin",
        TEST_PIN,
        UserRole::Admin,
        None,
    )
    .await
    .expect("create admin");

    let settings = get_settings(&pool).await.expect("settings");
    let today = company_date_now(&settings).expect("today");
    let start = format_date(today);
    let end = format_date(today);
    cleanup_closed_period(&pool, today, today).await;

    let mut app = test_app(pool.clone()).await;
    let cookies = login_as(&mut app, &code, TEST_PIN).await;
    let (_, reports_html, cookies) = get(&mut app, "/admin/reports", &cookies).await;
    let csrf = extract_csrf_token(&reports_html).expect("csrf");
    let body = format!("start={start}&end={end}&note=HTTP%20test%20close&csrf_token={csrf}");
    let (status, _, cookies) =
        post_form(&mut app, "/admin/reports/close-period", &cookies, &body).await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let closed: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM closed_pay_periods WHERE period_start = $1 AND period_end = $2)",
    )
    .bind(today)
    .bind(today)
    .fetch_one(&pool)
    .await
    .expect("closed period");
    assert!(closed);

    let (_, reports_closed_html, cookies) = get(&mut app, "/admin/reports", &cookies).await;
    assert!(reports_closed_html.contains("closed") || reports_closed_html.contains("Closed"));
    let csrf = extract_csrf_token(&reports_closed_html).expect("csrf");
    let body = format!("start={start}&end={end}&csrf_token={csrf}");
    let (status, _, _) = post_form(&mut app, "/admin/reports/reopen-period", &cookies, &body).await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let still_closed: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM closed_pay_periods WHERE period_start = $1 AND period_end = $2)",
    )
    .bind(today)
    .bind(today)
    .fetch_one(&pool)
    .await
    .expect("closed period");
    assert!(!still_closed);

    cleanup_employee(&pool, &code).await;
}

#[tokio::test]
async fn manager_can_submit_new_correction_via_http() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let mgr_code = unique_code("CRMG");
    let emp_code = unique_code("CRME");
    let manager = create_employee(
        &pool,
        &mgr_code,
        "Correction Manager",
        TEST_PIN,
        UserRole::Manager,
        None,
    )
    .await
    .expect("create manager");
    let employee = create_employee(
        &pool,
        &emp_code,
        "Correction Employee",
        TEST_PIN,
        UserRole::Employee,
        Some(manager.id),
    )
    .await
    .expect("create employee");

    let work_date = Date::from_calendar_date(2026, Month::May, 12).unwrap();
    let work_date_str = format_date(work_date);
    let correction_path = format!("/manager/team/{}/correct", employee.id);

    let mut app = test_app(pool.clone()).await;
    let cookies = login_as(&mut app, &mgr_code, TEST_PIN).await;
    let (_, correction_html, cookies) = get(&mut app, &correction_path, &cookies).await;
    assert!(
        correction_html.contains("Add Time Entry")
            || correction_html.contains("Correct Time Entry")
    );
    let csrf = extract_csrf_token(&correction_html).expect("csrf");
    let body = format!(
        "employee_id={}&work_date={work_date_str}&clock_in=08:00&clock_out=17:00&reason=Forgot%20to%20clock%20in&csrf_token={csrf}",
        employee.id
    );
    let (status, _, _) = post_form(&mut app, "/manager/correct", &cookies, &body).await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let entry_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM time_entries WHERE employee_id = $1 AND work_date = $2",
    )
    .bind(employee.id)
    .bind(work_date)
    .fetch_one(&pool)
    .await
    .expect("entry count");
    assert_eq!(entry_count, 1);

    cleanup_employee(&pool, &emp_code).await;
    cleanup_employee(&pool, &mgr_code).await;
}
