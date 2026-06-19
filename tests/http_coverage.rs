mod common;

use axum::http::StatusCode;
use dtr::models::{EodReportStatus, OtStatus, UserRole};
use dtr::services::clock::clock_in;

use dtr::services::reports::current_pay_period;
use dtr::services::requirements::{create_type, list_for_employee};
use dtr::services::settings::get_settings;
use dtr::services::timezone::{company_date_now, format_date};
use time::{Date, Month};
use uuid::Uuid;

use common::{
    create_ready_employee, expect_csrf_token, extract_csrf_token, get, get_bytes, get_with_headers,
    login_as, post_form, post_multipart, test_app, test_pool, url_encode,
};

const TEST_PIN: &str = "482915";

fn unique_code(prefix: &str) -> String {
    format!("{prefix}{}", &Uuid::new_v4().simple().to_string()[..8]).to_uppercase()
}

async fn cleanup_employee(pool: &sqlx::PgPool, code: &str) {
    let _ = sqlx::query("DELETE FROM eod_reports WHERE employee_id IN (SELECT id FROM employees WHERE employee_code = $1)")
        .bind(code)
        .execute(pool)
        .await;
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
async fn admin_can_create_employee_via_http() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let admin_code = unique_code("ADCR");
    let new_code = unique_code("NEW");
    create_ready_employee(
        &pool,
        &admin_code,
        "Create Employee Admin",
        TEST_PIN,
        UserRole::Admin,
        None,
    )
    .await
    .expect("create admin");

    let mut app = test_app(pool.clone()).await;
    let cookies = login_as(&mut app, &admin_code, TEST_PIN).await;
    let (_, employees_html, cookies) = get(&mut app, "/admin/employees", &cookies).await;
    let csrf = extract_csrf_token(&employees_html).expect("csrf");
    let body = format!(
        "employee_code={new_code}&full_name=HTTP%20Created%20User&pin={TEST_PIN}&role=employee&csrf_token={csrf}"
    );
    let (status, _, _) = post_form(&mut app, "/admin/employees", &cookies, &body).await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM employees WHERE employee_code = $1)")
            .bind(new_code.to_uppercase())
            .fetch_one(&pool)
            .await
            .expect("check employee");
    assert!(exists);

    cleanup_employee(&pool, &new_code).await;
    cleanup_employee(&pool, &admin_code).await;
}

#[tokio::test]
async fn admin_can_save_settings_via_http() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let code = unique_code("SETT");
    create_ready_employee(
        &pool,
        &code,
        "Settings Admin",
        TEST_PIN,
        UserRole::Admin,
        None,
    )
    .await
    .expect("create admin");

    let settings = get_settings(&pool).await.expect("settings");
    let company_name = format!("HTTPSettings{}", &Uuid::new_v4().simple().to_string()[..6]);

    let mut app = test_app(pool.clone()).await;
    let cookies = login_as(&mut app, &code, TEST_PIN).await;
    let (status, settings_html, cookies) = get(&mut app, "/admin/settings", &cookies).await;
    let csrf = expect_csrf_token("/admin/settings", status, &settings_html);
    let anchor = format_date(settings.pay_period_anchor);
    let pay_period = match settings.pay_period {
        dtr::models::PayPeriodType::Weekly => "weekly",
        dtr::models::PayPeriodType::Biweekly => "biweekly",
        dtr::models::PayPeriodType::Monthly => "monthly",
        dtr::models::PayPeriodType::Semimonthly => "semimonthly",
    };
    let ot_flag = if settings.ot_requires_approval {
        "&ot_requires_approval=on"
    } else {
        ""
    };
    let body = format!(
        "company_name={}&timezone={}&break_minutes={}&ot_threshold_minutes={}&grace_minutes={}&pay_period={}&pay_period_anchor={}{ot_flag}&journal_salary_expense_account={}&journal_salary_expense_label={}&journal_net_payable_account={}&journal_net_payable_label={}&csrf_token={csrf}",
        url_encode(&company_name),
        url_encode(&settings.timezone),
        settings.break_minutes,
        settings.ot_threshold_minutes,
        settings.grace_minutes,
        pay_period,
        anchor,
        url_encode(&settings.journal_salary_expense_account),
        url_encode(&settings.journal_salary_expense_label),
        url_encode(&settings.journal_net_payable_account),
        url_encode(&settings.journal_net_payable_label),
    );
    let (status, _, _) = post_form(&mut app, "/admin/settings", &cookies, &body).await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let saved = get_settings(&pool).await.expect("settings");
    assert_eq!(saved.company_name, company_name);

    cleanup_employee(&pool, &code).await;
}

#[tokio::test]
async fn admin_can_save_shift_via_http() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let admin_code = unique_code("SHFT");
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
    let (_, shifts_html, cookies) = get(&mut app, &path, &cookies).await;
    let csrf = extract_csrf_token(&shifts_html).expect("csrf");
    let body = format!(
        "employee_id={}&day_of_week=1&start_time=09:00&end_time=18:00&csrf_token={csrf}",
        employee.id
    );
    let (status, _, _) = post_form(&mut app, "/admin/shifts", &cookies, &body).await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM shift_templates WHERE employee_id = $1 AND day_of_week = 1",
    )
    .bind(employee.id)
    .fetch_one(&pool)
    .await
    .expect("shift count");
    assert_eq!(count, 1);

    cleanup_employee(&pool, &emp_code).await;
    cleanup_employee(&pool, &admin_code).await;
}

