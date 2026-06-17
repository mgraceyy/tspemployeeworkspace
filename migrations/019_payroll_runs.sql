CREATE TYPE payroll_run_status AS ENUM ('draft', 'finalized', 'voided');

CREATE TABLE payroll_runs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    period_start DATE NOT NULL,
    period_end DATE NOT NULL,
    status payroll_run_status NOT NULL DEFAULT 'draft',
    note TEXT,
    created_by UUID NOT NULL REFERENCES employees(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    finalized_at TIMESTAMPTZ,
    finalized_by UUID REFERENCES employees(id)
);

CREATE TABLE payroll_lines (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    run_id UUID NOT NULL REFERENCES payroll_runs(id) ON DELETE CASCADE,
    employee_id UUID NOT NULL REFERENCES employees(id),
    regular_minutes INT NOT NULL DEFAULT 0,
    approved_ot_minutes INT NOT NULL DEFAULT 0,
    pending_ot_minutes INT NOT NULL DEFAULT 0,
    no_show_days INT NOT NULL DEFAULT 0,
    base_pay_cents BIGINT NOT NULL,
    no_show_deduction_cents BIGINT NOT NULL,
    ot_pay_cents BIGINT NOT NULL,
    gross_pay_cents BIGINT NOT NULL,
    net_pay_cents BIGINT NOT NULL,
    UNIQUE (run_id, employee_id)
);

CREATE UNIQUE INDEX payroll_runs_one_finalized_per_period
    ON payroll_runs (period_start, period_end)
    WHERE status = 'finalized';

CREATE INDEX payroll_runs_created_at_idx ON payroll_runs (created_at DESC);
CREATE INDEX payroll_lines_run_idx ON payroll_lines (run_id);