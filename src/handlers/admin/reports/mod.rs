mod exports;
mod page;
mod pay_period;
mod presets;

pub use exports::{export_csv, export_detail_csv, export_xlsx};
pub use page::reports_page;
pub use pay_period::{close_pay_period_action, reopen_pay_period_action};
pub use presets::{delete_report_preset_action, save_report_preset_action};

use crate::models::UserRole;
use crate::services::reports::PayrollFilters;

pub(crate) fn payroll_filters_from_query(query: &page::ReportQuery) -> PayrollFilters {
    let role = query.role.as_deref().and_then(|r| match r {
        "employee" => Some(UserRole::Employee),
        "manager" => Some(UserRole::Manager),
        "admin" => Some(UserRole::Admin),
        _ => None,
    });
    PayrollFilters {
        department: query
            .department
            .as_ref()
            .map(|d| d.trim().to_string())
            .filter(|d| !d.is_empty()),
        role,
        employee_id: query.employee_id,
    }
}
