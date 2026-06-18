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
use crate::handlers::flash::redirect_with_flash;
use crate::handlers::render::{render_page, HtmlPage};
use crate::models::UserRole;
use crate::services::{
    audit::log_action,
    employees::{
        count_active_admins, create_employee, find_by_id, list_all, reset_employee_pin,
        set_employee_active, update_employee,
    },
    onboarding::{
        bulk_assign_department, count_active_without_department, count_admin_employee_rows,
        list_admin_employee_rows, list_distinct_departments, profile_completeness_pct,
        AdminEmployeeQuery, EmployeeListStatus,
    },
    pagination::{clamp_page, clamp_per_page, offset, PageInfo},
    settings::get_settings,
};
use crate::state::AppState;

use super::common::{pagination_context, ListPageQuery};

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
pub struct BulkDepartmentForm {
    department: String,
    employee_ids: Vec<Uuid>,
}

pub async fn bulk_assign_department_action(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
    Form(form): Form<BulkDepartmentForm>,
) -> AppResult<Redirect> {
    let count = bulk_assign_department(
        &state.pool,
        &form.employee_ids,
        &form.department,
        user.employee_id,
    )
    .await?;

    log_action(
        &state.pool,
        user.employee_id,
        "profile.bulk_department",
        &format!(
            "Assigned department \"{}\" to {} employee(s)",
            form.department.trim(),
            count
        ),
    )
    .await?;

    redirect_with_flash(
        &session,
        "/admin/employees",
        "success",
        &format!("Department updated for {count} employee(s)"),
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
    AuthUser(user): AuthUser,
    Form(form): Form<CreateEmployeeForm>,
) -> AppResult<Redirect> {
    let role = match form.role.as_str() {
        "manager" => UserRole::Manager,
        "admin" => UserRole::Admin,
        _ => UserRole::Employee,
    };

    let created = create_employee(
        &state.pool,
        &form.employee_code.trim().to_uppercase(),
        form.full_name.trim(),
        form.pin.trim(),
        role,
        form.manager_id,
    )
    .await?;

    log_action(
        &state.pool,
        user.employee_id,
        "employee.created",
        &format!(
            "Created employee {} ({})",
            created.full_name, created.employee_code
        ),
    )
    .await?;

    redirect_with_flash(&session, "/admin/employees", "success", "Employee created").await
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
}

pub async fn update_employee_action(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
    Path(employee_id): Path<Uuid>,
    Form(form): Form<UpdateEmployeeForm>,
) -> AppResult<Redirect> {
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

    let updated = update_employee(
        &state.pool,
        employee_id,
        &form.employee_code,
        form.full_name.trim(),
        role,
        form.manager_id,
    )
    .await?;

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

    redirect_with_flash(
        &session,
        &format!("/admin/employees/{employee_id}"),
        "success",
        "Employee updated",
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

    redirect_with_flash(
        &session,
        &format!("/admin/employees/{employee_id}"),
        "success",
        "PIN reset — employee must change on next login",
    )
    .await
}

pub async fn toggle_active_action(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
    Path(employee_id): Path<Uuid>,
) -> AppResult<Redirect> {
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

    let flash_message = if employee.is_active {
        "Employee deactivated"
    } else {
        "Employee reactivated"
    };
    redirect_with_flash(
        &session,
        &format!("/admin/employees/{employee_id}"),
        "success",
        flash_message,
    )
    .await
}
