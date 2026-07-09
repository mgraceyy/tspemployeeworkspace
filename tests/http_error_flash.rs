//! Verifies POST error paths redirect with in-app flash instead of raw 400 pages.

mod common;

use axum::http::StatusCode;
use dtr::models::UserRole;
use dtr::services::settings::get_settings;
use dtr::services::timezone::{company_date_now, now_company};
use time::Duration;
use uuid::Uuid;

use common::{
    create_ready_employee, extract_csrf_token, get, has_error_flash, has_info_flash, login_as,
    post_form, test_app, test_pool,
};

const TEST_PIN: &str = "482915";

fn unique_code(prefix: &str) -> String {
    format!("{prefix}{}", &Uuid::new_v4().simple().to_string()[..8]).to_uppercase()
}

async fn cleanup_employee(pool: &sqlx::PgPool, code: &str) {
    let _ = sqlx::query("DELETE FROM time_entries WHERE employee_id IN (SELECT id FROM employees WHERE employee_code = $1)")
        .bind(code)
        .execute(pool)
        .await;
    let _ = sqlx::query("DELETE FROM employees WHERE employee_code = $1")
        .bind(code)
        .execute(pool)
        .await;
}

#[tokio::test]
async fn double_clock_in_shows_error_flash() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping: DATABASE_URL not available");
        return;
    };

    let code = unique_code("CLK2");
    let employee = create_ready_employee(
        &pool,
        &code,
        "Double Clock",
        TEST_PIN,
        UserRole::Employee,
        None,
    )
    .await
    .expect("create employee");

    let settings = get_settings(&pool).await.expect("settings");
    let today = company_date_now(&settings).expect("today");
    let now = now_company(&settings).expect("now");

    sqlx::query(
        "INSERT INTO time_entries (employee_id, work_date, clock_in, attendance)
         VALUES ($1, $2, $3, 'on_time')",
    )
    .bind(employee.id)
    .bind(today)
    .bind(now)
    .execute(&pool)
    .await
    .expect("seed entry");

    let mut app = test_app(pool.clone()).await;
    let cookies = login_as(&mut app, &code, TEST_PIN).await;
    let (_, home_html, cookies) = get(&mut app, "/", &cookies).await;
    let csrf = extract_csrf_token(&home_html).expect("csrf");
    let body = format!("csrf_token={csrf}");
    let (status, _, cookies) = post_form(&mut app, "/clock/in", &cookies, &body).await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let (_, home_after, _) = get(&mut app, "/", &cookies).await;
    assert!(
        has_error_flash(&home_after),
        "expected error flash, got: {}",
        home_after.chars().take(400).collect::<String>()
    );
    assert!(home_after.contains("Already clocked in") || home_after.contains("Already completed"));

    cleanup_employee(&pool, &code).await;
}

#[tokio::test]
async fn clock_out_ot_reason_shows_error_flash() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping: DATABASE_URL not available");
        return;
    };

    let code = unique_code("CLKO");
    let employee = create_ready_employee(
        &pool,
        &code,
        "Clock Out Flash",
        TEST_PIN,
        UserRole::Employee,
        None,
    )
    .await
    .expect("create employee");

    let settings = get_settings(&pool).await.expect("settings");
    let today = company_date_now(&settings).expect("today");
    let clock_in_time = now_company(&settings).expect("now") - Duration::hours(10);

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
    assert!(has_error_flash(&home_after));
    assert!(
        home_after.to_lowercase().contains("overtime")
            || home_after.contains("reason")
    );

    cleanup_employee(&pool, &code).await;
}

#[tokio::test]
async fn duplicate_employee_create_redirects_with_flash() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping: DATABASE_URL not available");
        return;
    };

    let admin_code = unique_code("ADUP");
    create_ready_employee(
        &pool,
        &admin_code,
        "Dup Create Admin",
        TEST_PIN,
        UserRole::Admin,
        None,
    )
    .await
    .expect("admin");

    let mut app = test_app(pool.clone()).await;
    let cookies = login_as(&mut app, &admin_code, TEST_PIN).await;
    let (_, page, cookies) = get(&mut app, "/admin/employees", &cookies).await;
    let csrf = extract_csrf_token(&page).expect("csrf");
    let body = format!(
        "employee_code={admin_code}&full_name=Duplicate&pin=1234&department=Ops&role=employee&csrf_token={csrf}"
    );
    let (status, _, cookies) = post_form(&mut app, "/admin/employees", &cookies, &body).await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let (_, after, _) = get(&mut app, "/admin/employees?add=1", &cookies).await;
    assert!(has_error_flash(&after));
    assert!(
        after.contains("already exists") || after.contains("Employee code"),
        "unexpected flash: {}",
        after.chars().take(300).collect::<String>()
    );

    cleanup_employee(&pool, &admin_code).await;
}