#[tokio::test]
async fn admin_can_delete_report_preset_via_http() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let code = unique_code("PRDL");
    create_ready_employee(
        &pool,
        &code,
        "Preset Delete Admin",
        TEST_PIN,
        UserRole::Admin,
        None,
    )
    .await
    .expect("create admin");

    let preset_name = format!("Delete Me {}", &Uuid::new_v4().simple().to_string()[..6]);
    let mut app = test_app(pool.clone()).await;
    let cookies = login_as(&mut app, &code, TEST_PIN).await;
    let (status, reports_html, cookies) = get(&mut app, "/admin/reports", &cookies).await;
    let csrf = expect_csrf_token("/admin/reports", status, &reports_html);
    let body = format!("preset_name={preset_name}&csrf_token={csrf}");
    let (status, _, cookies) = post_form(&mut app, "/admin/reports/presets", &cookies, &body).await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let preset_id: Uuid = sqlx::query_scalar("SELECT id FROM report_presets WHERE name = $1")
        .bind(&preset_name)
        .fetch_one(&pool)
        .await
        .expect("preset id");

    let (_, reports_after_save, cookies) = get(&mut app, "/admin/reports", &cookies).await;
    let csrf = extract_csrf_token(&reports_after_save).expect("csrf");
    let delete_path = format!("/admin/reports/presets/{preset_id}/delete");
    let (status, _, _) = post_form(
        &mut app,
        &delete_path,
        &cookies,
        &format!("csrf_token={csrf}"),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM report_presets WHERE id = $1)")
            .bind(preset_id)
            .fetch_one(&pool)
            .await
            .expect("preset exists");
    assert!(!exists);

    cleanup_employee(&pool, &code).await;
}

#[tokio::test]
async fn manager_can_mark_absence_via_http() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let mgr_code = unique_code("ABMG");
    let emp_code = unique_code("ABEM");
    let manager = create_ready_employee(
        &pool,
        &mgr_code,
        "Absence Manager",
        TEST_PIN,
        UserRole::Manager,
        None,
    )
    .await
    .expect("create manager");
    let employee = create_ready_employee(
        &pool,
        &emp_code,
        "Absence Employee",
        TEST_PIN,
        UserRole::Employee,
        Some(manager.id),
    )
    .await
    .expect("create employee");

    let settings = get_settings(&pool).await.expect("settings");
    let today = company_date_now(&settings).expect("today");

    let mut app = test_app(pool.clone()).await;
    let cookies = login_as(&mut app, &mgr_code, TEST_PIN).await;
    let (_, dashboard_html, cookies) = get(&mut app, "/manager", &cookies).await;
    let csrf = extract_csrf_token(&dashboard_html).expect("csrf");
    let body = format!(
        "employee_id={}&absence_type=sick_leave&csrf_token={csrf}",
        employee.id
    );
    let (status, _, _) = post_form(&mut app, "/manager/absence", &cookies, &body).await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let attendance: String = sqlx::query_scalar(
        "SELECT attendance::text FROM time_entries WHERE employee_id = $1 AND work_date = $2",
    )
    .bind(employee.id)
    .bind(today)
    .fetch_one(&pool)
    .await
    .expect("attendance");
    assert_eq!(attendance, "sick_leave");

    cleanup_employee(&pool, &emp_code).await;
    cleanup_employee(&pool, &mgr_code).await;
}

#[tokio::test]
async fn manager_can_approve_ot_via_http() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let mgr_code = unique_code("OTMG");
    let emp_code = unique_code("OTEM");
    let manager = create_ready_employee(
        &pool,
        &mgr_code,
        "OT Manager",
        TEST_PIN,
        UserRole::Manager,
        None,
    )
    .await
    .expect("create manager");
    let employee = create_ready_employee(
        &pool,
        &emp_code,
        "OT Employee",
        TEST_PIN,
        UserRole::Employee,
        Some(manager.id),
    )
    .await
    .expect("create employee");

    let work_date = Date::from_calendar_date(2099, Month::March, 8).unwrap();
    let entry_id: Uuid = sqlx::query_scalar(
        "INSERT INTO time_entries
            (employee_id, work_date, regular_minutes, ot_minutes, ot_status, attendance, ot_reason)
         VALUES ($1, $2, 480, 45, 'pending', 'on_time', 'Project deadline')
         RETURNING id",
    )
    .bind(employee.id)
    .bind(work_date)
    .fetch_one(&pool)
    .await
    .expect("insert ot");

    let mut app = test_app(pool.clone()).await;
    let cookies = login_as(&mut app, &mgr_code, TEST_PIN).await;
    let (_, dashboard_html, cookies) = get(&mut app, "/manager", &cookies).await;
    let csrf = extract_csrf_token(&dashboard_html).expect("csrf");
    let review_path = format!("/manager/ot/{entry_id}/review");
    let body = format!("action=approve&csrf_token={csrf}");
    let (status, response, _) = post_form(&mut app, &review_path, &cookies, &body).await;
    assert_eq!(
        status,
        StatusCode::SEE_OTHER,
        "OT approve failed: {response}"
    );

    let ot_status: OtStatus =
        sqlx::query_scalar("SELECT ot_status::text FROM time_entries WHERE id = $1")
            .bind(entry_id)
            .fetch_one(&pool)
            .await
            .expect("ot status");
    assert_eq!(ot_status, OtStatus::Approved);

    cleanup_employee(&pool, &emp_code).await;
    cleanup_employee(&pool, &mgr_code).await;
}

