use axum::{
    extract::{Path, State},
    response::{IntoResponse, Redirect, Response},
    Form,
};
use minijinja::context;
use serde::Deserialize;
use time::{Date, Time};
use tower_sessions::Session;
use uuid::Uuid;

use crate::auth::{get_active_session, require_admin};
use crate::error::{AppError, AppResult};
use crate::handlers::render::{render_page, HtmlPage};
use crate::models::{PayPeriodType, UserRole};
use crate::services::{
    employees::{
        count_active_admins, create_employee, find_by_id, list_all, reset_employee_pin,
        set_employee_active, update_employee,
    },
    reports::{
        build_payroll_xlsx, current_pay_period, minutes_to_hours_decimal, pay_period_label,
        payroll_summary,
    },
    settings::{get_settings, update_settings},
    shifts::{list_for_employee, upsert_shift},
    timezone::manila_date_now,
};
use crate::state::AppState;

pub async fn employees_page(State(state): State<AppState>, session: Session) -> AppResult<HtmlPage> {
    let user = get_active_session(&session).await?;
    require_admin(&user)?;
    let settings = get_settings(&state.pool).await?;
    let employees = list_all(&state.pool).await?;

    render_page(
        &state,
        Some(user),
        &settings.company_name,
        "Employees",
        "admin/employees.html",
        context! {
            employees => employees,
            message => None::<String>,
        }
        .into(),
    )
    .await
}

#[derive(Deserialize)]
pub struct CreateEmployeeForm {
    employee_code: String,
    full_name: String,
    pin: String,
    role: String,
    manager_id: Option<Uuid>,
}

pub async fn create_employee_action(
    State(state): State<AppState>,
    session: Session,
    Form(form): Form<CreateEmployeeForm>,
) -> AppResult<Redirect> {
    let user = get_active_session(&session).await?;
    require_admin(&user)?;

    let role = match form.role.as_str() {
        "manager" => UserRole::Manager,
        "admin" => UserRole::Admin,
        _ => UserRole::Employee,
    };

    create_employee(
        &state.pool,
        &form.employee_code.trim().to_uppercase(),
        form.full_name.trim(),
        form.pin.trim(),
        role,
        form.manager_id,
    )
    .await?;

    Ok(Redirect::to("/admin/employees"))
}

pub async fn edit_employee_page(
    State(state): State<AppState>,
    session: Session,
    Path(employee_id): Path<Uuid>,
) -> AppResult<HtmlPage> {
    let user = get_active_session(&session).await?;
    require_admin(&user)?;
    let settings = get_settings(&state.pool).await?;
    let employees = list_all(&state.pool).await?;
    let employee = find_by_id(&state.pool, employee_id)
        .await?
        .ok_or(AppError::NotFound)?;

    render_page(
        &state,
        Some(user.clone()),
        &settings.company_name,
        "Edit Employee",
        "admin/employee_edit.html",
        context! {
            employee => context! {
                id => employee.id,
                employee_code => employee.employee_code,
                full_name => employee.full_name,
                role => employee.role,
                manager_id => employee.manager_id,
                is_active => employee.is_active,
            },
            employees => employees,
            current_user_id => user.employee_id,
            message => None::<String>,
        }
        .into(),
    )
    .await
}

#[derive(Deserialize)]
pub struct UpdateEmployeeForm {
    full_name: String,
    role: String,
    manager_id: Option<Uuid>,
}

pub async fn update_employee_action(
    State(state): State<AppState>,
    session: Session,
    Path(employee_id): Path<Uuid>,
    Form(form): Form<UpdateEmployeeForm>,
) -> AppResult<Redirect> {
    let user = get_active_session(&session).await?;
    require_admin(&user)?;

    let role = match form.role.as_str() {
        "manager" => UserRole::Manager,
        "admin" => UserRole::Admin,
        _ => UserRole::Employee,
    };

    if employee_id == user.employee_id && role != UserRole::Admin {
        return Err(AppError::bad_request("You cannot remove your own admin role"));
    }

    update_employee(
        &state.pool,
        employee_id,
        form.full_name.trim(),
        role,
        form.manager_id,
    )
    .await?;

    Ok(Redirect::to(&format!("/admin/employees/{employee_id}")))
}

#[derive(Deserialize)]
pub struct ResetPinForm {
    new_pin: String,
}

pub async fn reset_pin_action(
    State(state): State<AppState>,
    session: Session,
    Path(employee_id): Path<Uuid>,
    Form(form): Form<ResetPinForm>,
) -> AppResult<Redirect> {
    let user = get_active_session(&session).await?;
    require_admin(&user)?;

    reset_employee_pin(&state.pool, employee_id, form.new_pin.trim()).await?;

    Ok(Redirect::to(&format!("/admin/employees/{employee_id}")))
}

