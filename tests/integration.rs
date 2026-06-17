use dtr::auth::UserSession;
use dtr::db;
use dtr::error::AppError;
use dtr::models::AttendanceStatus;
use dtr::models::PayPeriodType;
use dtr::models::RequirementStatus;
use dtr::models::{OtStatus, UserRole};
use dtr::services::attendance::mark_absence_for_employee;
use dtr::services::audit::{list_audit_logs, log_action, AuditLogQuery};
use dtr::services::clock::{clock_in, clock_out, ot_status_for_minutes};
use dtr::services::compensation::{get_compensation, upsert_profile};
use dtr::services::corrections::{
    create_corrected_entry, list_correction_logs, CorrectionLogQuery, CorrectionSubmission,
};
use dtr::services::employees::create_employee;
use dtr::services::eod::{
    list_department_eod, list_employee_eod_history, needs_eod_reminder, save_report, unlock_report,
    EodTaskInput,
};
use dtr::services::holidays::{add_holiday, is_holiday};
use dtr::services::hours::calculate;
use dtr::services::notifications::list_for_user;
use dtr::services::onboarding::{
    bulk_assign_department, list_admin_employee_rows, profile_completeness_pct, AdminEmployeeQuery,
};
use dtr::services::ot::review_overtime;
use dtr::services::payroll::{gross_pay_cents, GrossPayInput};
use dtr::services::profile::{get_profile, update_admin, update_self_service, AdminProfileInput};
use dtr::services::reports::{payroll_summary, PayrollFilters};
use dtr::services::requirements::{
    can_submit_requirement, create_type, is_requirement_expired, list_for_employee,
    review_requirement, submit_requirement,
};
use dtr::services::settings::{get_settings, update_settings, SettingsUpdate};

use dtr::models::EodTaskKind;
use dtr::models::LeaveRequestType;
use dtr::models::PayrollRunStatus;
use dtr::services::leave::{create_request, review_request};
use dtr::services::payroll::{create_draft_run, finalize_run, get_run, list_lines_for_run};
use dtr::services::payroll_controls::{close_pay_period, reopen_pay_period, ClosePayPeriodResult};
use dtr::services::timezone::{combine_date_time, company_date_now, now_company};
use sqlx::PgPool;
use time::{Date, Month, Time};
use uuid::Uuid;

async fn try_pool() -> Option<PgPool> {
    dotenvy::dotenv().ok();
    let url = std::env::var("DATABASE_URL").ok()?;
    let pool = db::connect(&url).await.ok()?;
    db::migrate(&pool).await.ok()?;
    Some(pool)
}

fn unique_code(prefix: &str) -> String {
    format!("{prefix}{}", &Uuid::new_v4().simple().to_string()[..8])
}

async fn cleanup_employee(pool: &PgPool, code: &str) {
    let _ = sqlx::query(
        "DELETE FROM correction_logs WHERE time_entry_id IN (
            SELECT id FROM time_entries WHERE employee_id IN (
                SELECT id FROM employees WHERE employee_code = $1
            )
        )",
    )
    .bind(code)
    .execute(pool)
    .await;

    let _ = sqlx::query(
        "DELETE FROM time_entries WHERE employee_id IN (
            SELECT id FROM employees WHERE employee_code = $1
        )",
    )
    .bind(code)
    .execute(pool)
    .await;

    let _ = sqlx::query("DELETE FROM employees WHERE employee_code = $1")
        .bind(code)
        .execute(pool)
        .await;
}

#[tokio::test]
async fn payroll_summary_totals_regular_and_approved_ot() {
    let Some(pool) = try_pool().await else {
        eprintln!("skipping integration test: DATABASE_URL not available");
        return;
    };

    let code = unique_code("PAY");
    let employee = create_employee(
        &pool,
        &code,
        "Payroll Test",
        "482915",
        UserRole::Employee,
        None,
    )
    .await
    .expect("create employee");

    let work_date = Date::from_calendar_date(2026, Month::March, 10).unwrap();
    let settings = get_settings(&pool).await.expect("settings");
    let tz = settings.timezone.as_str();
    let clock_in =
        combine_date_time(work_date, Time::from_hms(8, 0, 0).unwrap(), tz).expect("clock in");
    let clock_out =
        combine_date_time(work_date, Time::from_hms(19, 0, 0).unwrap(), tz).expect("clock out");
    let breakdown = calculate(clock_in, clock_out, &settings);

    sqlx::query(
        "INSERT INTO time_entries
            (employee_id, work_date, clock_in, clock_out, gross_minutes, net_minutes,
             regular_minutes, ot_minutes, ot_status, attendance)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 'approved', 'on_time')",
    )
    .bind(employee.id)
    .bind(work_date)
    .bind(clock_in)
    .bind(clock_out)
    .bind(breakdown.gross_minutes)
    .bind(breakdown.net_minutes)
    .bind(breakdown.regular_minutes)
    .bind(breakdown.ot_minutes)
    .execute(&pool)
    .await
    .expect("insert entry");

    let rows = payroll_summary(&pool, work_date, work_date, &PayrollFilters::default())
        .await
        .expect("payroll");
    let row = rows
        .iter()
        .find(|r| r.employee_code == code)
        .expect("employee row");

    assert_eq!(row.regular_minutes, 480);
    assert_eq!(row.approved_ot_minutes, 120);

    cleanup_employee(&pool, &code).await;
}

