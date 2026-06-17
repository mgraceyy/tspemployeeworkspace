ALTER TYPE attendance_status ADD VALUE IF NOT EXISTS 'sick_leave';
ALTER TYPE attendance_status ADD VALUE IF NOT EXISTS 'vacation';
ALTER TYPE attendance_status ADD VALUE IF NOT EXISTS 'official_leave';

CREATE TABLE company_holidays (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    holiday_date DATE NOT NULL UNIQUE,
    name TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_company_holidays_date ON company_holidays (holiday_date);