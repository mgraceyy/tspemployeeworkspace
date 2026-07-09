//! Integration tests for leave balance tracking.

mod common;

use dtr::models::{LeaveDayPortion, LeaveRequestType, UserRole};
use dtr::services::employees::create_employee;
use dtr::services::leave::{cancel_request, create_request, review_request};
use dtr::services::leave_balances::{get_snapshot, set_balance};
use sqlx::PgPool;
use time::{Date, Month};
use uuid::Uuid;

fn unique_code(prefix: &str) -> String {
    format!("{prefix}{}", &Uuid::new_v4().simple().to_string()[..8]).to_uppercase()
}

async fn cleanup_employee(pool: &PgPool, code: &str) {
    let _ = sqlx::query("DELETE FROM employees WHERE employee_code = $1")
        .bind(code)
        .execute(pool)
        .await;
}

#[tokio::test]
async fn leave_request_rejected_when_balance_insufficient() {
    let Some(pool) = common::test_pool().await else {
        eprintln!("skipping: DATABASE_URL not available");
        return;
    };

    let emp_code = unique_code("LVBL");
    let mgr_code = unique_code("LVBM");
    let employee = create_employee(
        &pool,
        &emp_code,
        "Leave Balance Employee",
        "482915",
        UserRole::Employee,
        None,
    )
    .await
    .expect("employee");
    let _manager = create_employee(
        &pool,
        &mgr_code,
        "Leave Balance Manager",
        "482915",
        UserRole::Manager,
        None,
    )
    .await
    .expect("manager");

    set_balance(&pool, employee.id, LeaveRequestType::Vacation, 1.0)
        .await
        .expect("set balance");

    let start = Date::from_calendar_date(2099, Month::March, 1).unwrap();
    let end = Date::from_calendar_date(2099, Month::March, 5).unwrap();
    let err = create_request(
        &pool,
        employee.id,
        start,
        end,
        LeaveDayPortion::FullDay,
        LeaveRequestType::Vacation,
        Some("Need more days than balance"),
    )
    .await
    .expect_err("should fail");
    assert!(err.to_string().to_lowercase().contains("insufficient"));

    cleanup_employee(&pool, &emp_code).await;
    cleanup_employee(&pool, &mgr_code).await;
}

