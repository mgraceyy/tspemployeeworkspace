use axum::{
    extract::{Multipart, Path, State},
    response::Redirect,
    Form,
};
use minijinja::context;
use serde::{Deserialize, Serialize};
use tower_sessions::Session;
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::error::{AppError, AppResult};
use crate::handlers::flash::redirect_with_flash;
use crate::handlers::render::{render_page, HtmlPage};
use crate::services::{
    audit::log_action,
    compensation::{
        format_salary_cents, get_compensation, list_deduction_defaults, list_history,
        parse_allowance_to_cents, parse_salary_to_cents, save_deduction_defaults,
        upsert_profile, DeductionDefaultInput,
    },
    compensation_import::{apply_import, parse_import_csv, resolve_import_rows, ImportPreview},
    employees::find_by_id,
    payroll::deductions::list_deduction_types,
    settings::get_settings,
    timezone::{format_date, parse_date},
};
use crate::state::AppState;

const IMPORT_PREVIEW_KEY: &str = "comp_import_preview";

#[derive(Serialize, Deserialize)]
struct StoredImportPreview {
    rows: Vec<StoredImportRow>,
    valid_count: usize,
    error_count: usize,
}

#[derive(Serialize, Deserialize, Clone)]
struct StoredImportRow {
    line_number: usize,
    employee_code: String,
    employee_id: Option<Uuid>,
    full_name: Option<String>,
    monthly_salary_cents: i64,
    ot_rate_percent: i32,
    transport_allowance_cents: i64,
    meal_allowance_cents: i64,
    effective_from: Option<String>,
    error: Option<String>,
}

#[derive(Deserialize)]
pub struct CompensationForm {
    monthly_salary: String,
    ot_rate_percent: Option<i32>,
    transport_allowance: Option<String>,
    meal_allowance: Option<String>,
    effective_from: String,
}

pub async fn compensation_page(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
    Path(employee_id): Path<Uuid>,
) -> AppResult<HtmlPage> {
    let settings = get_settings(&state.pool).await?;
    let employee = find_by_id(&state.pool, employee_id)
        .await?
        .ok_or(AppError::NotFound)?;
    let profile = get_compensation(&state.pool, employee_id).await?;
    let history = list_history(&state.pool, employee_id).await?;
    let types = list_deduction_types(&state.pool).await?;
    let defaults = list_deduction_defaults(&state.pool, employee_id).await?;

    let (monthly_salary, ot_rate_percent, transport, meal, effective_from) =
        if let Some(ref p) = profile {
            (
                format_salary_cents(p.monthly_salary_cents),
                p.ot_rate_percent,
                format_salary_cents(p.transport_allowance_cents),
                format_salary_cents(p.meal_allowance_cents),
                format_date(p.effective_from),
            )
        } else {
            ("0.00".to_string(), 132, "0.00".to_string(), "0.00".to_string(), String::new())
        };

    let history_rows: Vec<_> = history
        .iter()
        .map(|h| {
            context! {
                salary => format_salary_cents(h.monthly_salary_cents),
                transport => format_salary_cents(h.transport_allowance_cents),
                meal => format_salary_cents(h.meal_allowance_cents),
                ot_rate => h.ot_rate_percent,
                effective_from => format_date(h.effective_from),
                effective_to => h.effective_to.map(format_date).unwrap_or_default(),
            }
        })
        .collect();

    let default_rows: Vec<_> = types
        .iter()
        .map(|t| {
            let amount = defaults
                .iter()
                .find(|(id, _)| *id == t.id)
                .map(|(_, cents)| format_salary_cents(*cents))
                .unwrap_or_default();
            context! {
                code => t.code.clone(),
                code_lower => t.code.to_lowercase(),
                name => t.name.clone(),
                amount => amount,
            }
        })
        .collect();

    render_page(
        &state,
        &session,
        Some(user),
        &settings.company_name,
        "Employee Compensation",
        "admin/compensation.html",
        context! {
            employee_id => employee_id,
            employee_code => employee.employee_code,
            full_name => employee.full_name,
            has_profile => profile.is_some(),
            monthly_salary => monthly_salary,
            transport_allowance => transport,
            meal_allowance => meal,
            ot_rate_percent => ot_rate_percent,
            effective_from => effective_from,
            default_ot_rate => 132,
            working_days => crate::services::payroll::MONTHLY_WORKING_DAYS,
            history => history_rows,
            deduction_defaults => default_rows,
        },
    )
    .await
}

