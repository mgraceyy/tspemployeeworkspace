DROP INDEX IF EXISTS payroll_runs_one_finalized_per_period;

CREATE UNIQUE INDEX payroll_runs_one_active_per_period
    ON payroll_runs (period_start, period_end)
    WHERE status IN ('draft', 'finalized');

CREATE UNIQUE INDEX payroll_deductions_line_type_unique
    ON payroll_deductions (line_id, deduction_type_id);

ALTER TABLE payroll_lines
    ADD CONSTRAINT payroll_lines_net_within_gross
    CHECK (net_pay_cents >= 0 AND net_pay_cents <= gross_pay_cents);