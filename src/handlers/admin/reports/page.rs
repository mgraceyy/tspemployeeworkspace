use axum::extract::{Query, State};
use minijinja::context;
use serde::Deserialize;
use time::Date;
use tower_sessions::Session;
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::error::AppResult;
use crate::handlers::render::{render_page, HtmlPage};
use crate::models::PayrollRunStatus;
use crate::models::UserRole;
use crate::services::{
    employees::list_all,
    onboarding::list_distinct_departments,
    payroll::runs::{employees_missing_compensation, get_active_run_for_period},
    payroll_controls::{
        is_period_closed, is_period_exactly_closed, list_overlapping_closed_periods,
        list_report_presets, ReportPreset,
    },
    reports::{
        assert_canonical_pay_period, current_pay_period, minutes_to_hours_decimal,
        pay_period_label, payroll_summary, resolve_report_period, ReportPeriod,
    },
    settings::get_settings,
    timezone::{company_date_now, format_date},
};
use crate::state::AppState;

use super::payroll_filters_from_query;

#[derive(Deserialize, Default)]
pub struct ReportQuery {
    pub start: Option<String>,
    pub end: Option<String>,
    pub department: Option<String>,
    pub role: Option<String>,
    pub employee_id: Option<Uuid>,
}

fn build_report_rows(
    rows: &[crate::services::reports::PayrollRow],
) -> Vec<minijinja::value::Value> {
    rows.iter()
        .map(|row| {
            context! {
                employee_code => row.employee_code.clone(),
                full_name => row.full_name.clone(),
                department => row.department.clone().unwrap_or_default(),
                regular_hours => minutes_to_hours_decimal(row.regular_minutes),
                approved_ot_hours => minutes_to_hours_decimal(row.approved_ot_minutes),
                pending_ot_hours => minutes_to_hours_decimal(row.pending_ot_minutes),
                payable_hours => minutes_to_hours_decimal(row.regular_minutes + row.approved_ot_minutes),
                sick_leave_days => row.sick_leave_days,
                vacation_days => row.vacation_days,
                official_leave_days => row.official_leave_days,
                offset_days => row.offset_days,
                no_show_days => row.no_show_days,
            }
        })
        .collect()
}

fn export_query_string(start: Date, end: Date, query: &ReportQuery) -> String {
    let mut parts = vec![
        format!("start={}", format_date(start)),
        format!("end={}", format_date(end)),
    ];
    if let Some(ref dept) = query.department {
        if !dept.trim().is_empty() {
            parts.push(format!("department={}", urlencoding_encode(dept)));
        }
    }
    if let Some(ref role) = query.role {
        if !role.trim().is_empty() {
            parts.push(format!("role={role}"));
        }
    }
    if let Some(id) = query.employee_id {
        parts.push(format!("employee_id={id}"));
    }
    format!("?{}", parts.join("&"))
}

fn urlencoding_encode(value: &str) -> String {
    value
        .chars()
        .map(|c| match c {
            ' ' => "%20".to_string(),
            '&' => "%26".to_string(),
            '=' => "%3D".to_string(),
            _ if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') => c.to_string(),
            _ => format!("%{:02X}", c as u32),
        })
        .collect()
}

fn role_query_value(role: UserRole) -> &'static str {
    match role {
        UserRole::Employee => "employee",
        UserRole::Manager => "manager",
        UserRole::Admin => "admin",
    }
}

fn preset_apply_url(preset: &ReportPreset, period: &ReportPeriod) -> String {
    let query = ReportQuery {
        start: Some(format_date(period.start)),
        end: Some(format_date(period.end)),
        department: preset.department.clone(),
        role: preset.role.map(role_query_value).map(str::to_string),
        employee_id: preset.employee_id,
    };
    format!(
        "/admin/reports{}",
        export_query_string(period.start, period.end, &query)
    )
}

