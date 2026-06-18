-- v0.3.0 Wave 2: allowances, deduction defaults, payroll pack exports

ALTER TABLE compensation_profiles
    ADD COLUMN transport_allowance_cents BIGINT NOT NULL DEFAULT 0
        CHECK (transport_allowance_cents >= 0),
    ADD COLUMN meal_allowance_cents BIGINT NOT NULL DEFAULT 0
        CHECK (meal_allowance_cents >= 0);

ALTER TABLE compensation_history
    ADD COLUMN transport_allowance_cents BIGINT NOT NULL DEFAULT 0,
    ADD COLUMN meal_allowance_cents BIGINT NOT NULL DEFAULT 0;

ALTER TABLE payroll_lines
    ADD COLUMN allowance_cents BIGINT NOT NULL DEFAULT 0;

ALTER TABLE payroll_runs
    ADD COLUMN attendance_snapshot_hash TEXT;

ALTER TABLE deduction_types
    ADD COLUMN is_active BOOLEAN NOT NULL DEFAULT TRUE,
    ADD COLUMN sort_order INT NOT NULL DEFAULT 0;

UPDATE deduction_types SET sort_order = 10 WHERE code = 'SSS';
UPDATE deduction_types SET sort_order = 20 WHERE code = 'PHIC';
UPDATE deduction_types SET sort_order = 30 WHERE code = 'HDMF';
UPDATE deduction_types SET sort_order = 40 WHERE code = 'WHT';
UPDATE deduction_types SET sort_order = 50 WHERE code = 'LOAN';
UPDATE deduction_types SET sort_order = 60 WHERE code = 'OTHER';

CREATE TABLE employee_deduction_defaults (
    employee_id UUID NOT NULL REFERENCES employees(id) ON DELETE CASCADE,
    deduction_type_id UUID NOT NULL REFERENCES deduction_types(id) ON DELETE CASCADE,
    amount_cents BIGINT NOT NULL DEFAULT 0 CHECK (amount_cents >= 0),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by UUID REFERENCES employees(id),
    PRIMARY KEY (employee_id, deduction_type_id)
);