#[tokio::test]
async fn ot_approval_moves_pending_to_payable() {
    let Some(pool) = try_pool().await else {
        eprintln!("skipping integration test: DATABASE_URL not available");
        return;
    };

    let mgr_code = unique_code("MGR");
    let emp_code = unique_code("EMP");
    let manager = create_employee(
        &pool,
        &mgr_code,
        "OT Manager",
        "482915",
        UserRole::Manager,
        None,
    )
    .await
    .expect("manager");
    let employee = create_employee(
        &pool,
        &emp_code,
        "OT Employee",
        "482915",
        UserRole::Employee,
        Some(manager.id),
    )
    .await
    .expect("employee");

    let work_date = Date::from_calendar_date(2026, Month::April, 5).unwrap();
    let entry_id: Uuid = sqlx::query_scalar(
        "INSERT INTO time_entries
            (employee_id, work_date, regular_minutes, ot_minutes, ot_status, attendance)
         VALUES ($1, $2, 480, 60, 'pending', 'on_time')
         RETURNING id",
    )
    .bind(employee.id)
    .bind(work_date)
    .fetch_one(&pool)
    .await
    .expect("insert");

    let before = payroll_summary(&pool, work_date, work_date, &PayrollFilters::default())
        .await
        .expect("payroll");
    let before_row = before.iter().find(|r| r.employee_code == emp_code).unwrap();
    assert_eq!(before_row.approved_ot_minutes, 0);
    assert_eq!(before_row.pending_ot_minutes, 60);

    review_overtime(&pool, entry_id, manager.id, true, None, false)
        .await
        .expect("approve");

    let after = payroll_summary(&pool, work_date, work_date, &PayrollFilters::default())
        .await
        .expect("payroll");
    let after_row = after.iter().find(|r| r.employee_code == emp_code).unwrap();
    assert_eq!(after_row.approved_ot_minutes, 60);
    assert_eq!(after_row.pending_ot_minutes, 0);

    cleanup_employee(&pool, &emp_code).await;
    cleanup_employee(&pool, &mgr_code).await;
}

#[tokio::test]
async fn correction_creates_audit_log_entry() {
    let Some(pool) = try_pool().await else {
        eprintln!("skipping integration test: DATABASE_URL not available");
        return;
    };

    let mgr_code = unique_code("MGR");
    let emp_code = unique_code("EMP");
    let manager = create_employee(
        &pool,
        &mgr_code,
        "Correction Manager",
        "482915",
        UserRole::Manager,
        None,
    )
    .await
    .expect("manager");
    let employee = create_employee(
        &pool,
        &emp_code,
        "Correction Employee",
        "482915",
        UserRole::Employee,
        Some(manager.id),
    )
    .await
    .expect("employee");

    let work_date = Date::from_calendar_date(2026, Month::May, 12).unwrap();
    let settings = get_settings(&pool).await.expect("settings");
    let tz = settings.timezone.as_str();
    let clock_in =
        combine_date_time(work_date, Time::from_hms(8, 0, 0).unwrap(), tz).expect("clock in");
    let clock_out =
        combine_date_time(work_date, Time::from_hms(17, 0, 0).unwrap(), tz).expect("clock out");

    create_corrected_entry(
        &pool,
        employee.id,
        work_date,
        &CorrectionSubmission {
            editor_id: manager.id,
            manager_id: manager.id,
            is_admin: false,
            new_clock_in: clock_in,
            new_clock_out: clock_out,
            reason: "Missed clock-in",
        },
    )
    .await
    .expect("correction");

    let logs = list_correction_logs(
        &pool,
        &CorrectionLogQuery {
            search: None,
            limit: 20,
            offset: 0,
        },
    )
    .await
    .expect("logs");
    assert!(logs
        .iter()
        .any(|log| { log.employee_code == emp_code && log.reason == "Missed clock-in" }));

    cleanup_employee(&pool, &emp_code).await;
    cleanup_employee(&pool, &mgr_code).await;
}

#[tokio::test]
async fn admin_audit_log_records_actions() {
    let Some(pool) = try_pool().await else {
        eprintln!("skipping integration test: DATABASE_URL not available");
        return;
    };

    let admin_code = unique_code("AUD");
    let admin = create_employee(
        &pool,
        &admin_code,
        "Audit Admin",
        "482915",
        UserRole::Admin,
        None,
    )
    .await
    .expect("create admin");

    log_action(
        &pool,
        admin.id,
        "settings.updated",
        "Updated company settings for Test Co",
    )
    .await
    .expect("log action");

    let logs = list_audit_logs(
        &pool,
        &AuditLogQuery {
            search: None,
            limit: 10,
            offset: 0,
        },
    )
    .await
    .expect("audit logs");
    assert!(logs.iter().any(|log| {
        log.actor_code == admin_code
            && log.action == "settings.updated"
            && log.summary.contains("Test Co")
    }));

    let _ = sqlx::query("DELETE FROM admin_audit_logs WHERE actor_id = $1")
        .bind(admin.id)
        .execute(&pool)
        .await;
    cleanup_employee(&pool, &admin_code).await;
}

#[tokio::test]
async fn employee_profile_self_service_updates_contact_fields() {
    let Some(pool) = try_pool().await else {
        eprintln!("skipping integration test: DATABASE_URL not available");
        return;
    };

    let code = unique_code("PRF");
    let employee = create_employee(
        &pool,
        &code,
        "Profile Test",
        "482915",
        UserRole::Employee,
        None,
    )
    .await
    .expect("create employee");

    update_self_service(
        &pool,
        employee.id,
        Some("09171234567"),
        Some("me@example.com"),
    )
    .await
    .expect("update self");

    let profile = get_profile(&pool, employee.id).await.expect("profile");
    assert_eq!(profile.contact_number.as_deref(), Some("09171234567"));
    assert_eq!(profile.personal_email.as_deref(), Some("me@example.com"));

    cleanup_employee(&pool, &code).await;
}

