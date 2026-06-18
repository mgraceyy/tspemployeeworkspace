use sqlx::PgPool;
use time::Date;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::services::compensation::{parse_allowance_to_cents, parse_salary_to_cents, upsert_profile};
use crate::services::employees::find_by_code;
use crate::services::timezone::parse_date;

#[derive(Debug, Clone)]
pub struct ImportRow {
    pub line_number: usize,
    pub employee_code: String,
    pub employee_id: Option<Uuid>,
    pub full_name: Option<String>,
    pub monthly_salary_cents: i64,
    pub ot_rate_percent: i32,
    pub transport_allowance_cents: i64,
    pub meal_allowance_cents: i64,
    pub effective_from: Option<Date>,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ImportPreview {
    pub rows: Vec<ImportRow>,
    pub valid_count: usize,
    pub error_count: usize,
}

pub fn parse_import_csv(bytes: &[u8]) -> AppResult<ImportPreview> {
    let mut reader = csv::Reader::from_reader(bytes);
    let headers = reader
        .headers()
        .map_err(|e| AppError::bad_request(format!("Invalid CSV header: {e}")))?
        .clone();

    let idx_code = header_index(&headers, &["employee_code", "code"])?;
    let idx_salary = header_index(&headers, &["monthly_salary", "salary"])?;
    let idx_ot = find_optional_index(&headers, &["ot_rate_percent", "ot_rate"]);
    let idx_transport = find_optional_index(&headers, &["transport_allowance", "transport"]);
    let idx_meal = find_optional_index(&headers, &["meal_allowance", "meal"]);
    let idx_effective = header_index(&headers, &["effective_from", "effective"])?;

    let mut rows = Vec::new();
    let mut valid_count = 0usize;
    let mut error_count = 0usize;

    for (offset, result) in reader.records().enumerate() {
        let record = result.map_err(|e| AppError::bad_request(format!("CSV row error: {e}")))?;
        let line_number = offset + 2;
        let code = record
            .get(idx_code)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_uppercase)
            .unwrap_or_default();

        if code.is_empty() {
            rows.push(ImportRow {
                line_number,
                employee_code: String::new(),
                employee_id: None,
                full_name: None,
                monthly_salary_cents: 0,
                ot_rate_percent: 132,
                transport_allowance_cents: 0,
                meal_allowance_cents: 0,
                effective_from: None,
                error: Some("employee_code is required".into()),
            });
            error_count += 1;
            continue;
        }

        let mut row_error = None;
        let monthly_salary_cents =
            match parse_salary_to_cents(record.get(idx_salary).unwrap_or("")) {
                Ok(v) => v,
                Err(e) => {
                    row_error = Some(e.to_string());
                    0
                }
            };
        let ot_rate_percent = idx_ot
            .and_then(|i| record.get(i))
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.parse::<i32>())
            .transpose()
            .map_err(|_| AppError::bad_request("ot_rate_percent must be a number"))?
            .unwrap_or(132);
        let transport_allowance_cents = idx_transport
            .and_then(|i| record.get(i))
            .map(str::trim)
            .unwrap_or("");
        let transport_allowance_cents = parse_allowance_to_cents(transport_allowance_cents)
            .unwrap_or_else(|e| {
                row_error.get_or_insert_with(|| e.to_string());
                0
            });
        let meal_allowance_cents = idx_meal
            .and_then(|i| record.get(i))
            .map(str::trim)
            .unwrap_or("");
        let meal_allowance_cents = parse_allowance_to_cents(meal_allowance_cents).unwrap_or_else(
            |e| {
                row_error.get_or_insert_with(|| e.to_string());
                0
            },
        );
        let effective_from = match record.get(idx_effective).map(str::trim).filter(|s| !s.is_empty()) {
            Some(s) => match parse_date(s) {
                Ok(d) => Some(d),
                Err(e) => {
                    row_error = Some(e.to_string());
                    None
                }
            },
            None => {
                row_error = Some("effective_from is required".into());
                None
            }
        };

        if row_error.is_none() {
            if !(100..=300).contains(&ot_rate_percent) {
                row_error = Some("ot_rate_percent must be between 100 and 300".into());
            }
        }

        if row_error.is_some() {
            error_count += 1;
        } else {
            valid_count += 1;
        }

        rows.push(ImportRow {
            line_number,
            employee_code: code,
            employee_id: None,
            full_name: None,
            monthly_salary_cents,
            ot_rate_percent,
            transport_allowance_cents,
            meal_allowance_cents,
            effective_from,
            error: row_error,
        });
    }

    if rows.is_empty() {
        return Err(AppError::bad_request("CSV has no data rows"));
    }

    Ok(ImportPreview {
        rows,
        valid_count,
        error_count,
    })
}

pub async fn resolve_import_rows(pool: &PgPool, preview: &mut ImportPreview) -> AppResult<()> {
    for row in &mut preview.rows {
        if row.error.is_some() {
            continue;
        }
        match find_by_code(pool, &row.employee_code).await? {
            Some(emp) => {
                row.employee_id = Some(emp.id);
                row.full_name = Some(emp.full_name);
            }
            None => {
                row.error = Some(format!("Unknown employee code: {}", row.employee_code));
                preview.valid_count = preview.valid_count.saturating_sub(1);
                preview.error_count += 1;
            }
        }
    }
    Ok(())
}

pub async fn apply_import(
    pool: &PgPool,
    preview: &ImportPreview,
    editor_id: Uuid,
) -> AppResult<usize> {
    let mut applied = 0usize;
    for row in &preview.rows {
        if row.error.is_some() {
            continue;
        }
        let employee_id = row
            .employee_id
            .ok_or_else(|| AppError::bad_request("Import row missing employee_id"))?;
        upsert_profile(
            pool,
            employee_id,
            row.monthly_salary_cents,
            row.ot_rate_percent,
            row.transport_allowance_cents,
            row.meal_allowance_cents,
            row.effective_from
                .ok_or_else(|| AppError::bad_request("Row missing effective_from"))?,
            editor_id,
        )
        .await?;
        applied += 1;
    }
    Ok(applied)
}

fn header_index(headers: &csv::StringRecord, names: &[&str]) -> AppResult<usize> {
    for (idx, header) in headers.iter().enumerate() {
        let normalized = header.trim().to_ascii_lowercase();
        if names.iter().any(|name| normalized == *name) {
            return Ok(idx);
        }
    }
    Err(AppError::bad_request(format!(
        "CSV must include column: {}",
        names[0]
    )))
}

fn find_optional_index(headers: &csv::StringRecord, names: &[&str]) -> Option<usize> {
    for (idx, header) in headers.iter().enumerate() {
        let normalized = header.trim().to_ascii_lowercase();
        if names.iter().any(|name| normalized == *name) {
            return Some(idx);
        }
    }
    None
}