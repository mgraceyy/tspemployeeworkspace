mod audit;
mod common;
mod corrections;
mod employees;
mod holidays;
mod reports;
mod settings;
mod shifts;

pub use audit::audit_page;
pub use corrections::corrections_page;
pub use employees::{
    bulk_assign_department_action, create_employee_action, edit_employee_page, employees_page,
    reset_pin_action, toggle_active_action, update_employee_action,
};
pub use holidays::{add_holiday_action, delete_holiday_action, holidays_page};
pub use reports::{
    close_pay_period_action, delete_report_preset_action, export_csv, export_detail_csv,
    export_xlsx, reopen_pay_period_action, reports_page, save_report_preset_action,
};
pub use settings::{save_settings, settings_page};
pub use shifts::{save_shift, shifts_page};
