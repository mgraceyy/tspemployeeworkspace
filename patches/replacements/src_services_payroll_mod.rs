pub mod compute;
pub mod deductions;
pub mod deductions_auto;
pub mod export;
pub mod gov_deductions;
pub mod labor_premiums;
pub mod payslips;
pub mod pdf;
pub mod runs;

pub use compute::{
    base_pay_cents_for_period, gross_pay_cents, hourly_rate_cents, no_show_deduction_cents,
    ot_pay_cents, period_salary_factor, round_div, GrossPayInput, MONTHLY_WORKING_DAYS,
    STANDARD_DAILY_MINUTES,
};
pub use deductions::{
    get_line_for_run, list_deduction_types, list_deductions_for_line,
    parse_optional_amount_to_cents, save_line_deductions, DeductionInput,
};
pub use deductions_auto::{
    apply_automatic_deductions_for_run, preflight_finalize_run,
    recalculate_government_deductions_for_run, FinalizePreflight,
};
pub use export::{
    build_bank_upload_csv, build_finalized_run_csv, build_journal_export_csv,
    count_missing_bank_accounts_for_run,
};
pub use gov_deductions::{
    is_government_deduction_code, period_government_deductions_cents, GovernmentDeductionToggles,
    AUTO_NOTE,
};
pub use labor_premiums::{
    day_premium_cents, load_labor_premium_toggles, sum_premium_pay_for_period, DayPremiumInput,
    DayPremiumMinutes, LaborPremiumToggles,
};
pub use payslips::{
    get_payslip_for_admin, get_payslip_for_employee, list_payslips_for_employee, PayslipDetail,
    PayslipListItem,
};
pub use pdf::build_payslip_pdf;
pub use runs::{
    create_draft_run, employees_missing_compensation, finalize_run, get_active_run_for_period,
    get_run, inactive_employee_count, is_draft_attendance_stale, list_lines_for_run,
    list_runnable_closed_periods, list_runs, total_deduction_cents, total_gross_cents,
    total_net_cents, total_pending_ot_minutes, void_draft_run, ClosedPeriodCandidate,
    PayrollRunListItem, PeriodPayrollStatus,
};