#[tokio::test]
async fn eod_required_after_clock_in_and_visible_by_department() {
    let Some(pool) = try_pool().await else {
        eprintln!("skipping integration test: DATABASE_URL not available");
        return;
    };

    let code_a = unique_code("EOD");
    let code_b = unique_code("EOD");
    let a = create_employee(
        &pool,
        &code_a,
        "EOD Alice",
        "482915",
        UserRole::Employee,
        None,
    )
    .await
    .expect("create a");
    let b = create_employee(
        &pool,
        &code_b,
        "EOD Bob",
        "482915",
        UserRole::Employee,
        None,
    )
    .await
    .expect("create b");

    update_admin(
        &pool,
        a.id,
        a.id,
        AdminProfileInput {
            contact_number: None,
            personal_email: None,
            birthdate: None,
            address: None,
            emergency_contact_name: None,
            emergency_contact_phone: None,
            job_title: Some("Engineer"),
            department: Some("Engineering"),
            employment_type: None,
            date_hired: None,
            work_location: None,
        },
    )
    .await
    .expect("profile a");
    update_admin(
        &pool,
        b.id,
        b.id,
        AdminProfileInput {
            contact_number: None,
            personal_email: None,
            birthdate: None,
            address: None,
            emergency_contact_name: None,
            emergency_contact_phone: None,
            job_title: Some("Engineer"),
            department: Some("Engineering"),
            employment_type: None,
            date_hired: None,
            work_location: None,
        },
    )
    .await
    .expect("profile b");

    assert!(!needs_eod_reminder(&pool, a.id).await.expect("reminder"));

    clock_in(&pool, a.id).await.expect("clock in");
    assert!(needs_eod_reminder(&pool, a.id).await.expect("reminder on"));

    let settings = get_settings(&pool).await.expect("settings");
    let today = company_date_now(&settings).expect("today");
    save_report(
        &pool,
        a.id,
        today,
        "Good day",
        true,
        &[EodTaskInput {
            kind: EodTaskKind::Completed,
            title: "Shipped feature".into(),
        }],
    )
    .await
    .expect("save eod");

    assert!(!needs_eod_reminder(&pool, a.id).await.expect("reminder off"));

    let locked = save_report(
        &pool,
        a.id,
        today,
        "Changed mind",
        false,
        &[EodTaskInput {
            kind: EodTaskKind::Completed,
            title: "Should fail".into(),
        }],
    )
    .await;
    assert!(locked.is_err());

    let visible = list_department_eod(&pool, b.id, "Engineering", today)
        .await
        .expect("dept eod");
    assert!(visible.iter().any(|r| r.employee_code == code_a));

    cleanup_employee(&pool, &code_a).await;
    cleanup_employee(&pool, &code_b).await;
}

#[tokio::test]
async fn requirement_checklist_submit_flow() {
    let Some(pool) = try_pool().await else {
        eprintln!("skipping integration test: DATABASE_URL not available");
        return;
    };

    let code = unique_code("REQ");
    let employee = create_employee(&pool, &code, "Req Test", "482915", UserRole::Employee, None)
        .await
        .expect("create employee");

    let req_type = create_type(
        &pool,
        "Test Document",
        "Bring original",
        true,
        false,
        1,
        None,
    )
    .await
    .expect("create type");

    let reqs = list_for_employee(&pool, employee.id).await.expect("list");
    let row = reqs
        .iter()
        .find(|r| r.requirement_type_id == req_type.id)
        .expect("seeded row");

    let upload_dir = std::env::temp_dir().join("dtr-integration-test-uploads");
    let _ = std::fs::create_dir_all(&upload_dir);
    submit_requirement(
        &pool,
        &upload_dir,
        5 * 1024 * 1024,
        employee.id,
        row.id,
        Some("Submitted at HR"),
        None,
    )
    .await
    .expect("submit");

    let updated = list_for_employee(&pool, employee.id)
        .await
        .expect("list again");
    let submitted = updated.iter().find(|r| r.id == row.id).expect("row");
    assert_eq!(submitted.status, RequirementStatus::Submitted);

    cleanup_employee(&pool, &code).await;
    let _ = sqlx::query("DELETE FROM requirement_types WHERE id = $1")
        .bind(req_type.id)
        .execute(&pool)
        .await;
}

#[tokio::test]
async fn eod_unlock_allows_editing_again() {
    let Some(pool) = try_pool().await else {
        eprintln!("skipping integration test: DATABASE_URL not available");
        return;
    };

    let code = unique_code("UNL");
    let employee = create_employee(
        &pool,
        &code,
        "Unlock Test",
        "482915",
        UserRole::Employee,
        None,
    )
    .await
    .expect("create employee");

    clock_in(&pool, employee.id).await.expect("clock in");
    let settings = get_settings(&pool).await.expect("settings");
    let today = company_date_now(&settings).expect("today");
    let report = save_report(
        &pool,
        employee.id,
        today,
        "Done for today",
        true,
        &[EodTaskInput {
            kind: EodTaskKind::Completed,
            title: "Task".into(),
        }],
    )
    .await
    .expect("submit eod");

    assert!(!needs_eod_reminder(&pool, employee.id)
        .await
        .expect("reminder"));

    let admin_id =
        sqlx::query_scalar::<_, Uuid>("SELECT id FROM employees WHERE role = 'admin' LIMIT 1")
            .fetch_one(&pool)
            .await
            .expect("admin id");

    unlock_report(&pool, report.id, admin_id)
        .await
        .expect("unlock");

    assert!(needs_eod_reminder(&pool, employee.id)
        .await
        .expect("reminder on"));

    save_report(
        &pool,
        employee.id,
        today,
        "Updated after unlock",
        true,
        &[EodTaskInput {
            kind: EodTaskKind::Completed,
            title: "Revised task".into(),
        }],
    )
    .await
    .expect("resubmit after unlock");

    let history = list_employee_eod_history(&pool, employee.id, 10)
        .await
        .expect("history");
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].summary, "Updated after unlock");

    cleanup_employee(&pool, &code).await;
}

