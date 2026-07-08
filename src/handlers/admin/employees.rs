use axum::{
    extract::{Path, Query, State},
    response::Redirect,
    Form,
};
use minijinja::context;
use serde::Deserialize;
use tower_sessions::Session;
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::error::{AppError, AppResult};
use crate::handlers::flash::redirect_with_flash_from_result_urls;
use crate::handlers::render::{render_page, HtmlPage};
use crate::handlers::profile::profile_context;
use crate::handlers::requirements::build_requirement_rows;
use crate::models::{LeaveRequestType, UserRole};
use crate::services::{
    audit::log_action,
    employees::{
        count_active_admins, create_employee, find_by_id, list_all, reset_employee_pin,
        set_employee_active, update_employee,
    },
    leave_balances::{format_leave_balance, get_snapshot, set_balance},
    onboarding::{
        count_active_without_department, count_admin_employee_rows, list_admin_employee_rows,
        list_distinct_departments, profile_completeness_pct, AdminEmployeeQuery,
        EmployeeListStatus,
    },
    profile::{get_profile, set_department, update_admin, AdminProfileInput},
    pagination::{clamp_page, clamp_per_page, offset, PageInfo},
    requirements::list_for_employee as list_requirements_for_employee,
    settings::get_settings,
    shifts::{list_for_employee as list_shifts_for_employee, upsert_shift},
    timezone::parse_date,
};
use crate::state::AppState;

use super::common::{pagination_context, parse_time, ListPageQuery};
use super::shifts::{build_shift_day_rows, shift_time_defaults};

pub async fn admin_home() -> Redirect {
    Redirect::to(crate::auth::ADMIN_HOME)
}

#[derive(Deserialize, Default)]
pub struct EmployeesListQuery {
    #[serde(flatten)]
    pub list: ListPageQuery,
    pub status: Option<String>,
}

pub async fn employees_page(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
    Query(list_query): Query<EmployeesListQuery>,
) -> AppResult<HtmlPage> {
    let settings = get_settings(&state.pool).await?;
    let page = clamp_page(list_query.list.page);
    let per_page = clamp_per_page(list_query.list.per_page);
    let status = EmployeeListStatus::from_query(list_query.status.as_deref());
    let total =
        count_admin_employee_rows(&state.pool, list_query.list.q.as_deref(), status).await?;
    let page_info = PageInfo::new(page, per_page, total);
    let employees = list_admin_employee_rows(
        &state.pool,
        &AdminEmployeeQuery {
            search: list_query.list.q.clone(),
            status,
            limit: per_page,
            offset: offset(page, per_page),
        },
    )
    .await?;
    let managers = list_all(&state.pool).await?;
    let departments = list_distinct_departments(&state.pool).await?;

    let employee_rows: Vec<_> = employees
        .iter()
        .map(|emp| {
            let profile_pct = profile_completeness_pct(emp);
            let no_department = emp
                .department
                .as_deref()
                .is_none_or(|d| d.trim().is_empty());
            context! {
                id => emp.id,
                employee_code => emp.employee_code.clone(),
                full_name => emp.full_name.clone(),
                role => emp.role,
                is_active => emp.is_active,
                department => emp.department.clone().unwrap_or_default(),
                no_department => no_department,
                requirements_met => emp.requirements_met,
                requirements_total => emp.requirements_total,
                requirements_label => if emp.requirements_total > 0 {
                    format!("{}/{}", emp.requirements_met, emp.requirements_total)
                } else {
                    "—".to_string()
                },
                profile_pct => profile_pct,
                profile_complete => profile_pct >= 100,
            }
        })
        .collect();

    let manager_options: Vec<_> = managers
        .iter()
        .filter(|e| e.role.is_manager_or_admin())
        .map(|e| context! { id => e.id, full_name => e.full_name.clone() })
        .collect();

    let no_department_count = count_active_without_department(&state.pool).await? as usize;

    render_page(
        &state,
        &session,
        Some(user),
        &settings.company_name,
        "Employees",
        "admin/employees.html",
        context! {
            employees => employee_rows,
            managers => manager_options,
            departments => departments,
            no_department_count => no_department_count,
            pagination => pagination_context("/admin/employees", &list_query.list, &page_info),
            status_filter => match status {
                EmployeeListStatus::Active => "active",
                EmployeeListStatus::Archived => "archived",
                EmployeeListStatus::All => "all",
            },
            show_archived => status == EmployeeListStatus::Archived,
            show_all => status == EmployeeListStatus::All,
        },
    )
    .await
}

#[derive(Deserialize)]
pub struct CreateEmployeeForm {
    employee_code: String,
    full_name: String,
    pin: String,
    role: String,
    department: String,
    manager_id: Option<Uuid>,
}

