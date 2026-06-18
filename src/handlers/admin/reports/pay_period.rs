use axum::{extract::State, response::Redirect, Form};
use serde::Deserialize;
use tower_sessions::Session;

use crate::auth::AuthUser;
use crate::error::{AppError, AppResult};
use crate::handlers::flash::redirect_with_flash;
use crate::services::{
    audit::log_action,
    payroll_controls::{close_pay_period, reopen_pay_period, ClosePayPeriodResult},
    reports::assert_canonical_pay_period,
    settings::get_settings,
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
    let settings = get_settings(&state.pool).await?;
    let canonical = assert_canonical_pay_period(&settings, start, end).is_ok();
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

            let message = if canonical {
                "Pay period closed — time edits in this range are now blocked. You can run payroll from Payroll Runs."
            } else {
                "Pay period closed — time edits in this range are now blocked. Note: this range is not a full pay period, so payroll cannot run until you close the exact canonical range."
            };
            redirect_with_flash(&session, &redirect_url, "success", message).await
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
