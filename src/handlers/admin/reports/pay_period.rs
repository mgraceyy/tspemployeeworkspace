use axum::{extract::State, response::Redirect, Form};
use serde::Deserialize;
use tower_sessions::Session;

use crate::auth::AuthUser;
use crate::error::{AppError, AppResult};
use crate::handlers::flash::redirect_with_flash;
use crate::services::{
    audit::log_action,
    payroll_controls::{close_pay_period, reopen_pay_period, ClosePayPeriodResult},
    timezone::{format_date, parse_date},
};
use crate::state::AppState;

#[derive(Deserialize)]
pub struct PeriodControlForm {
    start: String,
    end: String,
    note: Option<String>,
}

pub async fn close_pay_period_action(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
    Form(form): Form<PeriodControlForm>,
) -> AppResult<Redirect> {
    let start = parse_date(&form.start).map_err(AppError::bad_request)?;
    let end = parse_date(&form.end).map_err(AppError::bad_request)?;
    let result = close_pay_period(
        &state.pool,
        start,
        end,
        user.employee_id,
        form.note.as_deref(),
    )
    .await?;

    let redirect_url = format!(
        "/admin/reports?start={}&end={}",
        format_date(start),
        format_date(end)
    );

    match result {
        ClosePayPeriodResult::Closed => {
            log_action(
                &state.pool,
                user.employee_id,
                "reports.period_closed",
                &format!(
                    "Closed pay period {} to {}",
                    format_date(start),
                    format_date(end)
                ),
            )
            .await?;

            redirect_with_flash(
                &session,
                &redirect_url,
                "success",
                "Pay period closed — time edits in this range are now blocked",
            )
            .await
        }
        ClosePayPeriodResult::AlreadyClosed => {
            redirect_with_flash(
                &session,
                &redirect_url,
                "info",
                "This pay period is already closed",
            )
            .await
        }
    }
}

pub async fn reopen_pay_period_action(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
    Form(form): Form<PeriodControlForm>,
) -> AppResult<Redirect> {
    let start = parse_date(&form.start).map_err(AppError::bad_request)?;
    let end = parse_date(&form.end).map_err(AppError::bad_request)?;
    reopen_pay_period(&state.pool, start, end).await?;

    log_action(
        &state.pool,
        user.employee_id,
        "reports.period_reopened",
        &format!(
            "Reopened pay period {} to {}",
            format_date(start),
            format_date(end)
        ),
    )
    .await?;

    redirect_with_flash(
        &session,
        &format!(
            "/admin/reports?start={}&end={}",
            format_date(start),
            format_date(end)
        ),
        "success",
        "Pay period reopened — time edits are allowed again",
    )
    .await
}