#[tokio::test]
async fn manager_can_export_team_timesheet_csv_via_http() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let mgr_code = unique_code("TXMG");
    let emp_code = unique_code("TXEM");
    let manager = create_ready_employee(
        &pool,
        &mgr_code,
        "Timesheet Manager",
        TEST_PIN,
        UserRole::Manager,
        None,
    )
    .await
    .expect("create manager");
    let employee = create_ready_employee(
        &pool,
        &emp_code,
        "Timesheet Employee",
        TEST_PIN,
        UserRole::Employee,
        Some(manager.id),
    )
    .await
    .expect("create employee");

    let mut app = test_app(pool.clone()).await;
    let cookies = login_as(&mut app, &mgr_code, TEST_PIN).await;
    let path = format!("/manager/team/{}/export.csv", employee.id);
    let (status, body, headers) = get_bytes(&mut app, &path, &cookies).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "timesheet export failed for manager {mgr_code}"
    );
    let body_text = String::from_utf8_lossy(&body);
    assert!(body_text.contains(&emp_code));
    let content_type = common::header_value(&headers, "content-type").unwrap_or_default();
    assert!(
        content_type.contains("text/csv") || content_type.contains("octet-stream"),
        "unexpected content type: {content_type}"
    );

    cleanup_employee(&pool, &emp_code).await;
    cleanup_employee(&pool, &mgr_code).await;
}

#[tokio::test]
async fn employee_cannot_download_other_employees_requirement_file() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let code_a = unique_code("RQA");
    let code_b = unique_code("RQB");
    let employee_a = create_ready_employee(
        &pool,
        &code_a,
        "Requirement Owner",
        TEST_PIN,
        UserRole::Employee,
        None,
    )
    .await
    .expect("create employee a");
    let _employee_b = create_ready_employee(
        &pool,
        &code_b,
        "Requirement Intruder",
        TEST_PIN,
        UserRole::Employee,
        None,
    )
    .await
    .expect("create employee b");

    let req_type = create_type(&pool, "IDOR HTTP Doc", "IDOR test", true, false, 1, None)
        .await
        .expect("create type");

    let reqs = list_for_employee(&pool, employee_a.id)
        .await
        .expect("list requirements");
    let row = reqs
        .iter()
        .find(|r| r.requirement_type_id == req_type.id)
        .expect("seeded requirement");

    let mut app = test_app(pool.clone()).await;
    let cookies_a = login_as(&mut app, &code_a, TEST_PIN).await;
    let (_, requirements_html, cookies_a, _) =
        get_with_headers(&mut app, "/me/requirements", &cookies_a).await;
    let csrf = extract_csrf_token(&requirements_html).expect("csrf");
    let path = format!("/me/requirements/{}/submit", row.id);
    let (status, _, _) = post_multipart(
        &mut app,
        &path,
        &cookies_a,
        "idorboundary",
        &csrf,
        Some(("id.pdf", b"%PDF-1.4 idor test", "application/pdf")),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let cookies_b = login_as(&mut app, &code_b, TEST_PIN).await;
    let file_path = format!("/me/requirements/{}/file", row.id);
    let (status, _, _) = get(&mut app, &file_path, &cookies_b).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    cleanup_requirement_type(&pool, req_type.id).await;
    cleanup_employee(&pool, &code_a).await;
    cleanup_employee(&pool, &code_b).await;
}

#[tokio::test]
async fn manager_can_reject_leave_via_http() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let mgr_code = unique_code("LVRJ");
    let emp_code = unique_code("LVEJ");
    let manager = create_ready_employee(
        &pool,
        &mgr_code,
        "Leave Reject Manager",
        TEST_PIN,
        UserRole::Manager,
        None,
    )
    .await
    .expect("create manager");
    let employee = create_ready_employee(
        &pool,
        &emp_code,
        "Leave Reject Employee",
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
        "start_date={start}&end_date={end}&leave_type=offset&reason=Makeup%20hours&csrf_token={csrf}"
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
    let csrf = extract_csrf_token(&manager_leave_html).expect("csrf");
    let review_path = format!("/manager/leave/{request_id}/review");
    let body = format!("action=reject&note=Not%20approved&csrf_token={csrf}");
    let (status, _, _) = post_form(&mut app, &review_path, &mgr_cookies, &body).await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let status: String =
        sqlx::query_scalar("SELECT status::text FROM leave_requests WHERE id = $1")
            .bind(request_id)
            .fetch_one(&pool)
            .await
            .expect("leave status");
    assert_eq!(status, "rejected");

    cleanup_employee(&pool, &emp_code).await;
    cleanup_employee(&pool, &mgr_code).await;
}

