use serde::Serialize;
use sqlx::PgPool;
use uuid::Uuid;

use crate::auth::UserSession;
use crate::error::{AppError, AppResult};
use crate::services::{
    eod::{count_missing_team_eod, needs_eod_reminder},
    leave::count_pending_for_manager as count_pending_leave,
    ot::count_pending,
    pin_reset::count_pending_for_reviewer,
    requirements::is_requirement_expired,
    settings::get_settings,
    timezone::{company_date_now, format_time},
};

pub const EXPIRY_WARNING_DAYS: i64 = 30;

#[derive(Debug, Clone, Serialize)]
pub struct Notification {
    pub key: String,
    pub kind: String,
    pub severity: String,
    pub title: String,
    pub message: String,
    pub href: String,
}

pub async fn list_for_user(pool: &PgPool, user: &UserSession) -> AppResult<Vec<Notification>> {
    let mut items = Vec::new();
    let settings = get_settings(pool).await?;
    let today = company_date_now(&settings)?;

    if needs_eod_reminder(pool, user.employee_id).await? {
        items.push(Notification {
            key: format!("missing_eod:{today}"),
            kind: "missing_eod".into(),
            severity: "warning".into(),
            title: "EOD not submitted".into(),
            message: "You clocked in today but haven't submitted your end-of-day report yet."
                .into(),
            href: "/me/eod".into(),
        });
    }

    items.extend(
        expiring_requirement_notifications(pool, user.employee_id, &settings.timezone).await?,
    );

    if user.role.is_manager_or_admin() {
        let is_admin = user.role.is_admin();
        let pending_ot = count_pending(pool, user.employee_id, is_admin).await?;
        if pending_ot > 0 {
            let message = if pending_ot == 1 {
                "1 overtime request needs your approval.".into()
            } else {
                format!("{pending_ot} overtime requests need your approval.")
            };
            items.push(Notification {
                key: format!("pending_ot:{pending_ot}"),
                kind: "pending_ot".into(),
                severity: "warning".into(),
                title: "Pending OT approvals".into(),
                message,
                href: "/manager".into(),
            });
        }

        let missing_eod = count_missing_team_eod(pool, user.employee_id, is_admin).await?;
        if missing_eod > 0 {
            let message = if missing_eod == 1 {
                "1 team member clocked in but has not submitted EOD today.".into()
            } else {
                format!("{missing_eod} team members clocked in but have not submitted EOD today.")
            };
            items.push(Notification {
                key: format!("missing_team_eod:{today}:{missing_eod}"),
                kind: "missing_team_eod".into(),
                severity: "warning".into(),
                title: "Team EOD missing".into(),
                message,
                href: "/manager/eod".into(),
            });
        }

        let pending_leave = count_pending_leave(pool, user.employee_id, is_admin).await?;
        if pending_leave > 0 {
            let message = if pending_leave == 1 {
                "1 leave request needs your review.".into()
            } else {
                format!("{pending_leave} leave requests need your review.")
            };
            items.push(Notification {
                key: format!("pending_leave:{pending_leave}"),
                kind: "pending_leave".into(),
                severity: "warning".into(),
                title: "Pending leave requests".into(),
                message,
                href: "/manager/leave".into(),
            });
        }

        let pending_pin_resets =
            count_pending_for_reviewer(pool, user.employee_id, is_admin).await?;
        if pending_pin_resets > 0 {
            let message = if pending_pin_resets == 1 {
                "1 PIN reset request needs your review.".into()
            } else {
                format!("{pending_pin_resets} PIN reset requests need your review.")
            };
            items.push(Notification {
                key: format!("pending_pin_resets:{pending_pin_resets}"),
                kind: "pending_pin_resets".into(),
                severity: "warning".into(),
                title: "PIN reset requests".into(),
                message,
                href: "/manager/pin-resets".into(),
            });
        }

        let pending_req_reviews =
            count_pending_requirement_reviews_for_manager(pool, user.employee_id, is_admin).await?;
        if pending_req_reviews > 0 {
            let message = if pending_req_reviews == 1 {
                "1 team requirement is waiting for review.".into()
            } else {
                format!("{pending_req_reviews} team requirements are waiting for review.")
            };
            items.push(Notification {
                key: format!("pending_requirement_reviews:{pending_req_reviews}"),
                kind: "pending_requirement_reviews".into(),
                severity: "info".into(),
                title: "Requirements to review".into(),
                message,
                href: "/manager/requirements".into(),
            });
        }
    }

    let dismissed = list_dismissed_keys(pool, user.employee_id).await?;
    items.retain(|item| !dismissed.contains(&item.key));
    Ok(items)
}