pub async fn toggle_active_action(
    State(state): State<AppState>,
    session: Session,
    Path(employee_id): Path<Uuid>,
) -> AppResult<Redirect> {
    let user = get_active_session(&session).await?;
    require_admin(&user)?;

    if employee_id == user.employee_id {
        return Err(AppError::bad_request("You cannot deactivate your own account"));
    }

    let employee = find_by_id(&state.pool, employee_id)
        .await?
        .ok_or(AppError::NotFound)?;

    if employee.is_active && employee.role == UserRole::Admin {
        let admins = count_active_admins(&state.pool).await?;
        if admins <= 1 {
            return Err(AppError::bad_request("Cannot deactivate the last active admin"));
        }
    }

    set_employee_active(&state.pool, employee_id, !employee.is_active).await?;

    Ok(Redirect::to(&format!("/admin/employees/{employee_id}")))
}

pub async fn shifts_page(
    State(state): State<AppState>,
    session: Session,
    Path(employee_id): Path<Uuid>,
) -> AppResult<HtmlPage> {
    let user = get_active_session(&session).await?;
    require_admin(&user)?;
    let settings = get_settings(&state.pool).await?;
    let employees = list_all(&state.pool).await?;
    let shifts = list_for_employee(&state.pool, employee_id).await?;
    let selected = employees.iter().find(|e| e.id == employee_id);
    let day_rows: Vec<_> = [
        (0, "Sunday"),
        (1, "Monday"),
        (2, "Tuesday"),
        (3, "Wednesday"),
        (4, "Thursday"),
        (5, "Friday"),
        (6, "Saturday"),
    ]
    .into_iter()
    .map(|(day, name)| {
        let existing = shifts.iter().find(|s| s.day_of_week == day);
        context! {
            day => day,
            name => name,
            start_time => existing.map(|s| format!("{:02}:{:02}", s.start_time.hour(), s.start_time.minute())).unwrap_or_else(|| "08:00".into()),
            end_time => existing.map(|s| format!("{:02}:{:02}", s.end_time.hour(), s.end_time.minute())).unwrap_or_else(|| "17:00".into()),
        }
    })
    .collect();

    render_page(
        &state,
        Some(user),
        &settings.company_name,
        "Shift Schedules",
        "admin/shifts.html",
        context! {
            employees => employees,
            selected => selected,
            day_rows => day_rows,
            message => None::<String>,
        }
        .into(),
    )
    .await
}

#[derive(Deserialize)]
pub struct ShiftForm {
    employee_id: Uuid,
    day_of_week: i16,
    start_time: String,
    end_time: String,
}

pub async fn save_shift(
    State(state): State<AppState>,
    session: Session,
    Form(form): Form<ShiftForm>,
) -> AppResult<Redirect> {
    let user = get_active_session(&session).await?;
    require_admin(&user)?;

    let start = parse_time(&form.start_time)?;
    let end = parse_time(&form.end_time)?;

    upsert_shift(
        &state.pool,
        form.employee_id,
        form.day_of_week,
        start,
        end,
    )
    .await?;

    Ok(Redirect::to(&format!("/admin/shifts/{}", form.employee_id)))
}

pub async fn settings_page(State(state): State<AppState>, session: Session) -> AppResult<HtmlPage> {
    let user = get_active_session(&session).await?;
    require_admin(&user)?;
    let settings = get_settings(&state.pool).await?;

    render_page(
        &state,
        Some(user.clone()),
        &settings.company_name,
        "Company Settings",
        "admin/settings.html",
        context! {
            settings => settings,
            message => None::<String>,
        }
        .into(),
    )
    .await
}

#[derive(Deserialize)]
pub struct SettingsForm {
    break_minutes: i32,
    ot_threshold_minutes: i32,
    grace_minutes: i32,
    pay_period: String,
}

pub async fn save_settings(
    State(state): State<AppState>,
    session: Session,
    Form(form): Form<SettingsForm>,
) -> AppResult<Redirect> {
    let user = get_active_session(&session).await?;
    require_admin(&user)?;

    let pay_period = match form.pay_period.as_str() {
        "weekly" => PayPeriodType::Weekly,
        "biweekly" => PayPeriodType::Biweekly,
        "monthly" => PayPeriodType::Monthly,
        _ => PayPeriodType::Semimonthly,
    };

    update_settings(
        &state.pool,
        form.break_minutes,
        form.ot_threshold_minutes,
        form.grace_minutes,
        pay_period,
    )
    .await?;

    Ok(Redirect::to("/admin/settings"))
}

