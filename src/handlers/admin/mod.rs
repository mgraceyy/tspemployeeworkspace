mod audit;
mod common;
mod compensation;
mod corrections;
mod deduction_types;
mod employees;
mod holidays;
mod payroll;
mod reports;
mod settings;
mod shifts;

pub use crate::handlers::payslips::admin_payslip_page;
pub use audit::audit_page;
pub use compensation::{
    compensation_import_apply_action, compensation_import_page,
    compensation_import_preview_action, compensation_page, save_compensation_action,
    save_deduction_defaults_action,
};
pub use deduction_types::{
    create_deduction_type_action, deduction_types_page, toggle_deduction_type_action,
};
pub use corrections::corrections_page;
pub use employees::{
    bulk_assign_department_action, create_employee_action, edit_employee_page, employees_page,
    reset_pin_action, toggle_active_action, update_employee_action,
};
pub use holidays::{add_holiday_action, delete_holiday_action, holidays_page};
pub use payroll::{
    create_payroll_run_action, export_payroll_bank_csv, export_payroll_journal_csv,
    export_payroll_run_csv, finalize_payroll_run_action,
    payroll_line_deductions_page, payroll_run_page, payroll_runs_page,
    save_payroll_line_deductions_action, void_payroll_run_action,
};
pub use crate::handlers::payslips::{export_admin_payslip_pdf, export_my_payslip_pdf};
pub use reports::{
    close_pay_period_action, delete_report_preset_action, export_csv, export_detail_csv,
    export_xlsx, reopen_pay_period_action, reports_page, save_report_preset_action,
};
pub use settings::{save_settings, settings_page};
pub use shifts::{save_shift, shifts_page};