#[tokio::test]
async fn requirement_expiry_allows_resubmit() {
    let Some(pool) = try_pool().await else {
        eprintln!("skipping integration test: DATABASE_URL not available");
        return;
    };

    let code = unique_code("EXP");
    let employee = create_employee(
        &pool,
        &code,
        "Expiry Test",
        "482915",
        UserRole::Employee,
        None,
    )
    .await
    .expect("create employee");

    let req_type = create_type(
        &pool,
        "Medical Certificate",
        "Annual physical",
        true,
        false,
        1,
        Some(365),
    )
    .await
    .expect("create type");

    let reqs = list_for_employee(&pool, employee.id).await.expect("list");
    let row = reqs
        .iter()
        .find(|r| r.requirement_type_id == req_type.id)
        .expect("seeded row");

    let upload_dir = std::env::temp_dir().join("dtr-integration-test-uploads");
    let _ = std::fs::create_dir_all(&upload_dir);
    submit_requirement(
        &pool,
        &upload_dir,
        5 * 1024 * 1024,
        employee.id,
        row.id,
        Some("Submitted docs"),
        None,
    )
    .await
    .expect("submit");

    let admin_id =
        sqlx::query_scalar::<_, Uuid>("SELECT id FROM employees WHERE role = 'admin' LIMIT 1")
            .fetch_one(&pool)
            .await
            .expect("admin id");

    review_requirement(
        &pool,
        employee.id,
        row.id,
        admin_id,
        true,
        Some("Looks good"),
    )
    .await
    .expect("approve");

    let approved = list_for_employee(&pool, employee.id)
        .await
        .expect("approved list");
    let approved_row = approved.iter().find(|r| r.id == row.id).expect("row");
    assert_eq!(approved_row.status, RequirementStatus::Approved);
    assert!(approved_row.expires_at.is_some());
    assert!(!can_submit_requirement(approved_row));

    sqlx::query(
        "UPDATE employee_requirements SET expires_at = now() - interval '1 day' WHERE id = $1",
    )
    .bind(row.id)
    .execute(&pool)
    .await
    .expect("expire");

    let expired = list_for_employee(&pool, employee.id)
        .await
        .expect("expired list");
    let expired_row = expired.iter().find(|r| r.id == row.id).expect("row");
    assert!(is_requirement_expired(expired_row.expires_at));
    assert!(can_submit_requirement(expired_row));

    submit_requirement(
        &pool,
        &upload_dir,
        5 * 1024 * 1024,
        employee.id,
        row.id,
        Some("Renewed docs"),
        None,
    )
    .await
    .expect("resubmit");

    let resubmitted = list_for_employee(&pool, employee.id)
        .await
        .expect("resubmitted");
    let resubmitted_row = resubmitted.iter().find(|r| r.id == row.id).expect("row");
    assert_eq!(resubmitted_row.status, RequirementStatus::Submitted);
    assert!(resubmitted_row.expires_at.is_none());

    cleanup_employee(&pool, &code).await;
    let _ = sqlx::query("DELETE FROM requirement_types WHERE id = $1")
        .bind(req_type.id)
        .execute(&pool)
        .await;
}

#[tokio::test]
async fn bulk_department_assign_updates_profiles() {
    let Some(pool) = try_pool().await else {
        eprintln!("skipping integration test: DATABASE_URL not available");
        return;
    };

    let code_a = unique_code("DEPT");
    let code_b = unique_code("DEPT");
    let a = create_employee(&pool, &code_a, "Dept A", "482915", UserRole::Employee, None)
        .await
        .expect("create a");
    let b = create_employee(&pool, &code_b, "Dept B", "482915", UserRole::Employee, None)
        .await
        .expect("create b");

    let admin_id =
        sqlx::query_scalar::<_, Uuid>("SELECT id FROM employees WHERE role = 'admin' LIMIT 1")
            .fetch_one(&pool)
            .await
            .expect("admin id");

    let count = bulk_assign_department(&pool, &[a.id, b.id], "Operations", admin_id)
        .await
        .expect("bulk assign");
    assert_eq!(count, 2);

    let rows = list_admin_employee_rows(&pool, &AdminEmployeeQuery::default())
        .await
        .expect("list");
    let row_a = rows.iter().find(|r| r.id == a.id).expect("row a");
    assert_eq!(row_a.department.as_deref(), Some("Operations"));
    assert!(profile_completeness_pct(row_a) > 0);

    cleanup_employee(&pool, &code_a).await;
    cleanup_employee(&pool, &code_b).await;
}