pub async fn reports_page(State(state): State<AppState>, session: Session) -> AppResult<HtmlPage> {
    let user = get_active_session(&session).await?;
    require_admin(&user)?;
    let settings = get_settings(&state.pool).await?;
    let today = manila_date_now();
    let (start, end, label) = current_pay_period(today, settings.pay_period);
    let rows = payroll_summary(&state.pool, start, end).await?;

    let report_rows: Vec<_> = rows
        .iter()
        .map(|row| {
            context! {
                employee_code => row.employee_code.clone(),
                full_name => row.full_name.clone(),
                regular_hours => minutes_to_hours_decimal(row.regular_minutes),
                approved_ot_hours => minutes_to_hours_decimal(row.approved_ot_minutes),
                pending_ot_hours => minutes_to_hours_decimal(row.pending_ot_minutes),
                payable_hours => minutes_to_hours_decimal(row.regular_minutes + row.approved_ot_minutes),
            }
        })
        .collect();

    render_page(
        &state,
        Some(user),
        &settings.company_name,
        "Payroll Reports",
        "admin/reports.html",
        context! {
            period_label => label,
            pay_period_type => pay_period_label(settings.pay_period),
            rows => report_rows,
        }
        .into(),
    )
    .await
}

pub async fn export_csv(State(state): State<AppState>, session: Session) -> AppResult<Response> {
    let user = get_active_session(&session).await?;
    require_admin(&user)?;
    let settings = get_settings(&state.pool).await?;
    let today = manila_date_now();
    let (start, end, label) = current_pay_period(today, settings.pay_period);
    let rows = payroll_summary(&state.pool, start, end).await?;

    let mut csv_bytes = Vec::new();
    {
        let mut writer = csv::Writer::from_writer(&mut csv_bytes);
        writer
            .write_record([
                "Employee Code",
                "Name",
                "Regular Hours",
                "Approved OT Hours",
                "Pending OT Hours",
                "Payable Hours",
            ])
            .map_err(|e| AppError::Internal(e.into()))?;

        for row in &rows {
            writer
                .write_record([
                    row.employee_code.clone(),
                    row.full_name.clone(),
                    format!("{:.2}", minutes_to_hours_decimal(row.regular_minutes)),
                    format!("{:.2}", minutes_to_hours_decimal(row.approved_ot_minutes)),
                    format!("{:.2}", minutes_to_hours_decimal(row.pending_ot_minutes)),
                    format!(
                        "{:.2}",
                        minutes_to_hours_decimal(row.regular_minutes + row.approved_ot_minutes)
                    ),
                ])
                .map_err(|e| AppError::Internal(e.into()))?;
        }
        writer.flush().map_err(|e| AppError::Internal(e.into()))?;
    }

    let filename = format!("{}-payroll-{}.csv", settings.company_name.replace(' ', "-"), label);

    let disposition = format!("attachment; filename=\"{filename}\"");
    Ok((
        [
            (axum::http::header::CONTENT_TYPE, "text/csv".to_string()),
            (axum::http::header::CONTENT_DISPOSITION, disposition),
        ],
        csv_bytes,
    )
        .into_response())
}

pub async fn export_xlsx(State(state): State<AppState>, session: Session) -> AppResult<Response> {
    let user = get_active_session(&session).await?;
    require_admin(&user)?;
    let settings = get_settings(&state.pool).await?;
    let today = manila_date_now();
    let (start, end, label) = current_pay_period(today, settings.pay_period);
    let rows = payroll_summary(&state.pool, start, end).await?;

    let xlsx_bytes = build_payroll_xlsx(&settings.company_name, &label, &rows)?;
    let filename = format!(
        "{}-payroll-{}.xlsx",
        settings.company_name.replace(' ', "-"),
        label
    );
    let disposition = format!("attachment; filename=\"{filename}\"");

    Ok((
        [
            (
                axum::http::header::CONTENT_TYPE,
                "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet".to_string(),
            ),
            (axum::http::header::CONTENT_DISPOSITION, disposition),
        ],
        xlsx_bytes,
    )
        .into_response())
}

fn parse_time(value: &str) -> AppResult<Time> {
    let trimmed = value.trim();
    let parts: Vec<_> = trimmed.split(':').collect();
    if parts.len() != 2 {
        return Err(AppError::bad_request("Time must be HH:MM"));
    }
    let hour: u8 = parts[0]
        .parse()
        .map_err(|_| AppError::bad_request("Invalid hour"))?;
    let minute: u8 = parts[1]
        .parse()
        .map_err(|_| AppError::bad_request("Invalid minute"))?;
    Time::from_hms(hour, minute, 0).map_err(|_| AppError::bad_request("Invalid time"))
}