pub async fn save_compensation_action(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
    Path(employee_id): Path<Uuid>,
    Form(form): Form<CompensationForm>,
) -> AppResult<Redirect> {
    let employee = find_by_id(&state.pool, employee_id)
        .await?
        .ok_or(AppError::NotFound)?;

    let monthly_salary_cents = parse_salary_to_cents(&form.monthly_salary)?;
    let transport_allowance_cents =
        parse_allowance_to_cents(form.transport_allowance.as_deref().unwrap_or("0"))?;
    let meal_allowance_cents =
        parse_allowance_to_cents(form.meal_allowance.as_deref().unwrap_or("0"))?;
    let ot_rate_percent = form.ot_rate_percent.unwrap_or(132);
    let effective_from = parse_date(&form.effective_from).map_err(AppError::bad_request)?;

    upsert_profile(
        &state.pool,
        employee_id,
        monthly_salary_cents,
        ot_rate_percent,
        transport_allowance_cents,
        meal_allowance_cents,
        effective_from,
        user.employee_id,
    )
    .await?;

    log_action(
        &state.pool,
        user.employee_id,
        "compensation.updated",
        &format!(
            "Set compensation for {} ({}): PHP {} / month + allowances, OT {}%, effective {}",
            employee.full_name,
            employee.employee_code,
            format_salary_cents(monthly_salary_cents),
            ot_rate_percent,
            format_date(effective_from)
        ),
    )
    .await?;

    redirect_with_flash(
        &session,
        &format!("/admin/employees/{employee_id}/compensation"),
        "success",
        "Compensation saved",
    )
    .await
}

pub async fn save_deduction_defaults_action(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
    Path(employee_id): Path<Uuid>,
    Form(form): Form<std::collections::HashMap<String, String>>,
) -> AppResult<Redirect> {
    let types = list_deduction_types(&state.pool).await?;
    let mut inputs = Vec::new();
    for dtype in &types {
        let key = format!("default_{}", dtype.code.to_lowercase());
        let amount_cents =
            crate::services::payroll::parse_optional_amount_to_cents(
                form.get(&key).map(|s| s.as_str()).unwrap_or(""),
            )?;
        inputs.push(DeductionDefaultInput {
            deduction_type_id: dtype.id,
            amount_cents,
        });
    }
    save_deduction_defaults(&state.pool, employee_id, user.employee_id, &inputs).await?;

    redirect_with_flash(
        &session,
        &format!("/admin/employees/{employee_id}/compensation"),
        "success",
        "Default deductions saved",
    )
    .await
}