#[tokio::test]
async fn admin_can_update_employee_via_http() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let admin_code = unique_code("ADUP");
    let emp_code = unique_code("EMUP");
    create_ready_employee(
        &pool,
        &admin_code,
        "Update Admin",
        TEST_PIN,
        UserRole::Admin,
        None,
    )
    .await
    .expect("create admin");
    let employee = create_ready_employee(
        &pool,
        &emp_code,
        "Before Update",
        TEST_PIN,
        UserRole::Employee,
        None,
    )
    .await
    .expect("create employee");

    let updated_name = "After HTTP Update";
    let mut app = test_app(pool.clone()).await;
    let cookies = login_as(&mut app, &admin_code, TEST_PIN).await;
    let edit_path = format!("/admin/employees/{}", employee.id);
    let (_, edit_html, cookies) = get(&mut app, &edit_path, &cookies).await;
    let csrf = extract_csrf_token(&edit_html).expect("csrf");
    let body = format!(
        "employee_code={emp_code}&full_name={}&role=employee&csrf_token={csrf}",
        updated_name.replace(' ', "%20")
    );
    let (status, _, _) = post_form(&mut app, &edit_path, &cookies, &body).await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let name: String = sqlx::query_scalar("SELECT full_name FROM employees WHERE id = $1")
        .bind(employee.id)
        .fetch_one(&pool)
        .await
        .expect("name");
    assert_eq!(name, updated_name);

    cleanup_employee(&pool, &emp_code).await;
    cleanup_employee(&pool, &admin_code).await;
}

#[tokio::test]
async fn admin_can_bulk_assign_department_via_http() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let admin_code = unique_code("BKDP");
    let emp_code = unique_code("BKEM");
    create_ready_employee(
        &pool,
        &admin_code,
        "Bulk Dept Admin",
        TEST_PIN,
        UserRole::Admin,
        None,
    )
    .await
    .expect("create admin");
    let employee = create_ready_employee(
        &pool,
        &emp_code,
        "Bulk Dept Employee",
        TEST_PIN,
        UserRole::Employee,
        None,
    )
    .await
    .expect("create employee");

    let department = format!("Engineering{}", &Uuid::new_v4().simple().to_string()[..4]);
    let mut app = test_app(pool.clone()).await;
    let cookies = login_as(&mut app, &admin_code, TEST_PIN).await;
    let (_, employees_html, cookies) = get(&mut app, "/admin/employees", &cookies).await;
    let csrf = extract_csrf_token(&employees_html).expect("csrf");
    let body = format!(
        "department={}&employee_ids={}&csrf_token={csrf}",
        department, employee.id
    );
    let (status, _, _) = post_form(
        &mut app,
        "/admin/employees/bulk-department",
        &cookies,
        &body,
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let saved: Option<String> =
        sqlx::query_scalar("SELECT department FROM employee_profiles WHERE employee_id = $1")
            .bind(employee.id)
            .fetch_one(&pool)
            .await
            .expect("department");
    assert_eq!(saved.as_deref(), Some(department.as_str()));

    cleanup_employee(&pool, &emp_code).await;
    cleanup_employee(&pool, &admin_code).await;
}

#[tokio::test]
async fn manager_can_reject_ot_via_http() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let mgr_code = unique_code("OTRJ");
    let emp_code = unique_code("OTER");
    let manager = create_ready_employee(
        &pool,
        &mgr_code,
        "OT Reject Manager",
        TEST_PIN,
        UserRole::Manager,
        None,
    )
    .await
    .expect("create manager");
    let employee = create_ready_employee(
        &pool,
        &emp_code,
        "OT Reject Employee",
        TEST_PIN,
        UserRole::Employee,
        Some(manager.id),
    )
    .await
    .expect("create employee");

    let work_date = Date::from_calendar_date(2099, Month::March, 9).unwrap();
    let entry_id: Uuid = sqlx::query_scalar(
        "INSERT INTO time_entries
            (employee_id, work_date, regular_minutes, ot_minutes, ot_status, attendance, ot_reason)
         VALUES ($1, $2, 480, 30, 'pending', 'on_time', 'Unplanned stay')
         RETURNING id",
    )
    .bind(employee.id)
    .bind(work_date)
    .fetch_one(&pool)
    .await
    .expect("insert ot");

    let mut app = test_app(pool.clone()).await;
    let cookies = login_as(&mut app, &mgr_code, TEST_PIN).await;
    let (_, dashboard_html, cookies) = get(&mut app, "/manager", &cookies).await;
    let csrf = extract_csrf_token(&dashboard_html).expect("csrf");
    let review_path = format!("/manager/ot/{entry_id}/review");
    let body = format!("action=reject&note=Not%20needed&csrf_token={csrf}");
    let (status, response, _) = post_form(&mut app, &review_path, &cookies, &body).await;
    assert_eq!(
        status,
        StatusCode::SEE_OTHER,
        "OT reject failed: {response}"
    );

    let ot_status: OtStatus =
        sqlx::query_scalar("SELECT ot_status::text FROM time_entries WHERE id = $1")
            .bind(entry_id)
            .fetch_one(&pool)
            .await
            .expect("ot status");
    assert_eq!(ot_status, OtStatus::Rejected);

    cleanup_employee(&pool, &emp_code).await;
    cleanup_employee(&pool, &mgr_code).await;
}

