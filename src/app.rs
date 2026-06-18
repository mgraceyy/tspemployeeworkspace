use axum::{
    middleware,
    routing::{get, post},
    Router,
};
use tower_sessions::{service::CookieController, SessionManagerLayer, SessionStore};

use crate::auth::{
    csrf::validate_post, inject_active_session, post_limiter::limit_post_requests,
    require_admin_role, require_manager_role,
};
use crate::handlers::{
    admin, auth, employee, eod, health, leave, manager, metrics, notifications, payslips, profile,
    requirements,
};
use crate::middleware::request_metrics::record_request_metrics;
use crate::middleware::security_headers::add_security_headers;
use crate::middleware::static_cache::add_static_cache_headers;
use crate::state::AppState;

pub fn create_app<Store, C>(state: AppState, session_layer: SessionManagerLayer<Store, C>) -> Router
where
    Store: SessionStore + Clone + Send + Sync + 'static,
    C: CookieController + Clone + Send + Sync + 'static,
{
    let health_route = Router::new()
        .route("/health", get(health::health))
        .route("/metrics", get(metrics::prometheus_metrics));

    let public = Router::new()
        .route("/login", get(auth::login_page).post(auth::login_submit))
        .route(
            "/change-pin",
            get(auth::change_pin_page).post(auth::change_pin_submit),
        )
        .route("/logout", post(auth::logout))
        .route(
            "/login/request-pin-reset",
            get(auth::pin_reset_request_page).post(auth::pin_reset_request_submit),
        );

    let employee_routes = Router::new()
        .route("/", get(employee::home))
        .route("/clock/in", post(employee::clock_in_action))
        .route("/clock/out", post(employee::clock_out_action))
        .route("/me/timesheet", get(employee::timesheet))
        .route(
            "/me/timesheet/export.csv",
            get(employee::export_my_timesheet_csv),
        )
        .route(
            "/me/leave",
            get(leave::my_leave_page).post(leave::submit_leave_request),
        )
        .route(
            "/me/leave/{request_id}/cancel",
            post(leave::cancel_leave_request),
        )
        .route("/me/holidays", get(employee::holidays_page))
        .route(
            "/me/profile",
            get(profile::my_profile).post(profile::update_my_profile),
        )
        .route("/me/profile/photo", get(profile::my_profile_photo))
        .route(
            "/me/profile/photo/upload",
            post(profile::upload_my_profile_photo),
        )
        .route(
            "/me/profile/logout-everywhere",
            post(profile::logout_everywhere),
        )
        .route(
            "/me/profile/request-pin-reset",
            post(profile::request_pin_reset),
        )
        .route(
            "/me/profile/pin-reset/{request_id}/cancel",
            post(profile::cancel_pin_reset),
        )
        .route(
            "/employees/{employee_id}/photo",
            get(profile::employee_profile_photo),
        )
        .route("/me/requirements", get(requirements::my_requirements))
        .route(
            "/me/requirements/{requirement_id}/submit",
            post(requirements::submit_my_requirement),
        )
        .route(
            "/me/requirements/{requirement_id}/file",
            get(requirements::download_my_requirement_file),
        )
        .route("/me/eod", get(eod::my_eod).post(eod::save_my_eod))
        .route("/me/eod/history", get(eod::my_eod_history))
        .route("/me/team/eod", get(eod::team_eod_feed))
        .route("/me/eod/{report_id}", get(eod::view_eod_detail))
        .route("/me/payslips", get(payslips::my_payslips_page))
        .route("/me/payslips/{line_id}", get(payslips::view_my_payslip))
        .route(
            "/me/payslips/{line_id}/payslip.pdf",
            get(payslips::export_my_payslip_pdf),
        )
        .route("/notifications", get(notifications::notifications_page))
        .route(
            "/notifications/dismiss",
            post(notifications::dismiss_notification),
        )
        .route(
            "/notifications/dismiss-all",
            post(notifications::dismiss_all_notifications),
        )
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            inject_active_session,
        ));

    let manager_routes = Router::new()
        .route("/manager", get(manager::dashboard))
        .route("/manager/team", get(manager::team_list))
        .route(
            "/manager/team/{employee_id}/export.csv",
            get(manager::export_team_timesheet_csv),
        )
        .route("/manager/team/{employee_id}", get(manager::team_timesheet))
        .route(
            "/manager/team/{employee_id}/profile",
            get(profile::manager_work_profile),
        )
        .route(
            "/manager/team/{employee_id}/correct",
            get(manager::new_correction_form),
        )
        .route("/manager/correct/{entry_id}", get(manager::correct_form))
        .route("/manager/correct", post(manager::submit_correction))
        .route("/manager/absence", post(manager::mark_absence))
        .route("/manager/ot/{id}/review", post(manager::review_ot))
        .route("/manager/eod", get(eod::manager_eod_page))
        .route(
            "/manager/eod/export.csv",
            get(eod::manager_export_weekly_csv),
        )
        .route("/manager/eod/{employee_id}", get(eod::manager_view_eod))
        .route("/manager/pin-resets", get(manager::pin_resets_page))
        .route(
            "/manager/pin-resets/{request_id}/approve",
            post(manager::approve_pin_reset),
        )
        .route(
            "/manager/pin-resets/{request_id}/deny",
            post(manager::deny_pin_reset),
        )
        .route("/manager/leave", get(leave::manager_leave_page))
        .route(
            "/manager/leave/{request_id}/review",
            post(leave::review_leave_request),
        )
        .route(
            "/manager/requirements",
            get(requirements::manager_requirements_queue),
        )
        .route(
            "/manager/team/{employee_id}/requirements",
            get(requirements::manager_employee_requirements),
        )
        .route(
            "/manager/team/{employee_id}/requirements/{requirement_id}/review",
            post(requirements::manager_review_employee_requirement),
        )
        .route(
            "/manager/team/{employee_id}/requirements/{requirement_id}/file",
            get(requirements::download_manager_requirement_file),
        )
        .route_layer(middleware::from_fn(require_manager_role))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            inject_active_session,
        ));

    let admin_routes = Router::new()
        .route(
            "/admin/employees",
            get(admin::employees_page).post(admin::create_employee_action),
        )
        .route(
            "/admin/employees/bulk-department",
            post(admin::bulk_assign_department_action),
        )
        .route(
            "/admin/employees/{employee_id}",
            get(admin::edit_employee_page).post(admin::update_employee_action),
        )
        .route(
            "/admin/employees/{employee_id}/profile",
            get(profile::admin_profile_page).post(profile::admin_update_profile),
        )
        .route(
            "/admin/employees/{employee_id}/compensation",
            get(admin::compensation_page).post(admin::save_compensation_action),
        )
        .route(
            "/admin/employees/{employee_id}/compensation/deduction-defaults",
            post(admin::save_deduction_defaults_action),
        )
        .route(
            "/admin/compensation/import",
            get(admin::compensation_import_page).post(admin::compensation_import_preview_action),
        )
        .route(
            "/admin/compensation/import/apply",
            post(admin::compensation_import_apply_action),
        )
        .route(
            "/admin/deduction-types",
            get(admin::deduction_types_page).post(admin::create_deduction_type_action),
        )
        .route(
            "/admin/deduction-types/{type_id}/toggle",
            post(admin::toggle_deduction_type_action),
        )
        .route(
            "/admin/employees/{employee_id}/requirements",
            get(requirements::admin_employee_requirements),
        )
        .route(
            "/admin/employees/{employee_id}/requirements/{requirement_id}/review",
            post(requirements::review_employee_requirement),
        )
        .route(
            "/admin/employees/{employee_id}/requirements/{requirement_id}/file",
            get(requirements::download_admin_requirement_file),
        )
        .route(
            "/admin/employees/{employee_id}/reset-pin",
            post(admin::reset_pin_action),
        )
        .route(
            "/admin/employees/{employee_id}/toggle-active",
            post(admin::toggle_active_action),
        )
        .route(
            "/admin/requirements",
            get(requirements::admin_types_page).post(requirements::save_requirement_type),
        )
        .route("/admin/shifts/{employee_id}", get(admin::shifts_page))
        .route("/admin/shifts", post(admin::save_shift))
        .route(
            "/admin/settings",
            get(admin::settings_page).post(admin::save_settings),
        )
        .route(
            "/admin/holidays",
            get(admin::holidays_page).post(admin::add_holiday_action),
        )
        .route(
            "/admin/holidays/{holiday_id}/delete",
            post(admin::delete_holiday_action),
        )
        .route(
            "/admin/payroll",
            get(admin::payroll_runs_page).post(admin::create_payroll_run_action),
        )
        .route("/admin/payroll/{run_id}", get(admin::payroll_run_page))
        .route(
            "/admin/payroll/{run_id}/lines/{line_id}",
            get(admin::payroll_line_deductions_page)
                .post(admin::save_payroll_line_deductions_action),
        )
        .route(
            "/admin/payroll/{run_id}/lines/{line_id}/payslip",
            get(admin::admin_payslip_page),
        )
        .route(
            "/admin/payroll/{run_id}/void",
            post(admin::void_payroll_run_action),
        )
        .route(
            "/admin/payroll/{run_id}/finalize",
            post(admin::finalize_payroll_run_action),
        )
        .route(
            "/admin/payroll/{run_id}/export.csv",
            get(admin::export_payroll_run_csv),
        )
        .route(
            "/admin/payroll/{run_id}/export-bank.csv",
            get(admin::export_payroll_bank_csv),
        )
        .route(
            "/admin/payroll/{run_id}/export-journal.csv",
            get(admin::export_payroll_journal_csv),
        )
        .route(
            "/admin/payroll/{run_id}/lines/{line_id}/payslip.pdf",
            get(admin::export_admin_payslip_pdf),
        )
        .route("/admin/reports", get(admin::reports_page))
        .route(
            "/admin/reports/presets",
            post(admin::save_report_preset_action),
        )
        .route(
            "/admin/reports/presets/{preset_id}/delete",
            post(admin::delete_report_preset_action),
        )
        .route(
            "/admin/reports/close-period",
            post(admin::close_pay_period_action),
        )
        .route(
            "/admin/reports/reopen-period",
            post(admin::reopen_pay_period_action),
        )
        .route("/admin/reports/export.csv", get(admin::export_csv))
        .route(
            "/admin/reports/export-detail.csv",
            get(admin::export_detail_csv),
        )
        .route("/admin/reports/export.xlsx", get(admin::export_xlsx))
        .route("/admin/corrections", get(admin::corrections_page))
        .route("/admin/audit", get(admin::audit_page))
        .route("/admin/eod", get(eod::admin_eod_page))
        .route("/admin/eod/{report_id}/unlock", post(eod::admin_unlock_eod))
        .route_layer(middleware::from_fn(require_admin_role))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            inject_active_session,
        ));

    let app_routes = Router::new()
        .merge(public)
        .merge(employee_routes)
        .merge(manager_routes)
        .merge(admin_routes)
        .layer(middleware::from_fn(add_security_headers))
        .layer(middleware::from_fn(validate_post))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            limit_post_requests,
        ))
        .layer(session_layer);

    let static_routes = Router::new()
        .nest_service("/static", tower_http::services::ServeDir::new("static"))
        .layer(middleware::from_fn(add_static_cache_headers));

    Router::new()
        .merge(health_route)
        .merge(app_routes)
        .merge(static_routes)
        .layer(middleware::from_fn_with_state(
            state.clone(),
            record_request_metrics,
        ))
        .with_state(state)
        .layer(tower_http::trace::TraceLayer::new_for_http())
}