pub async fn create_employee_action(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
    Form(form): Form<CreateEmployeeForm>,
) -> AppResult<Redirect> {
    let role = match form.role.as_str() {
        "manager" => UserRole::Manager,
        "admin" => UserRole::Admin,
        _ => UserRole::Employee,
    };

    let result: AppResult<()> = async {
        let created = create_employee(
            &state.pool,
            &form.employee_code.trim().to_uppercase(),
            form.full_name.trim(),
            form.pin.trim(),
            role,
            form.manager_id,
        )
        .await?;

        set_department(&state.pool, created.id, &form.department).await?;

        log_action(
            &state.pool,
            user.employee_id,
            "employee.created",
            &format!(
                "Created employee {} ({}) in {}",
                created.full_name,
                created.employee_code,
                form.department.trim()
            ),
        )
        .await?;
        Ok(())
    }
    .await;

    redirect_with_flash_from_result_urls(
        &session,
        "/admin/employees",
        "/admin/employees?add=1",
        "Employee created",
        result,
    )
    .await
}

pub async fn edit_employee_page(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
    Path(employee_id): Path<Uuid>,
) -> AppResult<HtmlPage> {
    let settings = get_settings(&state.pool).await?;
    let employees = list_all(&state.pool).await?;
    let employee = find_by_id(&state.pool, employee_id)
        .await?
        .ok_or(AppError::NotFound)?;
    let profile = get_profile(&state.pool, employee_id).await?;
    let balances = get_snapshot(&state.pool, employee_id).await?;
    let shifts = list_shifts_for_employee(&state.pool, employee_id).await?;
    let (shift_default_start, shift_default_end) = shift_time_defaults(&shifts);
    let requirements = list_requirements_for_employee(&state.pool, employee_id).await?;

    render_page(
        &state,
        &session,
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
            profile => profile_context(
                &profile,
                &employee.employee_code,
                &employee.full_name,
                &format_leave_balance(balances.vacation_tenths),
                &format_leave_balance(balances.sick_tenths),
            ),
            day_rows => build_shift_day_rows(&shifts),
            shift_default_start => shift_default_start,
            shift_default_end => shift_default_end,
            requirements => build_requirement_rows(&requirements, &settings.timezone),
            message => None::<String>,
        },
    )
    .await
}

#[derive(Deserialize)]
pub struct UpdateEmployeeForm {
    employee_code: String,
    full_name: String,
    role: String,
    manager_id: Option<Uuid>,
    contact_number: Option<String>,
    personal_email: Option<String>,
    birthdate: Option<String>,
    address: Option<String>,
    emergency_contact_name: Option<String>,
    emergency_contact_phone: Option<String>,
    job_title: Option<String>,
    department: Option<String>,
    employment_type: Option<String>,
    date_hired: Option<String>,
    date_separated: Option<String>,
    work_location: Option<String>,
    bank_account: Option<String>,
    tin: Option<String>,
    sss_number: Option<String>,
    philhealth_number: Option<String>,
    vacation_balance: f64,
    sick_balance: f64,
    shift_start_0: String,
    shift_end_0: String,
    shift_start_1: String,
    shift_end_1: String,
    shift_start_2: String,
    shift_end_2: String,
    shift_start_3: String,
    shift_end_3: String,
    shift_start_4: String,
    shift_end_4: String,
    shift_start_5: String,
    shift_end_5: String,
    shift_start_6: String,
    shift_end_6: String,
}

fn shift_fields(form: &UpdateEmployeeForm) -> [(&str, &str); 7] {
    [
        (&form.shift_start_0, &form.shift_end_0),
        (&form.shift_start_1, &form.shift_end_1),
        (&form.shift_start_2, &form.shift_end_2),
        (&form.shift_start_3, &form.shift_end_3),
        (&form.shift_start_4, &form.shift_end_4),
        (&form.shift_start_5, &form.shift_end_5),
        (&form.shift_start_6, &form.shift_end_6),
    ]
}

