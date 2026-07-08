use anyhow::Context;
use dtr::auth::pin::hash_pin;
use uuid::Uuid;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    let database_url = std::env::var("DATABASE_URL").context("DATABASE_URL must be set")?;
    let pool = dtr::db::connect(&database_url).await?;

    let rows: Vec<(String, String, bool)> = sqlx::query_as(
        "SELECT employee_code, role::text, is_active FROM employees ORDER BY employee_code",
    )
    .fetch_all(&pool)
    .await?;

    if rows.is_empty() {
        println!("No employees in database.");
    } else {
        println!("Employees in database:");
        for (code, role, active) in &rows {
            println!("  {code} ({role}) active={active}");
        }
    }

    let pin_hash = hash_pin("1234")?;
    let admin_id: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM employees WHERE employee_code = 'ADMIN' LIMIT 1",
    )
    .fetch_optional(&pool)
    .await?;

    if let Some(id) = admin_id {
        sqlx::query(
            "UPDATE employees
             SET pin_hash = $2, is_active = TRUE, must_change_pin = TRUE, role = 'admin',
                 session_version = session_version + 1
             WHERE id = $1",
        )
        .bind(id)
        .bind(pin_hash)
        .execute(&pool)
        .await?;
        println!("Reset existing ADMIN account.");
    } else {
        sqlx::query(
            "INSERT INTO employees (employee_code, full_name, pin_hash, role, is_active, must_change_pin)
             VALUES ('ADMIN', 'System Administrator', $1, 'admin', TRUE, TRUE)",
        )
        .bind(pin_hash)
        .execute(&pool)
        .await?;
        println!("Created ADMIN account.");
    }

    println!();
    println!("Admin login ready:");
    println!("  Employee code: ADMIN");
    println!("  PIN: 1234");
    Ok(())
}