pub async fn compensation_import_page(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
) -> AppResult<HtmlPage> {
    let settings = get_settings(&state.pool).await?;
    let preview: Option<StoredImportPreview> = session
        .get(IMPORT_PREVIEW_KEY)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

    let rows: Vec<_> = preview
        .as_ref()
        .map(|p| {
            p.rows
                .iter()
                .map(|r| {
                    context! {
                        line_number => r.line_number,
                        employee_code => r.employee_code.clone(),
                        full_name => r.full_name.clone().unwrap_or_default(),
                        monthly_salary => format_salary_cents(r.monthly_salary_cents),
                        ot_rate_percent => r.ot_rate_percent,
                        transport_allowance => format_salary_cents(r.transport_allowance_cents),
                        meal_allowance => format_salary_cents(r.meal_allowance_cents),
                        effective_from => r.effective_from.clone().unwrap_or_default(),
                        error => r.error.clone().unwrap_or_default(),
                        has_error => r.error.is_some(),
                    }
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    render_page(
        &state,
        &session,
        Some(user),
        &settings.company_name,
        "Import Compensation",
        "admin/compensation_import.html",
        context! {
            has_preview => preview.is_some(),
            valid_count => preview.as_ref().map(|p| p.valid_count).unwrap_or(0),
            error_count => preview.as_ref().map(|p| p.error_count).unwrap_or(0),
            can_apply => preview.as_ref().is_some_and(|p| p.valid_count > 0 && p.error_count == 0),
            rows => rows,
        },
    )
    .await
}

pub async fn compensation_import_preview_action(
    State(state): State<AppState>,
    session: Session,
    AuthUser(_user): AuthUser,
    mut multipart: Multipart,
) -> AppResult<Redirect> {
    let mut bytes = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::Internal(e.into()))?
    {
        if field.name() == Some("csv_file") {
            bytes = Some(
                field
                    .bytes()
                    .await
                    .map_err(|e| AppError::Internal(e.into()))?,
            );
        }
    }
    let bytes = bytes.ok_or_else(|| AppError::bad_request("CSV file is required"))?;
    let mut preview = parse_import_csv(&bytes)?;
    resolve_import_rows(&state.pool, &mut preview).await?;
    store_preview(&session, &preview).await?;
    redirect_with_flash(
        &session,
        "/admin/compensation/import",
        "success",
        "CSV parsed — review rows below",
    )
    .await
}

pub async fn compensation_import_apply_action(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
) -> AppResult<Redirect> {
    let stored: Option<StoredImportPreview> = session
        .get(IMPORT_PREVIEW_KEY)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;
    let Some(stored) = stored else {
        return Err(AppError::bad_request("Upload a CSV preview first"));
    };
    if stored.error_count > 0 || stored.valid_count == 0 {
        return Err(AppError::bad_request(
            "Fix all CSV errors before applying import",
        ));
    }

    let preview = stored_to_preview(stored)?;
    let applied = apply_import(&state.pool, &preview, user.employee_id).await?;
    session
        .remove::<StoredImportPreview>(IMPORT_PREVIEW_KEY)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

    log_action(
        &state.pool,
        user.employee_id,
        "compensation.imported",
        &format!("Imported compensation for {applied} employee(s)"),
    )
    .await?;

    redirect_with_flash(
        &session,
        "/admin/compensation/import",
        "success",
        &format!("Applied compensation for {applied} employee(s)"),
    )
    .await
}

async fn store_preview(session: &Session, preview: &ImportPreview) -> AppResult<()> {
    let stored = StoredImportPreview {
        valid_count: preview.valid_count,
        error_count: preview.error_count,
        rows: preview
            .rows
            .iter()
            .map(|r| StoredImportRow {
                line_number: r.line_number,
                employee_code: r.employee_code.clone(),
                employee_id: r.employee_id,
                full_name: r.full_name.clone(),
                monthly_salary_cents: r.monthly_salary_cents,
                ot_rate_percent: r.ot_rate_percent,
                transport_allowance_cents: r.transport_allowance_cents,
                meal_allowance_cents: r.meal_allowance_cents,
                effective_from: r.effective_from.map(format_date),
                error: r.error.clone(),
            })
            .collect(),
    };
    session
        .insert(IMPORT_PREVIEW_KEY, stored)
        .await
        .map_err(|e| AppError::Internal(e.into()))
}

fn stored_to_preview(stored: StoredImportPreview) -> AppResult<ImportPreview> {
    let mut rows = Vec::new();
    for r in stored.rows {
        let effective_from = match r.effective_from.as_deref() {
            Some(s) if !s.is_empty() => Some(parse_date(s).map_err(AppError::bad_request)?),
            _ => None,
        };
        rows.push(crate::services::compensation_import::ImportRow {
            line_number: r.line_number,
            employee_code: r.employee_code,
            employee_id: r.employee_id,
            full_name: r.full_name,
            monthly_salary_cents: r.monthly_salary_cents,
            ot_rate_percent: r.ot_rate_percent,
            transport_allowance_cents: r.transport_allowance_cents,
            meal_allowance_cents: r.meal_allowance_cents,
            effective_from,
            error: r.error,
        });
    }
    Ok(ImportPreview {
        rows,
        valid_count: stored.valid_count,
        error_count: stored.error_count,
    })
}