ALTER TABLE requirement_types
    ADD COLUMN expires_after_days INT CHECK (expires_after_days IS NULL OR expires_after_days > 0);

ALTER TABLE employee_requirements
    ADD COLUMN expires_at TIMESTAMPTZ;

ALTER TABLE eod_reports
    ADD COLUMN unlocked_at TIMESTAMPTZ,
    ADD COLUMN unlocked_by UUID REFERENCES employees(id);