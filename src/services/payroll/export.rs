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
            "Allowances".to_string(),
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
                format_salary_cents(line.allowance_cents),
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

#[derive(Debug, sqlx::FromRow)]
struct BankExportRow {
    employee_code: String,
    full_name: String,
    bank_account: Option<String>,
    net_pay_cents: i64,
}

pub async fn build_bank_upload_csv(pool: &PgPool, run: &PayrollRun) -> AppResult<Vec<u8>> {
    if run.status != PayrollRunStatus::Finalized {
        return Err(AppError::bad_request(
            "Only finalized payroll runs can be exported",
        ));
    }

    let rows = sqlx::query_as::<_, BankExportRow>(
        "SELECT e.employee_code, e.full_name, p.bank_account, l.net_pay_cents
         FROM payroll_lines l
         JOIN employees e ON e.id = l.employee_id
         LEFT JOIN employee_profiles p ON p.employee_id = e.id
         WHERE l.run_id = $1
         ORDER BY e.full_name",
    )
    .bind(run.id)
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    let mut csv_bytes = Vec::new();
    {
        let mut writer = csv::Writer::from_writer(&mut csv_bytes);
        writer
            .write_record(["employee_code", "full_name", "bank_account", "net_pay"])
            .map_err(|e| AppError::Internal(e.into()))?;
        for row in rows {
            writer
                .write_record([
                    row.employee_code,
                    row.full_name,
                    row.bank_account.unwrap_or_default(),
                    format_salary_cents(row.net_pay_cents),
                ])
                .map_err(|e| AppError::Internal(e.into()))?;
        }
        writer.flush().map_err(|e| AppError::Internal(e.into()))?;
    }
    Ok(csv_bytes)
}

pub async fn build_journal_export_csv(
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
            .write_record([
                "pay_period",
                "employee_code",
                "employee_name",
                "account",
                "description",
                "debit",
                "credit",
            ])
            .map_err(|e| AppError::Internal(e.into()))?;

        for line in &lines {
            writer
                .write_record([
                    period_label.to_string(),
                    line.employee_code.clone(),
                    line.full_name.clone(),
                    "5100".to_string(),
                    "Salaries expense".to_string(),
                    format_salary_cents(line.gross_pay_cents),
                    String::new(),
                ])
                .map_err(|e| AppError::Internal(e.into()))?;

            let line_deductions = deductions.get(&line.id);
            for dtype in &types {
                let amount = line_deductions
                    .and_then(|m| m.get(&dtype.code))
                    .copied()
                    .unwrap_or(0);
                if amount <= 0 {
                    continue;
                }
                writer
                    .write_record([
                        period_label.to_string(),
                        line.employee_code.clone(),
                        line.full_name.clone(),
                        dtype.code.clone(),
                        dtype.name.clone(),
                        String::new(),
                        format_salary_cents(amount),
                    ])
                    .map_err(|e| AppError::Internal(e.into()))?;
            }

            writer
                .write_record([
                    period_label.to_string(),
                    line.employee_code.clone(),
                    line.full_name.clone(),
                    "2100".to_string(),
                    "Net pay payable".to_string(),
                    String::new(),
                    format_salary_cents(line.net_pay_cents),
                ])
                .map_err(|e| AppError::Internal(e.into()))?;
        }
        writer.flush().map_err(|e| AppError::Internal(e.into()))?;
    }
    Ok(csv_bytes)
}