#[tokio::test]
async fn approving_leave_deducts_balance() {
    let Some(pool) = common::test_pool().await else {
        eprintln!("skipping: DATABASE_URL not available");
        return;
    };

    let emp_code = unique_code("LVAP");
    let mgr_code = unique_code("LVAM");
    let employee = create_employee(
        &pool,
        &emp_code,
        "Leave Approve Employee",
        "482915",
        UserRole::Employee,
        None,
    )
    .await
    .expect("employee");
    let manager = create_employee(
        &pool,
        &mgr_code,
        "Leave Approve Manager",
        "482915",
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

    set_balance(&pool, employee.id, LeaveRequestType::Vacation, 5.0)
        .await
        .expect("set balance");

    let start = Date::from_calendar_date(2099, Month::April, 10).unwrap();
    let end = Date::from_calendar_date(2099, Month::April, 12).unwrap();
    let req = create_request(
        &pool,
        employee.id,
        start,
        end,
        LeaveDayPortion::FullDay,
        LeaveRequestType::Vacation,
        Some("Short break"),
    )
    .await
    .expect("create");

    review_request(&pool, req.id, manager.id, false, true, Some("ok"))
        .await
        .expect("approve");

    let snapshot = get_snapshot(&pool, employee.id).await.expect("snapshot");
    assert_eq!(snapshot.vacation_tenths, 20);

    cleanup_employee(&pool, &emp_code).await;
    cleanup_employee(&pool, &mgr_code).await;
}

#[tokio::test]
async fn stacked_pending_leave_respects_reserved_balance() {
    let Some(pool) = common::test_pool().await else {
        eprintln!("skipping: DATABASE_URL not available");
        return;
    };

    let emp_code = unique_code("LVST");
    let employee = create_employee(
        &pool,
        &emp_code,
        "Stacked Leave Employee",
        "482915",
        UserRole::Employee,
        None,
    )
    .await
    .expect("employee");

    set_balance(&pool, employee.id, LeaveRequestType::Vacation, 5.0)
        .await
        .expect("set balance");

    let first_start = Date::from_calendar_date(2099, Month::May, 1).unwrap();
    let first_end = Date::from_calendar_date(2099, Month::May, 3).unwrap();
    create_request(
        &pool,
        employee.id,
        first_start,
        first_end,
        LeaveDayPortion::FullDay,
        LeaveRequestType::Vacation,
        Some("First pending block"),
    )
    .await
    .expect("first request");

    let second_start = Date::from_calendar_date(2099, Month::May, 10).unwrap();
    let second_end = Date::from_calendar_date(2099, Month::May, 12).unwrap();
    let err = create_request(
        &pool,
        employee.id,
        second_start,
        second_end,
        LeaveDayPortion::FullDay,
        LeaveRequestType::Vacation,
        Some("Second pending block"),
    )
    .await
    .expect_err("reserved balance should block second request");
    assert!(err.to_string().to_lowercase().contains("insufficient"));
    assert!(err.to_string().contains("reserved"));

    cleanup_employee(&pool, &emp_code).await;
}

#[tokio::test]
async fn approved_leave_cannot_be_reviewed_twice() {
    let Some(pool) = common::test_pool().await else {
        eprintln!("skipping: DATABASE_URL not available");
        return;
    };

    let emp_code = unique_code("LVRV");
    let mgr_code = unique_code("LVRM");
    let employee = create_employee(
        &pool,
        &emp_code,
        "Review Twice Employee",
        "482915",
        UserRole::Employee,
        None,
    )
    .await
    .expect("employee");
    let manager = create_employee(
        &pool,
        &mgr_code,
        "Review Twice Manager",
        "482915",
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

    set_balance(&pool, employee.id, LeaveRequestType::SickLeave, 3.0)
        .await
        .expect("set balance");

    let start = Date::from_calendar_date(2099, Month::July, 1).unwrap();
    let req = create_request(
        &pool,
        employee.id,
        start,
        start,
        LeaveDayPortion::FullDay,
        LeaveRequestType::SickLeave,
        Some("One day sick"),
    )
    .await
    .expect("create");

    review_request(&pool, req.id, manager.id, false, true, Some("approved"))
        .await
        .expect("first review");

    let err = review_request(&pool, req.id, manager.id, false, true, Some("again"))
        .await
        .expect_err("second review should fail");
    assert!(err.to_string().to_lowercase().contains("pending"));

    cleanup_employee(&pool, &emp_code).await;
    cleanup_employee(&pool, &mgr_code).await;
}

#[tokio::test]
async fn cancelling_approved_leave_refunds_balance() {
    let Some(pool) = common::test_pool().await else {
        eprintln!("skipping: DATABASE_URL not available");
        return;
    };

    let emp_code = unique_code("LVCF");
    let mgr_code = unique_code("LVCM");
    let employee = create_employee(
        &pool,
        &emp_code,
        "Cancel Refund Employee",
        "482915",
        UserRole::Employee,
        None,
    )
    .await
    .expect("employee");
    let manager = create_employee(
        &pool,
        &mgr_code,
        "Cancel Refund Manager",
        "482915",
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

    set_balance(&pool, employee.id, LeaveRequestType::Vacation, 5.0)
        .await
        .expect("set balance");

    let start = Date::from_calendar_date(2099, Month::August, 4).unwrap();
    let end = Date::from_calendar_date(2099, Month::August, 6).unwrap();
    let req = create_request(
        &pool,
        employee.id,
        start,
        end,
        LeaveDayPortion::FullDay,
        LeaveRequestType::Vacation,
        Some("Approved then cancelled"),
    )
    .await
    .expect("create");

    review_request(&pool, req.id, manager.id, false, true, Some("ok"))
        .await
        .expect("approve");

    let after_approve = get_snapshot(&pool, employee.id).await.expect("snapshot");
    assert_eq!(after_approve.vacation_tenths, 20);

    cancel_request(&pool, employee.id, req.id)
        .await
        .expect("cancel approved");

    let after_cancel = get_snapshot(&pool, employee.id).await.expect("snapshot");
    assert_eq!(after_cancel.vacation_tenths, 50);

    let attendance_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM time_entries
         WHERE employee_id = $1 AND work_date BETWEEN $2 AND $3",
    )
    .bind(employee.id)
    .bind(start)
    .bind(end)
    .fetch_one(&pool)
    .await
    .expect("attendance count");
    assert_eq!(attendance_count, 0);

    cleanup_employee(&pool, &emp_code).await;
    cleanup_employee(&pool, &mgr_code).await;
}

#[tokio::test]
async fn parallel_leave_review_only_one_outcome() {
    let Some(pool) = common::test_pool().await else {
        eprintln!("skipping: DATABASE_URL not available");
        return;
    };

    let emp_code = unique_code("LVPR");
    let mgr_code = unique_code("LVPM");
    let employee = create_employee(
        &pool,
        &emp_code,
        "Parallel Review Employee",
        "482915",
        UserRole::Employee,
        None,
    )
    .await
    .expect("employee");
    let manager = create_employee(
        &pool,
        &mgr_code,
        "Parallel Review Manager",
        "482915",
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

    set_balance(&pool, employee.id, LeaveRequestType::Vacation, 5.0)
        .await
        .expect("set balance");

    let start = Date::from_calendar_date(2099, Month::September, 1).unwrap();
    let end = Date::from_calendar_date(2099, Month::September, 3).unwrap();
    let req = create_request(
        &pool,
        employee.id,
        start,
        end,
        LeaveDayPortion::FullDay,
        LeaveRequestType::Vacation,
        Some("Parallel stress"),
    )
    .await
    .expect("create");

    let pool_a = std::sync::Arc::new(pool.clone());
    let pool_b = std::sync::Arc::new(pool.clone());
    let request_id = req.id;
    let manager_id = manager.id;

    let (approve_result, reject_result) = tokio::join!(
        async move {
            review_request(
                &pool_a,
                request_id,
                manager_id,
                false,
                true,
                Some("approve"),
            )
            .await
        },
        async move {
            review_request(
                &pool_b,
                request_id,
                manager_id,
                false,
                false,
                Some("reject"),
            )
            .await
        },
    );

    let success_count = usize::from(approve_result.is_ok()) + usize::from(reject_result.is_ok());
    assert_eq!(
        success_count, 1,
        "exactly one review should succeed: approve={approve_result:?} reject={reject_result:?}"
    );

    let status: dtr::models::LeaveRequestStatus =
        sqlx::query_scalar("SELECT status FROM leave_requests WHERE id = $1")
            .bind(request_id)
            .fetch_one(&pool)
            .await
            .expect("status");

    let snapshot = get_snapshot(&pool, employee.id).await.expect("snapshot");
    match status {
        dtr::models::LeaveRequestStatus::Approved => {
            assert_eq!(snapshot.vacation_tenths, 20);
        }
        dtr::models::LeaveRequestStatus::Rejected => {
            assert_eq!(snapshot.vacation_tenths, 50);
        }
        other => panic!("unexpected final status: {other:?}"),
    }

    cleanup_employee(&pool, &emp_code).await;
    cleanup_employee(&pool, &mgr_code).await;
}

#[tokio::test]
async fn overlapping_leave_rejected_by_database_constraint() {
    let Some(pool) = common::test_pool().await else {
        eprintln!("skipping: DATABASE_URL not available");
        return;
    };

    let emp_code = unique_code("LVOV");
    let employee = create_employee(
        &pool,
        &emp_code,
        "Overlap DB Employee",
        "482915",
        UserRole::Employee,
        None,
    )
    .await
    .expect("employee");

    set_balance(&pool, employee.id, LeaveRequestType::Vacation, 10.0)
        .await
        .expect("set balance");

    let start = Date::from_calendar_date(2099, Month::October, 5).unwrap();
    let end = Date::from_calendar_date(2099, Month::October, 7).unwrap();
    create_request(
        &pool,
        employee.id,
        start,
        end,
        LeaveDayPortion::FullDay,
        LeaveRequestType::Vacation,
        Some("First block"),
    )
    .await
    .expect("first request");

    let overlap_start = Date::from_calendar_date(2099, Month::October, 7).unwrap();
    let overlap_end = Date::from_calendar_date(2099, Month::October, 9).unwrap();
    let err = sqlx::query(
        "INSERT INTO leave_requests (employee_id, start_date, end_date, leave_type, reason, status)
         VALUES ($1, $2, $3, 'vacation', 'Overlap insert', 'pending')",
    )
    .bind(employee.id)
    .bind(overlap_start)
    .bind(overlap_end)
    .execute(&pool)
    .await
    .expect_err("overlap should violate exclusion constraint");

    let message = err.to_string().to_lowercase();
    assert!(
        message.contains("leave_requests_no_overlap_pending_approved")
            || message.contains("overlap"),
        "unexpected error: {err}"
    );

    cleanup_employee(&pool, &emp_code).await;
}
