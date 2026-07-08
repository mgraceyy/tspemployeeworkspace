use anyhow::Context;
use dtr::auth::pin::hash_pin;
use dtr::models::{LeaveRequestType, UserRole};
use dtr::services::employees::{create_employee, find_by_code};
use dtr::services::leave_balances::set_balance;
use dtr::services::profile::set_department;
use dtr::services::shifts::upsert_shift;
use time::Time;
use uuid::Uuid;

const SAMPLE_CODE: &str = "SAMPLE01";
const SAMPLE_PIN: &str = "7391";
const SAMPLE_NAME: &str = "Alex Rivera";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    let database_url = std::env::var("DATABASE_URL").context("DATABASE_URL must be set")?;
    let pool = dtr::db::connect(&database_url).await?;

    let employee_id = if let Some(existing) = find_by_code(&pool, SAMPLE_CODE).await? {
        println!("Sample employee already exists — refreshing setup.");
        existing.id
    } else {
        let manager_id = if let Some(manager) = find_by_code(&pool, "E2MGR").await? {
            Some(manager.id)
        } else {
            sqlx::query_scalar(
                "SELECT id FROM employees
                 WHERE is_active = TRUE AND role IN ('manager', 'admin')
                 ORDER BY employee_code LIMIT 1",
            )
            .fetch_optional(&pool)
            .await?
        };

        let created = create_employee(
            &pool,
            SAMPLE_CODE,
            SAMPLE_NAME,
            SAMPLE_PIN,
            UserRole::Employee,
            manager_id,
        )
        .await?;
        println!("Created sample employee.");
        created.id
    };

    let pin_hash = hash_pin(SAMPLE_PIN)?;
    sqlx::query(
        "UPDATE employees
         SET full_name = $2, pin_hash = $3, is_active = TRUE, must_change_pin = FALSE, role = 'employee'
         WHERE id = $1",
    )
    .bind(employee_id)
    .bind(SAMPLE_NAME)
    .bind(pin_hash)
    .execute(&pool)
    .await?;

    set_department(&pool, employee_id, "Engineering").await?;
    set_balance(&pool, employee_id, LeaveRequestType::Vacation, 12.0).await?;
    set_balance(&pool, employee_id, LeaveRequestType::SickLeave, 10.0).await?;

    sqlx::query(
        "UPDATE employee_profiles
         SET job_title = 'Software Engineer',
             employment_type = 'Regular',
             work_location = 'Main Office',
             contact_number = '09171234567',
             personal_email = 'alex.rivera@example.com'
         WHERE employee_id = $1",
    )
    .bind(employee_id)
    .execute(&pool)
    .await?;

    seed_weekday_shifts(&pool, employee_id).await?;

    println!();
    println!("Sample employee ready for the employee portal:");
    println!("  Employee code: {SAMPLE_CODE}");
    println!("  PIN: {SAMPLE_PIN}");
    println!("  Name: {SAMPLE_NAME}");
    println!("  Department: Engineering");
    println!("  Shifts: Mon–Fri 08:00–17:00");
    println!();
    println!("Log in at http://localhost:8080/login");
    Ok(())
}

async fn seed_weekday_shifts(pool: &sqlx::PgPool, employee_id: Uuid) -> anyhow::Result<()> {
    let start = Time::from_hms(8, 0, 0).context("invalid start time")?;
    let end = Time::from_hms(17, 0, 0).context("invalid end time")?;
    for day in 1..=5 {
        upsert_shift(pool, employee_id, day, start, end).await?;
    }
    let off = Time::from_hms(0, 0, 0).context("invalid off time")?;
    for day in [0i16, 6] {
        upsert_shift(pool, employee_id, day, off, off).await?;
    }
    Ok(())
}