pub async fn update_employee_action(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
    Path(employee_id): Path<Uuid>,
    Form(form): Form<UpdateEmployeeForm>,
) -> AppResult<Redirect> {
    let employee_url = format!("/admin/employees/{employee_id}");
    let result: AppResult<()> = async {
        let role = match form.role.as_str() {
            "manager" => UserRole::Manager,
            "admin" => UserRole::Admin,
            _ => UserRole::Employee,
        };

        if employee_id == user.employee_id && role != UserRole::Admin {
            return Err(AppError::bad_request(
                "You cannot remove your own admin role",
            ));
        }

        let employee = find_by_id(&state.pool, employee_id)
            .await?
            .ok_or(AppError::NotFound)?;

        let updated = update_employee(
        &state.pool,
        employee_id,
        &form.employee_code,
        form.full_name.trim(),
        role,
        form.manager_id,
    )
    .await?;

    let birthdate = form
        .birthdate
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(parse_date)
        .transpose()
        .map_err(AppError::bad_request)?;
    let date_hired = form
        .date_hired
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(parse_date)
        .transpose()
        .map_err(AppError::bad_request)?;
    let date_separated = form
        .date_separated
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(parse_date)
        .transpose()
        .map_err(AppError::bad_request)?;

    update_admin(
        &state.pool,
        employee_id,
        user.employee_id,
        AdminProfileInput {
            contact_number: form.contact_number.as_deref(),
            personal_email: form.personal_email.as_deref(),
            birthdate,
            address: form.address.as_deref(),
            emergency_contact_name: form.emergency_contact_name.as_deref(),
            emergency_contact_phone: form.emergency_contact_phone.as_deref(),
            job_title: form.job_title.as_deref(),
            department: form.department.as_deref(),
            employment_type: form.employment_type.as_deref(),
            date_hired,
            date_separated,
            work_location: form.work_location.as_deref(),
            bank_account: form.bank_account.as_deref(),
            tin: form.tin.as_deref(),
            sss_number: form.sss_number.as_deref(),
            philhealth_number: form.philhealth_number.as_deref(),
        },
    )
    .await?;

    set_balance(
        &state.pool,
        employee_id,
        LeaveRequestType::Vacation,
        form.vacation_balance,
    )
    .await?;
    set_balance(
        &state.pool,
        employee_id,
        LeaveRequestType::SickLeave,
        form.sick_balance,
    )
    .await?;

    for (day, (start, end)) in shift_fields(&form).into_iter().enumerate() {
        let start_time = parse_time(start)?;
        let end_time = parse_time(end)?;
        upsert_shift(
            &state.pool,
            employee_id,
            day as i16,
            start_time,
            end_time,
        )
        .await?;
    }

    log_action(
        &state.pool,
        user.employee_id,
        "employee.updated",
        &format!(
            "Updated employee {} ({})",
            updated.full_name, updated.employee_code
        ),
    )
    .await?;
        log_action(
            &state.pool,
            user.employee_id,
            "profile.updated",
            &format!(
                "Updated profile for {} ({})",
                employee.full_name, employee.employee_code
            ),
        )
        .await?;
        Ok(())
    }
    .await;

    redirect_with_flash_from_result_urls(
        &session,
        &employee_url,
        &employee_url,
        "Employee saved",
        result,
    )
    .await
}

#[derive(Deserialize)]
pub struct ResetPinForm {
    new_pin: String,
}

pub async fn reset_pin_action(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
    Path(employee_id): Path<Uuid>,
    Form(form): Form<ResetPinForm>,
) -> AppResult<Redirect> {
    let employee_url = format!("/admin/employees/{employee_id}");
    let result: AppResult<()> = async {
        let employee = find_by_id(&state.pool, employee_id)
            .await?
            .ok_or(AppError::NotFound)?;

        reset_employee_pin(&state.pool, employee_id, form.new_pin.trim()).await?;

        log_action(
            &state.pool,
            user.employee_id,
            "employee.pin_reset",
            &format!(
                "Reset PIN for {} ({})",
                employee.full_name, employee.employee_code
            ),
        )
        .await?;
        Ok(())
    }
    .await;

    redirect_with_flash_from_result_urls(
        &session,
        &employee_url,
        &employee_url,
        "PIN reset — employee must change on next login",
        result,
    )
    .await
}

pub async fn toggle_active_action(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
    Path(employee_id): Path<Uuid>,
) -> AppResult<Redirect> {
    let employee_url = format!("/admin/employees/{employee_id}");
    let flash_message = async {
        if employee_id == user.employee_id {
            return Err(AppError::bad_request(
                "You cannot deactivate your own account",
            ));
        }

        let employee = find_by_id(&state.pool, employee_id)
            .await?
            .ok_or(AppError::NotFound)?;

        if employee.is_active && employee.role == UserRole::Admin {
            let admins = count_active_admins(&state.pool).await?;
            if admins <= 1 {
                return Err(AppError::bad_request(
                    "Cannot deactivate the last active admin",
                ));
            }
        }

        set_employee_active(&state.pool, employee_id, !employee.is_active).await?;

        let (action, message) = if employee.is_active {
            (
                "employee.deactivated",
                format!(
                    "Deactivated {} ({})",
                    employee.full_name, employee.employee_code
                ),
            )
        } else {
            (
                "employee.reactivated",
                format!(
                    "Reactivated {} ({})",
                    employee.full_name, employee.employee_code
                ),
            )
        };
        log_action(&state.pool, user.employee_id, action, &message).await?;

        Ok(if employee.is_active {
            "Employee deactivated"
        } else {
            "Employee reactivated"
        })
    }
    .await;

    match flash_message {
        Ok(message) => {
            redirect_with_flash_from_result_urls(
                &session,
                &employee_url,
                &employee_url,
                message,
                Ok(()),
            )
            .await
        }
        Err(err) => {
            redirect_with_flash_from_result_urls(&session, &employee_url, &employee_url, "", Err(err))
                .await
        }
    }
}
