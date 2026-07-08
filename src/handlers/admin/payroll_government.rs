//! Government deduction handler — wire into `handlers/admin/mod.rs`:
//! `mod payroll_government;` and `pub use payroll_government::recalculate_government_deductions_action;`

use axum::{
    extract::{Path, State},
    response::Redirect,
};
use tower_sessions::Session;
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::error::AppResult;
use crate::handlers::flash::redirect_with_flash_from_result;
use crate::services::{
    audit::log_action, payroll::recalculate_government_deductions_for_run, settings::get_settings,
    timezone::format_date,
};
use crate::state::AppState;

pub async fn recalculate_government_deductions_action(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
    Path(run_id): Path<Uuid>,
) -> AppResult<Redirect> {
    let run_url = format!("/admin/payroll/{run_id}");
    let result: AppResult<()> = async {
        let settings = get_settings(&state.pool).await?;
        let run = crate::services::payroll::get_run(&state.pool, run_id).await?;
        recalculate_government_deductions_for_run(&state.pool, run_id, &settings).await?;

        log_action(
            &state.pool,
            user.employee_id,
            "payroll.gov_deductions_recalculated",
            &format!(
                "Recalculated government auto-deductions for payroll {} to {}",
                format_date(run.period_start),
                format_date(run.period_end)
            ),
        )
        .await?;
        Ok(())
    }
    .await;

    redirect_with_flash_from_result(
        &session,
        &run_url,
        "Government and LWOP auto-deductions recalculated from current settings and compensation",
        result,
    )
    .await
}