#[tokio::test]
async fn change_pin_success_via_http() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let code = unique_code("PINS");
    let new_pin = "593847";
    create_ready_employee(
        &pool,
        &code,
        "Change PIN Success",
        TEST_PIN,
        UserRole::Employee,
        None,
    )
    .await
    .expect("create employee");

    let mut app = test_app(pool.clone()).await;
    let cookies = login_as(&mut app, &code, TEST_PIN).await;
    let (_, change_html, cookies) = get(&mut app, "/change-pin", &cookies).await;
    let csrf = extract_csrf_token(&change_html).expect("csrf");
    let body =
        format!("current_pin={TEST_PIN}&new_pin={new_pin}&confirm_pin={new_pin}&csrf_token={csrf}");
    let (status, _, _) = post_form(&mut app, "/change-pin", &cookies, &body).await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let cookies = login_as(&mut app, &code, new_pin).await;
    let (status, body, _) = get(&mut app, "/", &cookies).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Clock In / Out"));

    cleanup_employee(&pool, &code).await;
}

#[tokio::test]
async fn change_pin_locks_after_repeated_wrong_current_pin() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let code = unique_code("PINL");
    create_ready_employee(
        &pool,
        &code,
        "Change PIN Lock",
        TEST_PIN,
        UserRole::Employee,
        None,
    )
    .await
    .expect("create employee");

    let mut app = test_app(pool.clone()).await;
    let cookies = login_as(&mut app, &code, TEST_PIN).await;
    let (_, change_html, mut cookies) = get(&mut app, "/change-pin", &cookies).await;
    let csrf = extract_csrf_token(&change_html).expect("csrf");

    for _ in 0..5 {
        let body =
            format!("current_pin=000000&new_pin=593847&confirm_pin=593847&csrf_token={csrf}");
        let (status, response, updated_cookies) =
            post_form(&mut app, "/change-pin", &cookies, &body).await;
        assert_eq!(status, StatusCode::OK);
        assert!(response.contains("Current PIN is incorrect"));
        cookies = updated_cookies;
    }

    let body =
        format!("current_pin={TEST_PIN}&new_pin=593847&confirm_pin=593847&csrf_token={csrf}");
    let (status, response, _) = post_form(&mut app, "/change-pin", &cookies, &body).await;
    assert_eq!(status, StatusCode::OK);
    assert!(response.contains("Too many failed PIN change attempts"));

    cleanup_employee(&pool, &code).await;
}

#[tokio::test]
async fn admin_can_reset_pin_via_http() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let admin_code = unique_code("RPNAD");
    let emp_code = unique_code("RPNEM");
    create_ready_employee(
        &pool,
        &admin_code,
        "Reset PIN Admin",
        TEST_PIN,
        UserRole::Admin,
        None,
    )
    .await
    .expect("create admin");
    let employee = create_ready_employee(
        &pool,
        &emp_code,
        "Reset PIN Employee",
        TEST_PIN,
        UserRole::Employee,
        None,
    )
    .await
    .expect("create employee");

    let new_pin = "593847";
    let mut app = test_app(pool.clone()).await;
    let cookies = login_as(&mut app, &admin_code, TEST_PIN).await;
    let edit_path = format!("/admin/employees/{}", employee.id);
    let (status, edit_html, cookies) = get(&mut app, &edit_path, &cookies).await;
    let csrf = expect_csrf_token(&edit_path, status, &edit_html);
    let reset_path = format!("/admin/employees/{}/reset-pin", employee.id);
    let body = format!("new_pin={new_pin}&csrf_token={csrf}");
    let (status, _, _) = post_form(&mut app, &reset_path, &cookies, &body).await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let must_change: bool =
        sqlx::query_scalar("SELECT must_change_pin FROM employees WHERE id = $1")
            .bind(employee.id)
            .fetch_one(&pool)
            .await
            .expect("must_change_pin");
    assert!(must_change);

    cleanup_employee(&pool, &emp_code).await;
    cleanup_employee(&pool, &admin_code).await;
}

