use axum::{
    extract::{Path, State},
    response::Redirect,
    Form,
};
use serde::Deserialize;
use tower_sessions::Session;
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::error::AppResult;
use crate::handlers::flash::redirect_with_flash_from_result;
use crate::services::{
    audit::log_action,
    ot::{entry_audit_label, review_overtime},
};
use crate::state::AppState;

#[derive(Deserialize)]
pub struct OtReviewForm {
    action: String,
    note: Option<String>,
}

pub async fn review_ot(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
    Path(entry_id): Path<Uuid>,
    Form(form): Form<OtReviewForm>,
) -> AppResult<Redirect> {
    let approve = form.action == "approve";
    let flash_message = if approve {
        "Overtime approved"
    } else {
        "Overtime rejected"
    };

    let result: AppResult<()> = async {
        review_overtime(
            &state.pool,
            entry_id,
            user.employee_id,
            approve,
            form.note.filter(|n| !n.trim().is_empty()),
            user.role.is_admin(),
        )
        .await?;

        let label = entry_audit_label(&state.pool, entry_id).await?;
        let (action, summary) = if approve {
            ("ot.approved", format!("Approved overtime for {label}"))
        } else {
            ("ot.rejected", format!("Rejected overtime for {label}"))
        };
        log_action(&state.pool, user.employee_id, action, &summary).await?;
        Ok(())
    }
    .await;

    redirect_with_flash_from_result(&session, "/manager", flash_message, result).await
}