pub async fn dismiss(pool: &PgPool, employee_id: Uuid, key: &str) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO notification_dismissals (employee_id, notification_key)
         VALUES ($1, $2)
         ON CONFLICT (employee_id, notification_key) DO UPDATE
         SET dismissed_at = now()",
    )
    .bind(employee_id)
    .bind(key)
    .execute(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(())
}

pub async fn dismiss_all(pool: &PgPool, employee_id: Uuid, keys: &[String]) -> AppResult<()> {
    for key in keys {
        dismiss(pool, employee_id, key).await?;
    }
    Ok(())
}

async fn list_dismissed_keys(pool: &PgPool, employee_id: Uuid) -> AppResult<Vec<String>> {
    let rows: Vec<String> = sqlx::query_scalar(
        "SELECT notification_key FROM notification_dismissals WHERE employee_id = $1",
    )
    .bind(employee_id)
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(rows)
}

async fn expiring_requirement_notifications(
    pool: &PgPool,
    employee_id: Uuid,
    timezone: &str,
) -> AppResult<Vec<Notification>> {
    #[derive(sqlx::FromRow)]
    struct Row {
        expires_at: time::OffsetDateTime,
    }

    let rows = sqlx::query_as::<_, Row>(
        "SELECT er.expires_at
         FROM employee_requirements er
         JOIN requirement_types rt ON rt.id = er.requirement_type_id
         WHERE er.employee_id = $1
           AND er.status = 'approved'
           AND er.expires_at IS NOT NULL
           AND er.expires_at < now() + ($2 || ' days')::interval
         ORDER BY er.expires_at",
    )
    .bind(employee_id)
    .bind(EXPIRY_WARNING_DAYS)
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    let mut items = Vec::new();
    let mut expired = 0i64;
    let mut expiring = 0i64;

    for row in &rows {
        if is_requirement_expired(Some(row.expires_at)) {
            expired += 1;
        } else {
            expiring += 1;
        }
    }

    if expired > 0 {
        let message = if expired == 1 {
            "1 approved requirement has expired — please re-submit it.".into()
        } else {
            format!("{expired} approved requirements have expired — please re-submit them.")
        };
        items.push(Notification {
            key: format!("requirement_expired:{expired}"),
            kind: "requirement_expired".into(),
            severity: "urgent".into(),
            title: "Expired requirements".into(),
            message,
            href: "/me/requirements".into(),
        });
    }

    if expiring > 0 {
        let soonest = rows
            .iter()
            .find(|r| !is_requirement_expired(Some(r.expires_at)))
            .map(|r| format_time(r.expires_at, timezone));
        let message = if expiring == 1 {
            if let Some(when) = soonest {
                format!("1 requirement expires on {when}.")
            } else {
                "1 requirement is expiring within 30 days.".into()
            }
        } else {
            format!("{expiring} requirements expire within 30 days.")
        };
        items.push(Notification {
            key: format!("requirement_expiring:{expiring}"),
            kind: "requirement_expiring".into(),
            severity: "info".into(),
            title: "Requirements expiring soon".into(),
            message,
            href: "/me/requirements".into(),
        });
    }

    Ok(items)
}

async fn count_pending_requirement_reviews_for_manager(
    pool: &PgPool,
    manager_id: Uuid,
    is_admin: bool,
) -> AppResult<i64> {
    let count: i64 = if is_admin {
        sqlx::query_scalar(
            "SELECT COUNT(*)
             FROM employee_requirements
             WHERE status = 'submitted'",
        )
        .fetch_one(pool)
        .await
    } else {
        sqlx::query_scalar(
            "SELECT COUNT(*)
             FROM employee_requirements er
             JOIN employees e ON e.id = er.employee_id
             WHERE er.status = 'submitted' AND e.manager_id = $1",
        )
        .bind(manager_id)
        .fetch_one(pool)
        .await
    }
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(count)
}