#[tokio::test]
async fn admin_can_toggle_active_via_http() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let admin_code = unique_code("TGLAD");
    let emp_code = unique_code("TGLEM");
    create_ready_employee(
        &pool,
        &admin_code,
        "Toggle Active Admin",
        TEST_PIN,
        UserRole::Admin,
        None,
    )
    .await
    .expect("create admin");
    let employee = create_ready_employee(
        &pool,
        &emp_code,
        "Toggle Active Employee",
        TEST_PIN,
        UserRole::Employee,
        None,
    )
    .await
    .expect("create employee");

    let mut app = test_app(pool.clone()).await;
    let cookies = login_as(&mut app, &admin_code, TEST_PIN).await;
    let edit_path = format!("/admin/employees/{}", employee.id);
    let (_, edit_html, cookies) = get(&mut app, &edit_path, &cookies).await;
    let csrf = extract_csrf_token(&edit_html).expect("csrf");
    let toggle_path = format!("/admin/employees/{}/toggle-active", employee.id);
    let (status, _, cookies) = post_form(
        &mut app,
        &toggle_path,
        &cookies,
        &format!("csrf_token={csrf}"),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let is_active: bool = sqlx::query_scalar("SELECT is_active FROM employees WHERE id = $1")
        .bind(employee.id)
        .fetch_one(&pool)
        .await
        .expect("is_active");
    assert!(!is_active);

    let (_, edit_html, cookies) = get(&mut app, &edit_path, &cookies).await;
    let csrf = extract_csrf_token(&edit_html).expect("csrf");
    let (status, _, _) = post_form(
        &mut app,
        &toggle_path,
        &cookies,
        &format!("csrf_token={csrf}"),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let is_active: bool = sqlx::query_scalar("SELECT is_active FROM employees WHERE id = $1")
        .bind(employee.id)
        .fetch_one(&pool)
        .await
        .expect("is_active");
    assert!(is_active);

    cleanup_employee(&pool, &emp_code).await;
    cleanup_employee(&pool, &admin_code).await;
}

#[tokio::test]
async fn employee_can_save_and_submit_eod_via_http() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let code = unique_code("EODS");
    let employee = create_ready_employee(
        &pool,
        &code,
        "EOD Save Test",
        TEST_PIN,
        UserRole::Employee,
        None,
    )
    .await
    .expect("create employee");

    clock_in(&pool, employee.id).await.expect("clock in");

    let mut app = test_app(pool.clone()).await;
    let cookies = login_as(&mut app, &code, TEST_PIN).await;
    let (_, eod_html, cookies) = get(&mut app, "/me/eod", &cookies).await;
    let csrf = extract_csrf_token(&eod_html).expect("csrf");
    let draft_body =
        format!("completed=Draft%20task&summary=Draft%20summary&action=draft&csrf_token={csrf}");
    let (status, _, cookies) = post_form(&mut app, "/me/eod", &cookies, &draft_body).await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let status: EodReportStatus =
        sqlx::query_scalar("SELECT status::text FROM eod_reports WHERE employee_id = $1")
            .bind(employee.id)
            .fetch_one(&pool)
            .await
            .expect("eod status");
    assert_eq!(status, EodReportStatus::Draft);

    let (_, eod_html, cookies) = get(&mut app, "/me/eod", &cookies).await;
    let csrf = extract_csrf_token(&eod_html).expect("csrf");
    let submit_body = format!(
        "completed=Finished%20feature&summary=Shipped%20update&action=submit&csrf_token={csrf}"
    );
    let (status, _, _) = post_form(&mut app, "/me/eod", &cookies, &submit_body).await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let status: EodReportStatus =
        sqlx::query_scalar("SELECT status::text FROM eod_reports WHERE employee_id = $1")
            .bind(employee.id)
            .fetch_one(&pool)
            .await
            .expect("eod status");
    assert_eq!(status, EodReportStatus::Submitted);

    cleanup_employee(&pool, &code).await;
}

#[tokio::test]
async fn admin_can_unlock_eod_via_http() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let admin_code = unique_code("EODAD");
    let emp_code = unique_code("EODEM");
    create_ready_employee(
        &pool,
        &admin_code,
        "EOD Unlock Admin",
        TEST_PIN,
        UserRole::Admin,
        None,
    )
    .await
    .expect("create admin");
    let employee = create_ready_employee(
        &pool,
        &emp_code,
        "EOD Unlock Employee",
        TEST_PIN,
        UserRole::Employee,
        None,
    )
    .await
    .expect("create employee");

    clock_in(&pool, employee.id).await.expect("clock in");

    let mut app = test_app(pool.clone()).await;
    let emp_cookies = login_as(&mut app, &emp_code, TEST_PIN).await;
    let (_, eod_html, emp_cookies) = get(&mut app, "/me/eod", &emp_cookies).await;
    let csrf = extract_csrf_token(&eod_html).expect("csrf");
    let body = format!(
        "completed=Needs%20unlock&summary=Submitted%20for%20unlock%20test&action=submit&csrf_token={csrf}"
    );
    let (status, _, _) = post_form(&mut app, "/me/eod", &emp_cookies, &body).await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let report_id: Uuid = sqlx::query_scalar("SELECT id FROM eod_reports WHERE employee_id = $1")
        .bind(employee.id)
        .fetch_one(&pool)
        .await
        .expect("report id");

    let admin_cookies = login_as(&mut app, &admin_code, TEST_PIN).await;
    let (_, admin_eod_html, admin_cookies) = get(&mut app, "/admin/eod", &admin_cookies).await;
    let csrf = extract_csrf_token(&admin_eod_html).expect("csrf");
    let unlock_path = format!("/admin/eod/{report_id}/unlock");
    let (status, _, _) = post_form(
        &mut app,
        &unlock_path,
        &admin_cookies,
        &format!("csrf_token={csrf}"),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let status: EodReportStatus =
        sqlx::query_scalar("SELECT status::text FROM eod_reports WHERE id = $1")
            .bind(report_id)
            .fetch_one(&pool)
            .await
            .expect("eod status");
    assert_eq!(status, EodReportStatus::Draft);

    cleanup_employee(&pool, &emp_code).await;
    cleanup_employee(&pool, &admin_code).await;
}

