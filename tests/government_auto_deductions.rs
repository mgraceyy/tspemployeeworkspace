//! Integration test for government auto-deductions on payroll runs.
//! Requires wiring from patches/government_auto_deductions_wiring.md.

mod common;

use dtr::models::UserRole;
use dtr::services::compensation::{upsert_profile, UpsertProfileInput};
use dtr::services::employees::create_employee;
use dtr::services::payroll::{
    create_draft_run, list_deductions_for_line, list_lines_for_run,
    period_government_deductions_cents, recalculate_government_deductions_for_run,
};
use dtr::services::payroll_controls::close_pay_period;
use dtr::services::reports::current_pay_period;
use dtr::services::settings::{
    get_settings, settings_update_from, update_settings, SettingsUpdate,
};
use sqlx::PgPool;
use time::{Date, Month};
use uuid::Uuid;

fn unique_code(prefix: &str) -> String {
    format!("{prefix}{}", &Uuid::new_v4().simple().to_string()[..8]).to_uppercase()
}

fn isolated_payroll_period(settings: &dtr::models::CompanySettings) -> (Date, Date) {
    let anchor = Date::from_calendar_date(2099, Month::June, 10).unwrap();
    let (start, end, _) =
        current_pay_period(anchor, settings.pay_period, settings.pay_period_anchor);
    (start, end)
}

async fn enable_all_gov_toggles(pool: &PgPool) {
    let settings = get_settings(pool).await.expect("settings");
    update_settings(
        pool,
        &SettingsUpdate {
            auto_deduct_sss: true,
            auto_deduct_phic: true,
            auto_deduct_hdmf: true,
            auto_deduct_wht: true,
            ..settings_update_from(&settings)
        },
    )
    .await
    .expect("enable gov toggles");
}

async fn cleanup_payroll_test_period(
    pool: &PgPool,
    _run_id: Option<Uuid>,
    period_start: Date,
    period_end: Date,
) {
    let _ = sqlx::query(
        "DELETE FROM payroll_deductions WHERE line_id IN (
            SELECT pl.id FROM payroll_lines pl
            JOIN payroll_runs pr ON pr.id = pl.run_id
            WHERE pr.period_start = $1 AND pr.period_end = $2)",
    )
    .bind(period_start)
    .bind(period_end)
    .execute(pool)
    .await;
    let _ = sqlx::query(
        "DELETE FROM payroll_lines WHERE run_id IN (
            SELECT id FROM payroll_runs WHERE period_start = $1 AND period_end = $2)",
    )
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
        dtr::services::payroll_controls::reopen_pay_period(pool, period_start, period_end).await;
    let _ = sqlx::query(
        "DELETE FROM closed_pay_periods
         WHERE period_start <= $2 AND period_end >= $1",
    )
    .bind(period_start)
    .bind(period_end)
    .execute(pool)
    .await;
}

async fn cleanup_employee(pool: &PgPool, code: &str) {
    let _ = sqlx::query("DELETE FROM employees WHERE employee_code = $1")
        .bind(code)
        .execute(pool)
        .await;
}

#[tokio::test]
async fn draft_run_applies_government_auto_deductions_when_enabled() {
    let Some(pool) = common::test_pool().await else {
        eprintln!("skipping: DATABASE_URL not available");
        return;
    };

    enable_all_gov_toggles(&pool).await;

    let admin_code = unique_code("GVAD");
    let emp_code = unique_code("GVEM");
    let admin = create_employee(
        &pool,
        &admin_code,
        "Gov Deduction Admin",
        "482915",
        UserRole::Admin,
        None,
    )
    .await
    .expect("admin");
    let _employee = create_employee(
        &pool,
        &emp_code,
        "Gov Deduction Employee",
        "482915",
        UserRole::Employee,
        None,
    )
    .await
    .expect("employee");

    let monthly_salary_cents = 2_600_000_i64;
    let effective = Date::from_calendar_date(2026, Month::January, 1).unwrap();
    upsert_profile(
        &pool,
        &UpsertProfileInput::new(_employee.id, monthly_salary_cents, effective, admin.id),
    )
    .await
    .expect("compensation");

    let settings = get_settings(&pool).await.expect("settings");
    let (period_start, period_end) = isolated_payroll_period(&settings);
    cleanup_payroll_test_period(&pool, None, period_start, period_end).await;

    close_pay_period(
        &pool,
        period_start,
        period_end,
        admin.id,
        Some("gov auto test"),
    )
    .await
    .expect("close");

    let run_id = create_draft_run(&pool, period_start, period_end, admin.id, &settings, None)
        .await
        .expect("create draft");

    let lines = list_lines_for_run(&pool, run_id).await.expect("lines");
    let line = lines
        .iter()
        .find(|l| l.employee_code == emp_code.to_uppercase())
        .expect("employee line");

    let expected = period_government_deductions_cents(monthly_salary_cents, settings.pay_period);
    let deductions = list_deductions_for_line(&pool, line.id)
        .await
        .expect("deductions");

    let sss = deductions
        .iter()
        .find(|d| d.code == "SSS")
        .expect("sss row");
    assert_eq!(sss.amount_cents, expected.sss_cents);
    assert_eq!(sss.note.as_deref(), Some("Auto-calculated"));
    assert!(line.total_deduction_cents > 0);
    assert_eq!(
        line.net_pay_cents,
        line.gross_pay_cents - line.total_deduction_cents
    );

    cleanup_payroll_test_period(&pool, Some(run_id), period_start, period_end).await;
    cleanup_employee(&pool, &emp_code).await;
    cleanup_employee(&pool, &admin_code).await;
}