#[tokio::test]
async fn holiday_skips_eod_reminder() {
    let Some(pool) = try_pool().await else {
        eprintln!("skipping integration test: DATABASE_URL not available");
        return;
    };

    let code = unique_code("HOL");
    let employee = create_employee(
        &pool,
        &code,
        "Holiday Test",
        "482915",
        UserRole::Employee,
        None,
    )
    .await
    .expect("create employee");

    let settings = get_settings(&pool).await.expect("settings");
    let today = company_date_now(&settings).expect("today");
    add_holiday(&pool, today, "Test Holiday")
        .await
        .expect("add holiday");
    assert!(is_holiday(&pool, today).await.expect("is holiday"));

    clock_in(&pool, employee.id).await.expect("clock in");
    assert!(!needs_eod_reminder(&pool, employee.id)
        .await
        .expect("no eod on holiday"));

    let _ = sqlx::query("DELETE FROM company_holidays WHERE holiday_date = $1")
        .bind(today)
        .execute(&pool)
        .await;
    cleanup_employee(&pool, &code).await;
}

#[tokio::test]
async fn leave_types_appear_in_payroll_summary() {
    let Some(pool) = try_pool().await else {
        eprintln!("skipping integration test: DATABASE_URL not available");
        return;
    };

    let code = unique_code("LV");
    let employee = create_employee(
        &pool,
        &code,
        "Leave Test",
        "482915",
        UserRole::Employee,
        None,
    )
    .await
    .expect("create employee");

    let admin_id =
        sqlx::query_scalar::<_, Uuid>("SELECT id FROM employees WHERE role = 'admin' LIMIT 1")
            .fetch_one(&pool)
            .await
            .expect("admin id");

    let settings = get_settings(&pool).await.expect("settings");
    let today = company_date_now(&settings).expect("today");
    mark_absence_for_employee(
        &pool,
        employee.id,
        today,
        AttendanceStatus::SickLeave,
        admin_id,
        true,
        admin_id,
    )
    .await
    .expect("mark sick leave");

    let rows = payroll_summary(&pool, today, today, &PayrollFilters::default())
        .await
        .expect("payroll");
    let row = rows.iter().find(|r| r.employee_code == code).expect("row");
    assert_eq!(row.sick_leave_days, 1);

    mark_absence_for_employee(
        &pool,
        employee.id,
        today - time::Duration::days(1),
        AttendanceStatus::Offset,
        admin_id,
        true,
        admin_id,
    )
    .await
    .expect("mark offset");

    let yesterday = today - time::Duration::days(1);
    let rows = payroll_summary(&pool, yesterday, today, &PayrollFilters::default())
        .await
        .expect("payroll range");
    let row = rows.iter().find(|r| r.employee_code == code).expect("row");
    assert_eq!(row.offset_days, 1);

    cleanup_employee(&pool, &code).await;
}

#[tokio::test]
async fn in_app_notifications_include_missing_eod() {
    let Some(pool) = try_pool().await else {
        eprintln!("skipping integration test: DATABASE_URL not available");
        return;
    };

    let code = unique_code("NTF");
    let employee = create_employee(
        &pool,
        &code,
        "Notify Test",
        "482915",
        UserRole::Employee,
        None,
    )
    .await
    .expect("create employee");

    clock_in(&pool, employee.id).await.expect("clock in");

    let user = UserSession {
        employee_id: employee.id,
        employee_code: code.clone(),
        full_name: employee.full_name.clone(),
        role: UserRole::Employee,
        must_change_pin: false,
    };

    let notes = list_for_user(&pool, &user).await.expect("notifications");
    assert!(notes.iter().any(|n| n.kind == "missing_eod"));

    cleanup_employee(&pool, &code).await;
}

#[test]
fn ot_status_auto_approves_when_approval_disabled() {
    let settings = dtr::models::CompanySettings {
        company_name: "Test".into(),
        break_minutes: 60,
        ot_threshold_minutes: 480,
        grace_minutes: 5,
        pay_period: dtr::models::PayPeriodType::Semimonthly,
        pay_period_anchor: Date::from_calendar_date(2024, Month::January, 1).unwrap(),
        timezone: "Asia/Manila".into(),
        ot_requires_approval: false,
    };

    assert_eq!(ot_status_for_minutes(90, &settings), OtStatus::Approved);
    assert_eq!(ot_status_for_minutes(0, &settings), OtStatus::None);
}

#[tokio::test]
async fn clock_out_requires_ot_reason_when_pending() {
    let Some(pool) = try_pool().await else {
        eprintln!("skipping integration test: DATABASE_URL not available");
        return;
    };

    let code = unique_code("OTR");
    let employee = create_employee(
        &pool,
        &code,
        "OT Reason Test",
        "482915",
        UserRole::Employee,
        None,
    )
    .await
    .expect("create employee");

    let settings = get_settings(&pool).await.expect("settings");
    let today = company_date_now(&settings).expect("today");
    let clock_in = now_company(&settings).expect("now") - time::Duration::hours(10);

    sqlx::query(
        "INSERT INTO time_entries (employee_id, work_date, clock_in, attendance)
         VALUES ($1, $2, $3, 'on_time')",
    )
    .bind(employee.id)
    .bind(today)
    .bind(clock_in)
    .execute(&pool)
    .await
    .expect("insert entry");

    let without_reason = clock_out(&pool, employee.id, None).await;
    assert!(matches!(
        without_reason,
        Err(AppError::BadRequest(msg)) if msg.contains("reason for overtime")
    ));

    let with_reason = clock_out(&pool, employee.id, Some("Client deadline"))
        .await
        .expect("clock out with reason");
    assert_eq!(
        with_reason.ot_request_reason.as_deref(),
        Some("Client deadline")
    );
    assert_eq!(with_reason.ot_status, OtStatus::Pending);

    cleanup_employee(&pool, &code).await;
}

