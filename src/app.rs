use axum::{
    routing::{get, post},
    Router,
};
use tower_sessions::{service::CookieController, SessionManagerLayer, SessionStore};

use crate::handlers::{admin, auth, employee, health, manager};
use crate::state::AppState;

pub fn create_app<Store, C>(
    state: AppState,
    session_layer: SessionManagerLayer<Store, C>,
) -> Router
where
    Store: SessionStore + Clone + Send + Sync + 'static,
    C: CookieController + Clone + Send + Sync + 'static,
{
    let health_route = Router::new()
        .route("/health", get(health::health))
        .with_state(state.clone());

    let public = Router::new()
        .route("/login", get(auth::login_page).post(auth::login_submit))
        .route("/change-pin", get(auth::change_pin_page).post(auth::change_pin_submit))
        .route("/logout", post(auth::logout));

    let employee_routes = Router::new()
        .route("/", get(employee::home))
        .route("/clock/in", post(employee::clock_in_action))
        .route("/clock/out", post(employee::clock_out_action))
        .route("/me/timesheet", get(employee::timesheet));

    let manager_routes = Router::new()
        .route("/manager", get(manager::dashboard))
        .route("/manager/team", get(manager::team_list))
        .route("/manager/team/{employee_id}", get(manager::team_timesheet))
        .route(
            "/manager/team/{employee_id}/correct",
            get(manager::new_correction_form),
        )
        .route("/manager/correct/{entry_id}", get(manager::correct_form))
        .route("/manager/correct", post(manager::submit_correction))
        .route("/manager/no-show", post(manager::mark_no_show))
        .route("/manager/ot/{id}/review", post(manager::review_ot));

    let admin_routes = Router::new()
        .route("/admin/employees", get(admin::employees_page).post(admin::create_employee_action))
        .route(
            "/admin/employees/{employee_id}",
            get(admin::edit_employee_page).post(admin::update_employee_action),
        )
        .route(
            "/admin/employees/{employee_id}/reset-pin",
            post(admin::reset_pin_action),
        )
        .route(
            "/admin/employees/{employee_id}/toggle-active",
            post(admin::toggle_active_action),
        )
        .route("/admin/shifts/{employee_id}", get(admin::shifts_page))
        .route("/admin/shifts", post(admin::save_shift))
        .route("/admin/settings", get(admin::settings_page).post(admin::save_settings))
        .route("/admin/reports", get(admin::reports_page))
        .route("/admin/reports/export.csv", get(admin::export_csv))
        .route("/admin/reports/export.xlsx", get(admin::export_xlsx));

    let app_routes = Router::new()
        .merge(public)
        .merge(employee_routes)
        .merge(manager_routes)
        .merge(admin_routes)
        .layer(session_layer)
        .with_state(state)
        .layer(tower_http::trace::TraceLayer::new_for_http())
        .nest_service("/static", tower_http::services::ServeDir::new("static"));

    Router::new().merge(health_route).merge(app_routes)
}