#[tokio::test]
async fn recalculate_refreshes_stale_government_deductions() {
    let Some(pool) = common::test_pool().await else {
        eprintln!("skipping: DATABASE_URL not available");
        return;
    };

    enable_all_gov_toggles(&pool).await;

    let admin_code = unique_code("GRAD");
    let emp_code = unique_code("GREM");
    let admin = create_employee(
        &pool,
        &admin_code,
        "Gov Recalc Admin",
        "482915",
        UserRole::Admin,
        None,
    )
    .await
    .expect("admin");
    let employee = create_employee(
        &pool,
        &emp_code,
        "Gov Recalc Employee",
        "482915",
        UserRole::Employee,
        None,
    )
    .await
    .expect("employee");

    let monthly_salary_cents = 2_600_000_i64;
    let effective = Date::from_calendar_date(2026, Month::January, 1).unwrap();
    upsert_profile(
        &pool,
        &UpsertProfileInput::new(employee.id, monthly_salary_cents, effective, admin.id),
    )
    .await
    .expect("compensation");

    let settings = get_settings(&pool).await.expect("settings");
    let (period_start, period_end) = isolated_payroll_period(&settings);
    cleanup_payroll_test_period(&pool, None, period_start, period_end).await;

    close_pay_period(
        &pool,
        period_start,
        period_end,
        admin.id,
        Some("gov recalc"),
    )
    .await
    .expect("close");

    let run_id = create_draft_run(&pool, period_start, period_end, admin.id, &settings, None)
        .await
        .expect("create");

    let lines = list_lines_for_run(&pool, run_id).await.expect("lines");
    let line = lines
        .iter()
        .find(|l| l.employee_code == emp_code.to_uppercase())
        .expect("line");

    sqlx::query(
        "UPDATE payroll_deductions SET amount_cents = 1
         WHERE line_id = $1 AND note = 'Auto-calculated'",
    )
    .bind(line.id)
    .execute(&pool)
    .await
    .expect("stale auto row");

    recalculate_government_deductions_for_run(&pool, run_id, &settings)
        .await
        .expect("recalculate");

    let expected = period_government_deductions_cents(monthly_salary_cents, settings.pay_period);
    let deductions = list_deductions_for_line(&pool, line.id)
        .await
        .expect("deductions");
    let sss = deductions.iter().find(|d| d.code == "SSS").expect("sss");
    assert_eq!(sss.amount_cents, expected.sss_cents);

    cleanup_payroll_test_period(&pool, Some(run_id), period_start, period_end).await;
    cleanup_employee(&pool, &emp_code).await;
    cleanup_employee(&pool, &admin_code).await;
}

#[tokio::test]
async fn disabled_toggles_skip_government_auto_deductions() {
    let Some(pool) = common::test_pool().await else {
        eprintln!("skipping: DATABASE_URL not available");
        return;
    };

    let settings = get_settings(&pool).await.expect("settings");
    update_settings(
        &pool,
        &SettingsUpdate {
            auto_deduct_sss: false,
            auto_deduct_phic: false,
            auto_deduct_hdmf: false,
            auto_deduct_wht: false,
            ..settings_update_from(&settings)
        },
    )
    .await
    .expect("disable toggles");

    let admin_code = unique_code("GDAD");
    let emp_code = unique_code("GDEM");
    let admin = create_employee(
        &pool,
        &admin_code,
        "Gov Disabled Admin",
        "482915",
        UserRole::Admin,
        None,
    )
    .await
    .expect("admin");
    let employee = create_employee(
        &pool,
        &emp_code,
        "Gov Disabled Employee",
        "482915",
        UserRole::Employee,
        None,
    )
    .await
    .expect("employee");

    let effective = Date::from_calendar_date(2026, Month::January, 1).unwrap();
    upsert_profile(
        &pool,
        &UpsertProfileInput::new(employee.id, 2_600_000, effective, admin.id),
    )
    .await
    .expect("compensation");

    let settings = get_settings(&pool).await.expect("settings");
    let (period_start, period_end) = isolated_payroll_period(&settings);
    cleanup_payroll_test_period(&pool, None, period_start, period_end).await;

    close_pay_period(
        &pool,
        period_start,
        period_end,
        admin.id,
        Some("gov disabled"),
    )
    .await
    .expect("close");

    let run_id = create_draft_run(&pool, period_start, period_end, admin.id, &settings, None)
        .await
        .expect("create");

    let lines = list_lines_for_run(&pool, run_id).await.expect("lines");
    let line = lines
        .iter()
        .find(|l| l.employee_code == emp_code.to_uppercase())
        .expect("line");

    let deductions = list_deductions_for_line(&pool, line.id)
        .await
        .expect("deductions");
    assert!(
        !deductions
            .iter()
            .any(|d| d.note.as_deref() == Some("Auto-calculated")),
        "no auto gov deductions when toggles off"
    );

    cleanup_payroll_test_period(&pool, Some(run_id), period_start, period_end).await;
    cleanup_employee(&pool, &emp_code).await;
    cleanup_employee(&pool, &admin_code).await;
}
