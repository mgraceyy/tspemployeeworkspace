use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tower_sessions::Session;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::models::UserRole;
use crate::services::employees::find_by_id;

pub const SESSION_KEY: &str = "user";
pub const FLASH_KEY: &str = "flash";
pub const SESSION_IDLE_HOURS: i32 = 12;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlashMessage {
    pub kind: String,
    pub message: String,
}

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

/// Reloads the employee row and refreshes the signed session when role or PIN flags change.
pub async fn sync_session_with_db(pool: &PgPool, session: &Session) -> AppResult<UserSession> {
    let cached = get_session(session).await?;
    let Some(employee) = find_by_id(pool, cached.employee_id).await? else {
        clear_session(session).await?;
        return Err(AppError::Unauthorized);
    };
    if !employee.is_active {
        clear_session(session).await?;
        return Err(AppError::Unauthorized);
    }

    let fresh = UserSession {
        employee_id: employee.id,
        employee_code: employee.employee_code,
        full_name: employee.full_name,
        role: employee.role,
        must_change_pin: employee.must_change_pin,
    };

    if fresh.employee_code != cached.employee_code
        || fresh.full_name != cached.full_name
        || fresh.role != cached.role
        || fresh.must_change_pin != cached.must_change_pin
    {
        set_session(session, fresh.clone()).await?;
    }

    Ok(fresh)
}

pub async fn get_active_session_from_db(
    pool: &PgPool,
    session: &Session,
) -> AppResult<UserSession> {
    let user = sync_session_with_db(pool, session).await?;
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

pub async fn set_flash(session: &Session, kind: &str, message: &str) -> AppResult<()> {
    session
        .insert(
            FLASH_KEY,
            FlashMessage {
                kind: kind.to_string(),
                message: message.to_string(),
            },
        )
        .await
        .map_err(|e| AppError::Internal(e.into()))
}

pub async fn take_flash(session: &Session) -> AppResult<Option<FlashMessage>> {
    let flash = session
        .get::<FlashMessage>(FLASH_KEY)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;
    if flash.is_some() {
        session
            .remove::<FlashMessage>(FLASH_KEY)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;
    }
    Ok(flash)
}