pub async fn reports_page(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
    Query(query): Query<ReportQuery>,
) -> AppResult<HtmlPage> {
    let settings = get_settings(&state.pool).await?;
    let today = company_date_now(&settings)?;
    let period = resolve_report_period(
        &settings,
        today,
        query.start.as_deref(),
        query.end.as_deref(),
    )?;
    let filters = payroll_filters_from_query(&query);
    let rows = payroll_summary(&state.pool, period.start, period.end, &filters).await?;
    let custom_range = query.start.is_some() && query.end.is_some();
    let employees = list_all(&state.pool).await?;
    let departments = list_distinct_departments(&state.pool).await?;
    let presets = list_report_presets(&state.pool).await?;
    let period_closed = is_period_closed(&state.pool, period.start, period.end).await?;
    let period_exactly_closed =
        is_period_exactly_closed(&state.pool, period.start, period.end).await?;
    let overlapping_closed =
        list_overlapping_closed_periods(&state.pool, period.start, period.end).await?;
    let mut overlapping_closed_rows = Vec::new();
    for row in &overlapping_closed {
        let payroll_run =
            get_active_run_for_period(&state.pool, row.period_start, row.period_end).await?;
        let (reopen_blocked, reopen_blocked_reason, payroll_run_url) = match payroll_run {
            Some(run) => {
                let reason = if run.status == PayrollRunStatus::Draft {
                    "Void the draft payroll run before reopening this period."
                } else {
                    "Payroll is finalized for this period — reopen is blocked."
                };
                (
                    true,
                    reason.to_string(),
                    Some(format!("/admin/payroll/{}", run.id)),
                )
            }
            None => (false, String::new(), None),
        };
        overlapping_closed_rows.push(context! {
            start_date => format_date(row.period_start),
            end_date => format_date(row.period_end),
            note => row.note.clone().unwrap_or_default(),
            reopen_blocked => reopen_blocked,
            reopen_blocked_reason => reopen_blocked_reason,
            payroll_run_url => payroll_run_url.unwrap_or_default(),
        });
    }

    let employee_options: Vec<_> = employees
        .iter()
        .filter(|e| e.is_active)
        .map(|e| {
            context! {
                id => e.id,
                label => format!("{} ({})", e.full_name, e.employee_code),
            }
        })
        .collect();

    let preset_rows: Vec<_> = presets
        .iter()
        .map(|preset| {
            context! {
                id => preset.id,
                name => preset.name.clone(),
                apply_url => preset_apply_url(preset, &period),
            }
        })
        .collect();

    let is_canonical_period =
        assert_canonical_pay_period(&settings, period.start, period.end).is_ok();
    let (_, _, canonical_period_label) =
        current_pay_period(period.end, settings.pay_period, settings.pay_period_anchor);
    let total_pending_ot_minutes: i64 = rows.iter().map(|r| r.pending_ot_minutes).sum();
    let mut missing_compensation =
        employees_missing_compensation(&state.pool, period.end).await?;
    missing_compensation.sort();
    let payroll_run = if period_exactly_closed {
        get_active_run_for_period(&state.pool, period.start, period.end).await?
    } else {
        None
    };
    let (payroll_run_id, payroll_run_status, payroll_run_url) = match payroll_run {
        Some(run) => {
            let status = match run.status {
                PayrollRunStatus::Draft => "Draft",
                PayrollRunStatus::Finalized => "Finalized",
                PayrollRunStatus::Voided => "Voided",
            };
            (
                Some(run.id),
                Some(status.to_string()),
                Some(format!("/admin/payroll/{}", run.id)),
            )
        }
        None => (None, None, None),
    };

    render_page(
        &state,
        &session,
        Some(user),
        &settings.company_name,
        "Payroll Reports",
        "admin/reports.html",
        context! {
            period_label => period.label,
            pay_period_type => if custom_range { "Custom" } else { pay_period_label(settings.pay_period) },
            settings_pay_period_label => pay_period_label(settings.pay_period),
            start_date => format_date(period.start),
            end_date => format_date(period.end),
            custom_range => custom_range,
            period_closed => period_closed,
            period_exactly_closed => period_exactly_closed,
            is_canonical_period => is_canonical_period,
            canonical_period_label => canonical_period_label,
            has_pending_ot => total_pending_ot_minutes > 0,
            total_pending_ot_minutes => total_pending_ot_minutes,
            total_pending_ot_hours => format!("{:.2}", minutes_to_hours_decimal(total_pending_ot_minutes)),
            payroll_run_id => payroll_run_id,
            payroll_run_status => payroll_run_status.unwrap_or_default(),
            payroll_run_url => payroll_run_url.unwrap_or_default(),
            payroll_ready => period_exactly_closed && is_canonical_period && payroll_run_id.is_none(),
            overlapping_closed_periods => overlapping_closed_rows,
            export_query => export_query_string(period.start, period.end, &query),
            filter_department => query.department.clone().unwrap_or_default(),
            filter_role => query.role.clone().unwrap_or_default(),
            filter_employee_id => query.employee_id.map(|id| id.to_string()).unwrap_or_default(),
            departments => departments,
            employees => employee_options,
            presets => preset_rows,
            rows => build_report_rows(&rows),
            missing_compensation => missing_compensation,
        },
    )
    .await
}