#[tokio::test]
async fn closed_pay_period_blocks_clock_in_and_correction() {
    let Some(pool) = try_pool().await else {
        eprintln!("skipping integration test: DATABASE_URL not available");
        return;
    };

    let code = unique_code("CLS");
    let employee = create_employee(
        &pool,
        &code,
        "Closed Period",
        "482915",
        UserRole::Employee,
        None,
    )
    .await
    .expect("create employee");

    let settings = get_settings(&pool).await.expect("settings");
    let today = company_date_now(&settings).expect("today");
    let admin_id = Uuid::new_v4();
    close_pay_period(&pool, today, today, admin_id, Some("integration test"))
        .await
        .expect("close period");

    let clock_result = clock_in(&pool, employee.id).await;
    assert!(matches!(
        clock_result,
        Err(AppError::BadRequest(msg)) if msg.contains("closed pay period")
    ));

    let tz = settings.timezone.as_str();
    let clock_in_time =
        combine_date_time(today, Time::from_hms(8, 0, 0).unwrap(), tz).expect("clock in time");
    let clock_out_time =
        combine_date_time(today, Time::from_hms(17, 0, 0).unwrap(), tz).expect("clock out time");

    let correction_result = create_corrected_entry(
        &pool,
        employee.id,
        today,
        &CorrectionSubmission {
            editor_id: admin_id,
            manager_id: admin_id,
            is_admin: true,
            new_clock_in: clock_in_time,
            new_clock_out: clock_out_time,
            reason: "test",
        },
    )
    .await;
    assert!(matches!(
        correction_result,
        Err(AppError::BadRequest(msg)) if msg.contains("closed pay period")
    ));

    let _ = sqlx::query("DELETE FROM closed_pay_periods WHERE period_start = $1")
        .bind(today)
        .execute(&pool)
        .await;
    cleanup_employee(&pool, &code).await;
}

#[tokio::test]
async fn closed_pay_period_blocks_leave_create_and_approval() {
    let Some(pool) = try_pool().await else {
        eprintln!("skipping integration test: DATABASE_URL not available");
        return;
    };

    let code = unique_code("LVCL");
    let employee = create_employee(
        &pool,
        &code,
        "Leave Closed Test",
        "482915",
        UserRole::Employee,
        None,
    )
    .await
    .expect("create employee");

    let settings = get_settings(&pool).await.expect("settings");
    let today = company_date_now(&settings).expect("today");
    let admin_id =
        sqlx::query_scalar::<_, Uuid>("SELECT id FROM employees WHERE role = 'admin' LIMIT 1")
            .fetch_one(&pool)
            .await
            .expect("admin id");

    let pending = create_request(
        &pool,
        employee.id,
        today,
        today,
        LeaveRequestType::Vacation,
        Some("Planned day off"),
    )
    .await
    .expect("create leave before close");

    close_pay_period(&pool, today, today, admin_id, Some("leave test"))
        .await
        .expect("close period");

    let create_result = create_request(
        &pool,
        employee.id,
        today,
        today,
        LeaveRequestType::SickLeave,
        Some("Should fail"),
    )
    .await;
    assert!(matches!(
        create_result,
        Err(AppError::BadRequest(msg)) if msg.contains("closed pay period")
    ));

    let approve_result = review_request(&pool, pending.id, admin_id, true, true, None).await;
    assert!(matches!(
        approve_result,
        Err(AppError::BadRequest(msg)) if msg.contains("closed pay period")
    ));

    let _ = sqlx::query("DELETE FROM closed_pay_periods WHERE period_start = $1")
        .bind(today)
        .execute(&pool)
        .await;
    let _ = sqlx::query("DELETE FROM leave_requests WHERE employee_id = $1")
        .bind(employee.id)
        .execute(&pool)
        .await;
    cleanup_employee(&pool, &code).await;
}

#[tokio::test]
async fn closed_pay_period_blocks_ot_eod_and_absence() {
    let Some(pool) = try_pool().await else {
        eprintln!("skipping integration test: DATABASE_URL not available");
        return;
    };

    let mgr_code = unique_code("MGCL");
    let emp_code = unique_code("EMCL");
    let manager = create_employee(
        &pool,
        &mgr_code,
        "Closed OT Manager",
        "482915",
        UserRole::Manager,
        None,
    )
    .await
    .expect("manager");
    let employee = create_employee(
        &pool,
        &emp_code,
        "Closed OT Employee",
        "482915",
        UserRole::Employee,
        Some(manager.id),
    )
    .await
    .expect("employee");

    let settings = get_settings(&pool).await.expect("settings");
    let today = company_date_now(&settings).expect("today");
    let admin_id =
        sqlx::query_scalar::<_, Uuid>("SELECT id FROM employees WHERE role = 'admin' LIMIT 1")
            .fetch_one(&pool)
            .await
            .expect("admin id");

    let entry_id: Uuid = sqlx::query_scalar(
        "INSERT INTO time_entries
            (employee_id, work_date, regular_minutes, ot_minutes, ot_status, attendance)
         VALUES ($1, $2, 480, 60, 'pending', 'on_time')
         RETURNING id",
    )
    .bind(employee.id)
    .bind(today)
    .fetch_one(&pool)
    .await
    .expect("insert pending ot");

    clock_in(&pool, employee.id)
        .await
        .expect("clock in before close");

    close_pay_period(&pool, today, today, admin_id, Some("ot eod absence test"))
        .await
        .expect("close period");

    let ot_result = review_overtime(&pool, entry_id, manager.id, true, None, false).await;
    assert!(matches!(
        ot_result,
        Err(AppError::BadRequest(msg)) if msg.contains("closed pay period")
    ));

    let eod_result = save_report(
        &pool,
        employee.id,
        today,
        "Should fail",
        true,
        &[EodTaskInput {
            kind: EodTaskKind::Completed,
            title: "Blocked task".into(),
        }],
    )
    .await;
    assert!(matches!(
        eod_result,
        Err(AppError::BadRequest(msg)) if msg.contains("closed pay period")
    ));

    let absence_result = mark_absence_for_employee(
        &pool,
        employee.id,
        today,
        AttendanceStatus::SickLeave,
        admin_id,
        true,
        admin_id,
    )
    .await;
    assert!(matches!(
        absence_result,
        Err(AppError::BadRequest(msg)) if msg.contains("closed pay period")
    ));

    let _ = sqlx::query("DELETE FROM closed_pay_periods WHERE period_start = $1")
        .bind(today)
        .execute(&pool)
        .await;
    cleanup_employee(&pool, &emp_code).await;
    cleanup_employee(&pool, &mgr_code).await;
}