#[tokio::test]
async fn eod_empty_submit_shows_error_flash() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping: DATABASE_URL not available");
        return;
    };

    let code = unique_code("EODF");
    let employee = create_ready_employee(
        &pool,
        &code,
        "EOD Flash",
        TEST_PIN,
        UserRole::Employee,
        None,
    )
    .await
    .expect("create employee");

    let settings = get_settings(&pool).await.expect("settings");
    let today = company_date_now(&settings).expect("today");
    let now = now_company(&settings).expect("now");

    sqlx::query(
        "INSERT INTO time_entries (employee_id, work_date, clock_in, attendance)
         VALUES ($1, $2, $3, 'on_time')",
    )
    .bind(employee.id)
    .bind(today)
    .bind(now)
    .execute(&pool)
    .await
    .expect("clock in");

    let mut app = test_app(pool.clone()).await;
    let cookies = login_as(&mut app, &code, TEST_PIN).await;
    let (_, eod_html, cookies) = get(&mut app, "/me/eod", &cookies).await;
    let csrf = extract_csrf_token(&eod_html).expect("csrf");
    let body = format!("action=submit&summary=&csrf_token={csrf}");
    let (status, _, cookies) = post_form(&mut app, "/me/eod", &cookies, &body).await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let (_, after, _) = get(&mut app, "/me/eod", &cookies).await;
    assert!(has_error_flash(&after));

    cleanup_employee(&pool, &code).await;
}

#[tokio::test]
async fn ot_double_approve_shows_info_flash() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping: DATABASE_URL not available");
        return;
    };

    let emp_code = unique_code("OTEM");
    let mgr_code = unique_code("OTMG");
    let employee = create_ready_employee(
        &pool,
        &emp_code,
        "OT Employee",
        TEST_PIN,
        UserRole::Employee,
        None,
    )
    .await
    .expect("employee");
    let manager = create_ready_employee(
        &pool,
        &mgr_code,
        "OT Manager",
        TEST_PIN,
        UserRole::Manager,
        None,
    )
    .await
    .expect("manager");
    sqlx::query("UPDATE employees SET manager_id = $2 WHERE id = $1")
        .bind(employee.id)
        .bind(manager.id)
        .execute(&pool)
        .await
        .expect("assign manager");

    let settings = get_settings(&pool).await.expect("settings");
    let today = company_date_now(&settings).expect("today");
    let entry_id: Uuid = sqlx::query_scalar(
        "INSERT INTO time_entries
            (employee_id, work_date, clock_in, clock_out, gross_minutes, net_minutes,
             regular_minutes, ot_minutes, ot_status, attendance)
         VALUES ($1, $2, now() - interval '10 hours', now(), 600, 570, 480, 90, 'pending', 'on_time')
         RETURNING id",
    )
    .bind(employee.id)
    .bind(today)
    .fetch_one(&pool)
    .await
    .expect("entry");

    let mut app = test_app(pool.clone()).await;
    let cookies = login_as(&mut app, &mgr_code, TEST_PIN).await;
    let (_, dash, cookies) = get(&mut app, "/manager", &cookies).await;
    let csrf = extract_csrf_token(&dash).expect("csrf");
    let body = format!("action=approve&csrf_token={csrf}");
    let path = format!("/manager/ot/{entry_id}/review");
    let (status, _, cookies) = post_form(&mut app, &path, &cookies, &body).await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let (status2, _, cookies) = post_form(&mut app, &path, &cookies, &body).await;
    assert_eq!(status2, StatusCode::SEE_OTHER);
    let (_, dash_after, _) = get(&mut app, "/manager", &cookies).await;
    assert!(
        has_info_flash(&dash_after) || has_error_flash(&dash_after),
        "expected flash on double OT approve"
    );
    assert!(dash_after.contains("already reviewed") || dash_after.contains("not pending"));

    cleanup_employee(&pool, &emp_code).await;
    cleanup_employee(&pool, &mgr_code).await;
}