#[tokio::test]
async fn employee_can_download_own_requirement_file_via_http() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let code = unique_code("DLRQ");
    let employee = create_ready_employee(
        &pool,
        &code,
        "Requirement Download",
        TEST_PIN,
        UserRole::Employee,
        None,
    )
    .await
    .expect("create employee");

    let req_type = create_type(
        &pool,
        "Download HTTP Doc",
        "Download test",
        true,
        false,
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

    let pdf_bytes = b"%PDF-1.4 download test content";
    let mut app = test_app(pool.clone()).await;
    let cookies = login_as(&mut app, &code, TEST_PIN).await;
    let (_, requirements_html, cookies, _) =
        get_with_headers(&mut app, "/me/requirements", &cookies).await;
    let csrf = extract_csrf_token(&requirements_html).expect("csrf");
    let submit_path = format!("/me/requirements/{}/submit", row.id);
    let (status, _, cookies) = post_multipart(
        &mut app,
        &submit_path,
        &cookies,
        "dlboundary",
        &csrf,
        Some(("proof.pdf", pdf_bytes, "application/pdf")),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let file_path = format!("/me/requirements/{}/file", row.id);
    let (status, body, headers) = get_bytes(&mut app, &file_path, &cookies).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(&body[..8], b"%PDF-1.4");
    let content_type = common::header_value(&headers, "content-type").unwrap_or_default();
    assert!(
        content_type.contains("application/pdf"),
        "unexpected content type: {content_type}"
    );

    cleanup_requirement_type(&pool, req_type.id).await;
    cleanup_employee(&pool, &code).await;
}

#[tokio::test]
async fn admin_can_save_compensation_via_http() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let admin_code = unique_code("CMPAD");
    let emp_code = unique_code("CMPEM");
    create_ready_employee(
        &pool,
        &admin_code,
        "Compensation Admin",
        TEST_PIN,
        UserRole::Admin,
        None,
    )
    .await
    .expect("create admin");
    let employee = create_ready_employee(
        &pool,
        &emp_code,
        "Compensation Employee",
        TEST_PIN,
        UserRole::Employee,
        None,
    )
    .await
    .expect("create employee");

    let mut app = test_app(pool.clone()).await;
    let cookies = login_as(&mut app, &admin_code, TEST_PIN).await;
    let path = format!("/admin/employees/{}/compensation", employee.id);
    let (status, comp_html, cookies) = get(&mut app, &path, &cookies).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "GET {path} failed: {status}; body: {}",
        comp_html.chars().take(300).collect::<String>()
    );
    assert!(comp_html.contains("Monthly salary"));
    let csrf = extract_csrf_token(&comp_html).expect("csrf");
    let body = format!(
        "monthly_salary=26000.00&ot_rate_percent=132&effective_from=2026-01-01&csrf_token={csrf}"
    );
    let (status, _, _) = post_form(&mut app, &path, &cookies, &body).await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let salary_cents: i64 = sqlx::query_scalar(
        "SELECT monthly_salary_cents FROM compensation_profiles WHERE employee_id = $1",
    )
    .bind(employee.id)
    .fetch_one(&pool)
    .await
    .expect("salary");
    assert_eq!(salary_cents, 2_600_000);

    let emp_cookies = login_as(&mut app, &emp_code, TEST_PIN).await;
    let (status, _, _) = get(&mut app, &path, &emp_cookies).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let _ = sqlx::query("DELETE FROM compensation_profiles WHERE employee_id = $1")
        .bind(employee.id)
        .execute(&pool)
        .await;
    cleanup_employee(&pool, &emp_code).await;
    cleanup_employee(&pool, &admin_code).await;
}