#[tokio::test]
async fn reopen_pay_period_allows_clock_in_again() {
    let Some(pool) = try_pool().await else {
        eprintln!("skipping integration test: DATABASE_URL not available");
        return;
    };

    let code = unique_code("REOP");
    let employee = create_employee(
        &pool,
        &code,
        "Reopen Test",
        "482915",
        UserRole::Employee,
        None,
    )
    .await
    .expect("create employee");

    let settings = get_settings(&pool).await.expect("settings");
    let today = company_date_now(&settings).expect("today");
    let admin_id =
        sqlx::query_scalar::<_, Uuid>("SELECT id FROM employees WHERE role = 'admin' LIMIT 1")
            .fetch_one(&pool)
            .await
            .expect("admin id");

    close_pay_period(&pool, today, today, admin_id, Some("reopen test"))
        .await
        .expect("close period");

    let blocked = clock_in(&pool, employee.id).await;
    assert!(matches!(
        blocked,
        Err(AppError::BadRequest(msg)) if msg.contains("closed pay period")
    ));

    reopen_pay_period(&pool, today, today)
        .await
        .expect("reopen period");

    clock_in(&pool, employee.id)
        .await
        .expect("clock in after reopen");

    let _ = sqlx::query("DELETE FROM time_entries WHERE employee_id = $1")
        .bind(employee.id)
        .execute(&pool)
        .await;
    cleanup_employee(&pool, &code).await;
}

#[tokio::test]
async fn company_timezone_drives_clock_in_work_date() {
    let Some(pool) = try_pool().await else {
        eprintln!("skipping integration test: DATABASE_URL not available");
        return;
    };

    let original = get_settings(&pool).await.expect("settings");
    let new_timezone = if original.timezone == "America/New_York" {
        "Asia/Tokyo"
    } else {
        "America/New_York"
    };

    let update = SettingsUpdate {
        company_name: &original.company_name,
        timezone: new_timezone,
        break_minutes: original.break_minutes,
        ot_threshold_minutes: original.ot_threshold_minutes,
        grace_minutes: original.grace_minutes,
        pay_period: original.pay_period,
        pay_period_anchor: original.pay_period_anchor,
        ot_requires_approval: original.ot_requires_approval,
    };
    update_settings(&pool, &update)
        .await
        .expect("update timezone");

    let code = unique_code("TZ");
    let employee = create_employee(
        &pool,
        &code,
        "Timezone Test",
        "482915",
        UserRole::Employee,
        None,
    )
    .await
    .expect("create employee");

    let entry = clock_in(&pool, employee.id)
        .await
        .expect("clock in with updated timezone");
    let settings = get_settings(&pool).await.expect("settings");
    let expected_date = company_date_now(&settings).expect("company today");

    assert_eq!(settings.timezone, new_timezone);
    assert_eq!(entry.work_date, expected_date);

    let _ = sqlx::query("DELETE FROM time_entries WHERE employee_id = $1")
        .bind(employee.id)
        .execute(&pool)
        .await;
    cleanup_employee(&pool, &code).await;

    let restore = SettingsUpdate {
        company_name: &original.company_name,
        timezone: &original.timezone,
        break_minutes: original.break_minutes,
        ot_threshold_minutes: original.ot_threshold_minutes,
        grace_minutes: original.grace_minutes,
        pay_period: original.pay_period,
        pay_period_anchor: original.pay_period_anchor,
        ot_requires_approval: original.ot_requires_approval,
    };
    update_settings(&pool, &restore)
        .await
        .expect("restore timezone");
}

#[tokio::test]
async fn duplicate_pay_period_close_is_idempotent() {
    let Some(pool) = try_pool().await else {
        eprintln!("skipping integration test: DATABASE_URL not available");
        return;
    };

    let start = Date::from_calendar_date(2099, Month::February, 1).unwrap();
    let end = Date::from_calendar_date(2099, Month::February, 7).unwrap();
    let admin_id =
        sqlx::query_scalar::<_, Uuid>("SELECT id FROM employees WHERE role = 'admin' LIMIT 1")
            .fetch_one(&pool)
            .await
            .expect("admin id");

    let first = close_pay_period(&pool, start, end, admin_id, Some("first close"))
        .await
        .expect("first close");
    assert_eq!(first, ClosePayPeriodResult::Closed);

    let second = close_pay_period(&pool, start, end, admin_id, Some("second close"))
        .await
        .expect("second close");
    assert_eq!(second, ClosePayPeriodResult::AlreadyClosed);

    reopen_pay_period(&pool, start, end)
        .await
        .expect("cleanup reopen");
}

