use sqlx::PgPool;
use time::Date;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::models::{EmployeeProfile, EmployeeWorkProfile};

pub struct AdminProfileInput<'a> {
    pub contact_number: Option<&'a str>,
    pub personal_email: Option<&'a str>,
    pub birthdate: Option<Date>,
    pub address: Option<&'a str>,
    pub emergency_contact_name: Option<&'a str>,
    pub emergency_contact_phone: Option<&'a str>,
    pub job_title: Option<&'a str>,
    pub department: Option<&'a str>,
    pub employment_type: Option<&'a str>,
    pub date_hired: Option<Date>,
    pub work_location: Option<&'a str>,
    pub bank_account: Option<&'a str>,
    pub tin: Option<&'a str>,
    pub sss_number: Option<&'a str>,
    pub philhealth_number: Option<&'a str>,
}

const PROFILE_COLUMNS: &str = "employee_id, contact_number, personal_email, birthdate, address,
                emergency_contact_name, emergency_contact_phone, job_title, department,
                employment_type, date_hired, work_location, bank_account, tin, sss_number,
                philhealth_number, photo_path, updated_at, updated_by";

fn empty_to_none(value: Option<&str>) -> Option<String> {
    value.map(str::trim).and_then(|v| {
        if v.is_empty() {
            None
        } else {
            Some(v.to_string())
        }
    })
}

pub async fn ensure_profile(pool: &PgPool, employee_id: Uuid) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO employee_profiles (employee_id) VALUES ($1) ON CONFLICT (employee_id) DO NOTHING",
    )
    .bind(employee_id)
    .execute(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(())
}

pub async fn get_profile(pool: &PgPool, employee_id: Uuid) -> AppResult<EmployeeProfile> {
    ensure_profile(pool, employee_id).await?;
    sqlx::query_as::<_, EmployeeProfile>(&format!(
        "SELECT {PROFILE_COLUMNS} FROM employee_profiles WHERE employee_id = $1"
    ))
    .bind(employee_id)
    .fetch_one(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))
}

pub async fn update_self_service(
    pool: &PgPool,
    employee_id: Uuid,
    contact_number: Option<&str>,
    personal_email: Option<&str>,
) -> AppResult<EmployeeProfile> {
    ensure_profile(pool, employee_id).await?;
    sqlx::query_as::<_, EmployeeProfile>(&format!(
        "UPDATE employee_profiles
         SET contact_number = $2,
             personal_email = $3,
             updated_at = now(),
             updated_by = $1
         WHERE employee_id = $1
         RETURNING {PROFILE_COLUMNS}"
    ))
    .bind(employee_id)
    .bind(empty_to_none(contact_number))
    .bind(empty_to_none(personal_email))
    .fetch_one(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))
}

pub async fn update_admin(
    pool: &PgPool,
    employee_id: Uuid,
    editor_id: Uuid,
    input: AdminProfileInput<'_>,
) -> AppResult<EmployeeProfile> {
    ensure_profile(pool, employee_id).await?;
    sqlx::query_as::<_, EmployeeProfile>(&format!(
        "UPDATE employee_profiles
         SET contact_number = $3,
             personal_email = $4,
             birthdate = $5,
             address = $6,
             emergency_contact_name = $7,
             emergency_contact_phone = $8,
             job_title = $9,
             department = $10,
             employment_type = $11,
             date_hired = $12,
             work_location = $13,
             bank_account = $14,
             tin = $15,
             sss_number = $16,
             philhealth_number = $17,
             updated_at = now(),
             updated_by = $2
         WHERE employee_id = $1
         RETURNING {PROFILE_COLUMNS}"
    ))
    .bind(employee_id)
    .bind(editor_id)
    .bind(empty_to_none(input.contact_number))
    .bind(empty_to_none(input.personal_email))
    .bind(input.birthdate)
    .bind(empty_to_none(input.address))
    .bind(empty_to_none(input.emergency_contact_name))
    .bind(empty_to_none(input.emergency_contact_phone))
    .bind(empty_to_none(input.job_title))
    .bind(empty_to_none(input.department))
    .bind(empty_to_none(input.employment_type))
    .bind(input.date_hired)
    .bind(empty_to_none(input.work_location))
    .bind(empty_to_none(input.bank_account))
    .bind(empty_to_none(input.tin))
    .bind(empty_to_none(input.sss_number))
    .bind(empty_to_none(input.philhealth_number))
    .fetch_one(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))
}

pub async fn set_photo_path(
    pool: &PgPool,
    employee_id: Uuid,
    editor_id: Uuid,
    photo_path: Option<&str>,
) -> AppResult<()> {
    ensure_profile(pool, employee_id).await?;
    sqlx::query(
        "UPDATE employee_profiles
         SET photo_path = $3, updated_at = now(), updated_by = $2
         WHERE employee_id = $1",
    )
    .bind(employee_id)
    .bind(editor_id)
    .bind(photo_path)
    .execute(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(())
}

pub async fn get_work_profile(pool: &PgPool, employee_id: Uuid) -> AppResult<EmployeeWorkProfile> {
    ensure_profile(pool, employee_id).await?;
    sqlx::query_as::<_, EmployeeWorkProfile>(
        "SELECT e.id AS employee_id, e.employee_code, e.full_name,
                p.job_title, p.department, p.employment_type, p.date_hired, p.work_location
         FROM employees e
         JOIN employee_profiles p ON p.employee_id = e.id
         WHERE e.id = $1",
    )
    .bind(employee_id)
    .fetch_one(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))
}

pub async fn get_department(pool: &PgPool, employee_id: Uuid) -> AppResult<Option<String>> {
    ensure_profile(pool, employee_id).await?;
    let dept: Option<String> =
        sqlx::query_scalar("SELECT department FROM employee_profiles WHERE employee_id = $1")
            .bind(employee_id)
            .fetch_one(pool)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;
    Ok(dept.filter(|d| !d.trim().is_empty()))
}