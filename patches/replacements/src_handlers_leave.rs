use axum::{extract::State, response::Redirect, Form};
use minijinja::context;
use serde::Deserialize;
use tower_sessions::Session;

use crate::auth::AuthUser;
use crate::error::{AppError, AppResult};
use crate::handlers::flash::redirect_with_flash;
use crate::handlers::render::{render_page, HtmlPage};
use crate::models::{LeaveRequestStatus, LeaveRequestType};
use crate::services::{
    audit::log_action,
    leave::{
        cancel_request, create_request, list_approved_for_manager, list_for_employee,
        list_pending_for_manager, revoke_approved_request_by_id, review_request,
    },
    leave_balances::get_snapshot,
    payroll_controls::assert_date_range_editable,
    settings::get_settings,
    timezone::{format_date, parse_date},
};
use crate::state::AppState;

fn leave_status_label(status: LeaveRequestStatus) -> &'static str {
    match status {
        LeaveRequestStatus::Pending => "Pending",
        LeaveRequestStatus::Approved => "Approved",
        LeaveRequestStatus::Rejected => "Rejected",
        LeaveRequestStatus::Cancelled => "Cancelled",
    }
}

pub async fn my_leave_page(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
) -> AppResult<HtmlPage> {
    let settings = get_settings(&state.pool).await?;
    let requests = list_for_employee(&state.pool, user.employee_id).await?;
    let balances = get_snapshot(&state.pool, user.employee_id).await?;
    let rows: Vec<_> = requests
        .iter()
        .map(|req| {
            context! {
                id => req.id,
                start_date => format_date(req.start_date),
                end_date => format_date(req.end_date),
                leave_type => req.leave_type.label(),
                reason => req.reason.clone().unwrap_or_default(),
                status => leave_status_label(req.status),
                reviewer_note => req.reviewer_note.clone().unwrap_or_default(),
                can_cancel => req.status == LeaveRequestStatus::Pending
                    || req.status == LeaveRequestStatus::Approved,
            }
        })
        .collect();

    render_page(
        &state,
        &session,
        Some(user),
        &settings.company_name,
        "Leave Requests",
        "employee/leave.html",
        context! {
            requests => rows,
            vacation_balance => balances.vacation_days,
            sick_balance => balances.sick_days,
        },
    )
    .await
}

#[derive(Deserialize)]
pub struct LeaveRequestForm {
    start_date: String,
    end_date: String,
    leave_type: String,
    reason: Option<String>,
}

fn parse_leave_type(value: &str) -> AppResult<LeaveRequestType> {
    match value {
        "sick_leave" => Ok(LeaveRequestType::SickLeave),
        "vacation" => Ok(LeaveRequestType::Vacation),
        "official_leave" => Ok(LeaveRequestType::OfficialLeave),
        "offset" => Ok(LeaveRequestType::Offset),
        _ => Err(AppError::bad_request("Invalid leave type")),
    }
}

pub async fn submit_leave_request(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
    Form(form): Form<LeaveRequestForm>,
) -> AppResult<Redirect> {
    let start_date = parse_date(&form.start_date).map_err(AppError::bad_request)?;
    let end_date = parse_date(&form.end_date).map_err(AppError::bad_request)?;
    let leave_type = parse_leave_type(&form.leave_type)?;

    create_request(
        &state.pool,
        user.employee_id,
        start_date,
        end_date,
        leave_type,
        form.reason.as_deref(),
    )
    .await?;

    redirect_with_flash(
        &session,
        "/me/leave",
        "success",
        "Leave request submitted for manager approval",
    )
    .await
}

pub async fn cancel_leave_request(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
    axum::extract::Path(request_id): axum::extract::Path<uuid::Uuid>,
) -> AppResult<Redirect> {
    cancel_request(&state.pool, user.employee_id, request_id).await?;
    redirect_with_flash(&session, "/me/leave", "success", "Leave request cancelled").await
}

pub async fn manager_leave_page(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
) -> AppResult<HtmlPage> {
    let settings = get_settings(&state.pool).await?;
    let is_admin = user.role.is_admin();
    let pending = list_pending_for_manager(&state.pool, user.employee_id, is_admin).await?;
    let mut rows = Vec::with_capacity(pending.len());
    for req in &pending {
        let balances = get_snapshot(&state.pool, req.employee_id).await?;
        rows.push(context! {
            id => req.id,
            employee_id => req.employee_id,
            employee_code => req.employee_code.clone(),
            full_name => req.full_name.clone(),
            start_date => format_date(req.start_date),
            end_date => format_date(req.end_date),
            leave_type => req.leave_type.label(),
            reason => req.reason.clone().unwrap_or_default(),
            vacation_balance => balances.vacation_days,
            sick_balance => balances.sick_days,
        });
    }

    let approved = list_approved_for_manager(&state.pool, user.employee_id, is_admin).await?;
    let mut approved_rows = Vec::new();
    for req in &approved {
        let can_revoke = assert_date_range_editable(
            &state.pool,
            req.start_date,
            req.end_date,
        )
        .await
        .is_ok();
        approved_rows.push(context! {
            id => req.id,
            employee_code => req.employee_code.clone(),
            full_name => req.full_name.clone(),
            start_date => format_date(req.start_date),
            end_date => format_date(req.end_date),
            leave_type => req.leave_type.label(),
            reason => req.reason.clone().unwrap_or_default(),
            can_revoke => can_revoke,
        });
    }

    render_page(
        &state,
        &session,
        Some(user),
        &settings.company_name,
        "Leave Requests",
        "manager/leave.html",
        context! {
            pending => rows,
            approved => approved_rows,
        },
    )
    .await
}

#[derive(Deserialize)]
pub struct ReviewLeaveForm {
    action: String,
    note: Option<String>,
}

pub async fn review_leave_request(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
    axum::extract::Path(request_id): axum::extract::Path<uuid::Uuid>,
    Form(form): Form<ReviewLeaveForm>,
) -> AppResult<Redirect> {
    let approve = form.action == "approve";
    let request = review_request(
        &state.pool,
        request_id,
        user.employee_id,
        user.role.is_admin(),
        approve,
        form.note.as_deref(),
    )
    .await?;

    log_action(
        &state.pool,
        user.employee_id,
        if approve {
            "leave.approved"
        } else {
            "leave.rejected"
        },
        &format!(
            "{} leave for {} ({}) — {} to {}",
            if approve { "Approved" } else { "Rejected" },
            request.full_name,
            request.employee_code,
            format_date(request.start_date),
            format_date(request.end_date)
        ),
    )
    .await?;

    redirect_with_flash(
        &session,
        "/manager/leave",
        "success",
        if approve {
            "Leave request approved"
        } else {
            "Leave request rejected"
        },
    )
    .await
}

pub async fn revoke_leave_request(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
    axum::extract::Path(request_id): axum::extract::Path<uuid::Uuid>,
) -> AppResult<Redirect> {
    let request = revoke_approved_request_by_id(
        &state.pool,
        request_id,
        user.employee_id,
        user.role.is_admin(),
    )
    .await?;

    log_action(
        &state.pool,
        user.employee_id,
        "leave.revoked",
        &format!(
            "Revoked approved leave for {} ({}) — {} to {}",
            request.full_name,
            request.employee_code,
            format_date(request.start_date),
            format_date(request.end_date)
        ),
    )
    .await?;

    redirect_with_flash(
        &session,
        "/manager/leave",
        "success",
        "Approved leave revoked and balance restored",
    )
    .await
}