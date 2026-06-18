use std::collections::HashMap;

use sqlx::PgPool;
use uuid::Uuid;

use super::deductions::list_deduction_types;
use super::runs::list_lines_for_run;
use crate::error::{AppError, AppResult};
use crate::models::{PayrollRun, PayrollRunStatus};
use crate::services::compensation::format_salary_cents;

#[derive(Debug, sqlx::FromRow)]
struct LineDeductionRow {
    line_id: Uuid,
    code: String,
    amount_cents: i64,
}

async fn deductions_by_line(
    pool: &PgPool,
    line_ids: &[Uuid],
) -> AppResult<HashMap<Uuid, HashMap<String, i64>>> {
    if line_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let rows = sqlx::query_as::<_, LineDeductionRow>(
        "SELECT d.line_id, t.code, d.amount_cents
         FROM payroll_deductions d
         JOIN deduction_types t ON t.id = d.deduction_type_id
         WHERE d.line_id = ANY($1)
         ORDER BY t.code",
    )
    .bind(line_ids)
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    let mut map: HashMap<Uuid, HashMap<String, i64>> = HashMap::new();
    for row in rows {
        map.entry(row.line_id)
            .or_default()
            .insert(row.code, row.amount_cents);
    }
    Ok(map)
}

pub async fn build_finalized_run_csv(
    pool: &PgPool,
    run: &PayrollRun,
    period_label: &str,
) -> AppResult<Vec<u8>> {
    if run.status != PayrollRunStatus::Finalized {
        return Err(AppError::bad_request(
            "Only finalized payroll runs can be exported",
        ));
    }

    let lines = list_lines_for_run(pool, run.id).await?;
    let types = list_deduction_types(pool).await?;
    let line_ids: Vec<Uuid> = lines.iter().map(|l| l.id).collect();
    let deductions = deductions_by_line(pool, &line_ids).await?;

    let mut csv_bytes = Vec::new();
    {
        let mut writer = csv::Writer::from_writer(&mut csv_bytes);
        writer
            .write_record(["Pay period", period_label])
            .map_err(|e| AppError::Internal(e.into()))?;

        let mut headers = vec![
            "Employee Code".to_string(),
            "Name".to_string(),
            "Department".to_string(),
            "Regular Minutes".to_string(),
            "Approved OT Minutes".to_string(),
            "No-show Days".to_string(),
            "Base Pay".to_string(),
            "No-show Deduction".to_string(),
            "OT Pay".to_string(),
            "Gross Pay".to_string(),
        ];
        for dtype in &types {
            headers.push(dtype.name.clone());
        }
        headers.push("Total Deductions".to_string());
        headers.push("Net Pay".to_string());
        writer
            .write_record(&headers)
            .map_err(|e| AppError::Internal(e.into()))?;

        for line in &lines {
            let line_deductions = deductions.get(&line.id);
            let mut record = vec![
                line.employee_code.clone(),
                line.full_name.clone(),
                line.department.clone().unwrap_or_default(),
                line.regular_minutes.to_string(),
                line.approved_ot_minutes.to_string(),
                line.no_show_days.to_string(),
                format_salary_cents(line.base_pay_cents),
                format_salary_cents(line.no_show_deduction_cents),
                format_salary_cents(line.ot_pay_cents),
                format_salary_cents(line.gross_pay_cents),
            ];
            for dtype in &types {
                let amount = line_deductions
                    .and_then(|m| m.get(&dtype.code))
                    .copied()
                    .unwrap_or(0);
                record.push(if amount > 0 {
                    format_salary_cents(amount)
                } else {
                    String::new()
                });
            }
            record.push(format_salary_cents(line.total_deduction_cents));
            record.push(format_salary_cents(line.net_pay_cents));
            writer
                .write_record(&record)
                .map_err(|e| AppError::Internal(e.into()))?;
        }
        writer.flush().map_err(|e| AppError::Internal(e.into()))?;
    }
    Ok(csv_bytes)
}
