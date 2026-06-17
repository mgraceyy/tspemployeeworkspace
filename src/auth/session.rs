use serde::{Deserialize, Serialize};
use tower_sessions::Session;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::models::UserRole;

pub const SESSION_KEY: &str = "user";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserSession {
    pub employee_id: Uuid,
    pub employee_code: String,
    pub full_name: String,
    pub role: UserRole,
    pub must_change_pin: bool,
}

pub async fn get_session(session: &Session) -> AppResult<UserSession> {
    session
        .get(SESSION_KEY)
        .await
        .map_err(|e| AppError::Internal(e.into()))?
        .ok_or(AppError::Unauthorized)
}

pub async fn get_active_session(session: &Session) -> AppResult<UserSession> {
    let user = get_session(session).await?;
    if user.must_change_pin {
        return Err(AppError::PinChangeRequired);
    }
    Ok(user)
}

pub async fn set_session(session: &Session, user: UserSession) -> AppResult<()> {
    session
        .insert(SESSION_KEY, user)
        .await
        .map_err(|e| AppError::Internal(e.into()))
}

pub async fn clear_session(session: &Session) -> AppResult<()> {
    session
        .flush()
        .await
        .map_err(|e| AppError::Internal(e.into()))
}

pub fn require_manager(user: &UserSession) -> AppResult<()> {
    if user.role.is_manager_or_admin() {
        Ok(())
    } else {
        Err(AppError::Forbidden)
    }
}

pub fn require_admin(user: &UserSession) -> AppResult<()> {
    if user.role.is_admin() {
        Ok(())
    } else {
        Err(AppError::Forbidden)
    }
}