#[tokio::test]
async fn admin_can_finalize_payroll_run_via_http() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping http test: DATABASE_URL not available");
        return;
    };

    let admin_code = unique_code("PYHT");
    let emp_code = unique_code("PYHE");
    let admin = create_ready_employee(
        &pool,
        &admin_code,
        "Payroll HTTP Admin",
        TEST_PIN,
        UserRole::Admin,
        None,
    )
    .await
    .expect("create admin");
    let _employee = create_ready_employee(
        &pool,
        &emp_code,
        "Payroll HTTP Employee",
        TEST_PIN,
        UserRole::Employee,
        None,
    )
    .await
    .expect("create employee");

    let settings = get_settings(&pool).await.expect("settings");
    let anchor = Date::from_calendar_date(2099, Month::June, 10).unwrap();
    let (period_start, period_end, _) =
        current_pay_period(anchor, settings.pay_period, settings.pay_period_anchor);
    let effective = Date::from_calendar_date(2026, Month::January, 1).unwrap();

    let active_ids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT e.id FROM employees e
         LEFT JOIN compensation_profiles c ON c.employee_id = e.id
         WHERE e.is_active = TRUE AND c.employee_id IS NULL",
    )
    .fetch_all(&pool)
    .await
    .expect("missing comp ids");
    for id in active_ids {
        dtr::services::compensation::upsert_profile(
            &pool,
            &dtr::services::compensation::UpsertProfileInput::new(
                id, 1_000_000, effective, admin.id,
            ),
        )
        .await
        .expect("upsert comp");
    }

    dtr::services::payroll_controls::close_pay_period(
        &pool,
        period_start,
        period_end,
        admin.id,
        Some("http payroll test"),
    )
    .await
    .expect("close period");

    let mut app = test_app(pool.clone()).await;
    let cookies = login_as(&mut app, &admin_code, TEST_PIN).await;
    let (status, payroll_html, cookies) = get(&mut app, "/admin/payroll", &cookies).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "GET /admin/payroll failed: {status}; body: {}",
        payroll_html.chars().take(300).collect::<String>()
    );
    assert!(payroll_html.contains("Create draft"));
    let csrf = extract_csrf_token(&payroll_html).expect("csrf");
    let start = format_date(period_start);
    let end = format_date(period_end);
    let body = format!("period_start={start}&period_end={end}&csrf_token={csrf}");
    let (status, _, cookies) = post_form(&mut app, "/admin/payroll", &cookies, &body).await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let run_id: Uuid = sqlx::query_scalar(
        "SELECT id FROM payroll_runs WHERE period_start = $1 AND period_end = $2 ORDER BY created_at DESC LIMIT 1",
    )
    .bind(period_start)
    .bind(period_end)
    .fetch_one(&pool)
    .await
    .expect("run id");

    let _ = sqlx::query("UPDATE payroll_lines SET pending_ot_minutes = 0 WHERE run_id = $1")
        .bind(run_id)
        .execute(&pool)
        .await;

    let run_path = format!("/admin/payroll/{run_id}");
    let (_, run_html, cookies) = get(&mut app, &run_path, &cookies).await;
    assert!(run_html.contains("Finalize run"));
    assert!(run_html.contains("Deductions"));

    let line_id: Uuid = sqlx::query_scalar(
        "SELECT l.id FROM payroll_lines l
         JOIN employees e ON e.id = l.employee_id
         WHERE l.run_id = $1 AND e.employee_code = $2",
    )
    .bind(run_id)
    .bind(emp_code.to_uppercase())
    .fetch_one(&pool)
    .await
    .expect("line id");

    let deductions_path = format!("/admin/payroll/{run_id}/lines/{line_id}");
    let (_, deductions_html, cookies) = get(&mut app, &deductions_path, &cookies).await;
    assert!(deductions_html.contains("Manual deductions"));
    let csrf = extract_csrf_token(&deductions_html).expect("csrf");
    let body = format!("amount_sss=500.00&note_sss=HTTP+test&csrf_token={csrf}");
    let (status, _, cookies) = post_form(&mut app, &deductions_path, &cookies, &body).await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let net_pay: i64 = sqlx::query_scalar("SELECT net_pay_cents FROM payroll_lines WHERE id = $1")
        .bind(line_id)
        .fetch_one(&pool)
        .await
        .expect("net pay");
    let gross_pay: i64 =
        sqlx::query_scalar("SELECT gross_pay_cents FROM payroll_lines WHERE id = $1")
            .bind(line_id)
            .fetch_one(&pool)
            .await
            .expect("gross pay");
    assert_eq!(net_pay, gross_pay - 50_000);

    let (_, run_html, cookies) = get(&mut app, &run_path, &cookies).await;
    let csrf = extract_csrf_token(&run_html).expect("csrf");
    let finalize_path = format!("/admin/payroll/{run_id}/finalize");
    let (status, _, _) = post_form(
        &mut app,
        &finalize_path,
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
    assert_eq!(status, "finalized");

    let export_path = format!("/admin/payroll/{run_id}/export.csv");
    let (status, export_body, _) = get(&mut app, &export_path, &cookies).await;
    assert_eq!(status, StatusCode::OK);
    assert!(export_body.contains("Employee Code"));
    assert!(export_body.contains("Net Pay"));
    assert!(export_body.contains(&emp_code.to_uppercase()));

    let payslip_path = format!("/admin/payroll/{run_id}/lines/{line_id}/payslip");
    let (_, payslip_html, _) = get(&mut app, &payslip_path, &cookies).await;
    assert!(payslip_html.contains("Employee Payslip"));
    assert!(payslip_html.contains("Net pay"));

    let emp_cookies = login_as(&mut app, &emp_code, TEST_PIN).await;
    let (_, list_html, emp_cookies) = get(&mut app, "/me/payslips", &emp_cookies).await;
    assert!(list_html.contains("My Payslips"));
    let (_, payslip_html, _) =
        get(&mut app, &format!("/me/payslips/{line_id}"), &emp_cookies).await;
    assert!(payslip_html.contains("Gross pay"));
    assert!(payslip_html.contains("Net pay"));

    let other_code = unique_code("PYHO");
    create_ready_employee(
        &pool,
        &other_code,
        "Other Payslip Employee",
        TEST_PIN,
        UserRole::Employee,
        None,
    )
    .await
    .expect("other employee");
    let other_cookies = login_as(&mut app, &other_code, TEST_PIN).await;
    let (status, _, _) = get(&mut app, &format!("/me/payslips/{line_id}"), &other_cookies).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let _ = sqlx::query("DELETE FROM payroll_lines WHERE run_id = $1")
        .bind(run_id)
        .execute(&pool)
        .await;
    let _ = sqlx::query("DELETE FROM payroll_runs WHERE id = $1")
        .bind(run_id)
        .execute(&pool)
        .await;
    let _ =
        sqlx::query("DELETE FROM closed_pay_periods WHERE period_start = $1 AND period_end = $2")
            .bind(period_start)
            .bind(period_end)
            .execute(&pool)
            .await;
    cleanup_employee(&pool, &other_code).await;
    cleanup_employee(&pool, &emp_code).await;
    cleanup_employee(&pool, &admin_code).await;
}
