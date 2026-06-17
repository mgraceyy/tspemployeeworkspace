-- Monthly compensation profiles (TalaSora Prime: all employees monthly)

CREATE TABLE compensation_profiles (
    employee_id UUID PRIMARY KEY REFERENCES employees(id) ON DELETE CASCADE,
    monthly_salary_cents BIGINT NOT NULL CHECK (monthly_salary_cents > 0),
    ot_rate_percent INT NOT NULL DEFAULT 132 CHECK (ot_rate_percent >= 100 AND ot_rate_percent <= 300),
    effective_from DATE NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by UUID REFERENCES employees(id)
);

CREATE TABLE compensation_history (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    employee_id UUID NOT NULL REFERENCES employees(id) ON DELETE CASCADE,
    monthly_salary_cents BIGINT NOT NULL,
    ot_rate_percent INT NOT NULL,
    effective_from DATE NOT NULL,
    effective_to DATE,
    changed_by UUID REFERENCES employees(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX compensation_history_employee_idx ON compensation_history (employee_id, effective_from DESC);