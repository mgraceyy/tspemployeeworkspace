CREATE TABLE report_presets (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL UNIQUE,
    department TEXT,
    role user_role,
    employee_id UUID REFERENCES employees(id) ON DELETE SET NULL,
    created_by UUID NOT NULL REFERENCES employees(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE closed_pay_periods (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    period_start DATE NOT NULL,
    period_end DATE NOT NULL,
    closed_by UUID NOT NULL REFERENCES employees(id),
    closed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    note TEXT,
    UNIQUE (period_start, period_end)
);

CREATE INDEX idx_closed_pay_periods_range ON closed_pay_periods (period_start, period_end);