#[tokio::test]
async fn overlapping_pay_period_close_is_rejected() {
    let Some(pool) = try_pool().await else {
        eprintln!("skipping integration test: DATABASE_URL not available");
        return;
    };

    let admin_id =
        sqlx::query_scalar::<_, Uuid>("SELECT id FROM employees WHERE role = 'admin' LIMIT 1")
            .fetch_one(&pool)
            .await
            .expect("admin id");

    let first_start = Date::from_calendar_date(2099, Month::April, 1).unwrap();
    let first_end = Date::from_calendar_date(2099, Month::April, 7).unwrap();
    let overlap_start = Date::from_calendar_date(2099, Month::April, 5).unwrap();
    let overlap_end = Date::from_calendar_date(2099, Month::April, 12).unwrap();

    close_pay_period(
        &pool,
        first_start,
        first_end,
        admin_id,
        Some("overlap test"),
    )
    .await
    .expect("first close");

    let result = close_pay_period(
        &pool,
        overlap_start,
        overlap_end,
        admin_id,
        Some("should fail"),
    )
    .await;
    assert!(matches!(
        result,
        Err(AppError::BadRequest(msg)) if msg.contains("overlaps existing closed period")
    ));

    reopen_pay_period(&pool, first_start, first_end)
        .await
        .expect("cleanup reopen");
}

#[tokio::test]
async fn compensation_profile_persists_and_gross_pay_follows_policy() {
    let Some(pool) = try_pool().await else {
        eprintln!("skipping integration test: DATABASE_URL not available");
        return;
    };

    let admin_code = unique_code("CMPA");
    let emp_code = unique_code("CMPE");
    let admin = create_employee(
        &pool,
        &admin_code,
        "Comp Admin",
        "482915",
        UserRole::Admin,
        None,
    )
    .await
    .expect("admin");
    let employee = create_employee(
        &pool,
        &emp_code,
        "Comp Employee",
        "482915",
        UserRole::Employee,
        None,
    )
    .await
    .expect("employee");

    let effective = time::Date::from_calendar_date(2026, time::Month::January, 1).unwrap();
    upsert_profile(&pool, employee.id, 2_600_000, 132, effective, admin.id)
        .await
        .expect("upsert compensation");

    let profile = get_compensation(&pool, employee.id)
        .await
        .expect("get compensation")
        .expect("profile exists");
    assert_eq!(profile.monthly_salary_cents, 2_600_000);
    assert_eq!(profile.ot_rate_percent, 132);

    let gross = gross_pay_cents(&GrossPayInput {
        monthly_salary_cents: profile.monthly_salary_cents,
        ot_rate_percent: profile.ot_rate_percent,
        pay_period: PayPeriodType::Semimonthly,
        approved_ot_minutes: 60,
        no_show_days: 1,
    });
    assert_eq!(gross, 1_216_500);

    let _ = sqlx::query("DELETE FROM compensation_profiles WHERE employee_id = $1")
        .bind(employee.id)
        .execute(&pool)
        .await;
    cleanup_employee(&pool, &emp_code).await;
    cleanup_employee(&pool, &admin_code).await;
}

async fn ensure_all_active_have_compensation(pool: &PgPool, admin_id: Uuid, effective: Date) {
    let ids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT e.id FROM employees e
         LEFT JOIN compensation_profiles c ON c.employee_id = e.id
         WHERE e.is_active = TRUE AND c.employee_id IS NULL",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();
    for id in ids {
        let _ = upsert_profile(pool, id, 1_000_000, 132, effective, admin_id).await;
    }
}

#[tokio::test]
async fn admin_can_create_and_finalize_payroll_run() {
    let Some(pool) = try_pool().await else {
        eprintln!("skipping integration test: DATABASE_URL not available");
        return;
    };

    let admin_code = unique_code("PYAD");
    let emp_code = unique_code("PYEM");
    let admin = create_employee(
        &pool,
        &admin_code,
        "Payroll Run Admin",
        "482915",
        UserRole::Admin,
        None,
    )
    .await
    .expect("admin");
    let _employee = create_employee(
        &pool,
        &emp_code,
        "Payroll Run Employee",
        "482915",
        UserRole::Employee,
        None,
    )
    .await
    .expect("employee");

    let settings = get_settings(&pool).await.expect("settings");
    let today = company_date_now(&settings).expect("today");
    let effective = Date::from_calendar_date(2026, Month::January, 1).unwrap();

    ensure_all_active_have_compensation(&pool, admin.id, effective).await;

    close_pay_period(&pool, today, today, admin.id, Some("payroll run test"))
        .await
        .expect("close period");

    let run_id = create_draft_run(&pool, today, today, admin.id, &settings, None)
        .await
        .expect("create draft");
    let run = get_run(&pool, run_id).await.expect("get run");
    assert_eq!(run.status, PayrollRunStatus::Draft);

    let lines = list_lines_for_run(&pool, run_id).await.expect("lines");
    assert!(!lines.is_empty());
    assert!(lines
        .iter()
        .any(|l| l.employee_code == emp_code.to_uppercase()));

    finalize_run(&pool, run_id, admin.id)
        .await
        .expect("finalize");
    let run = get_run(&pool, run_id).await.expect("get run");
    assert_eq!(run.status, PayrollRunStatus::Finalized);

    let _ = sqlx::query("DELETE FROM payroll_lines WHERE run_id = $1")
        .bind(run_id)
        .execute(&pool)
        .await;
    let _ = sqlx::query("DELETE FROM payroll_runs WHERE id = $1")
        .bind(run_id)
        .execute(&pool)
        .await;
    reopen_pay_period(&pool, today, today)
        .await
        .expect("reopen");
    cleanup_employee(&pool, &emp_code).await;
    cleanup_employee(&pool, &admin_code).await;
}
