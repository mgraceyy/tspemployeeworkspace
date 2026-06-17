CREATE EXTENSION IF NOT EXISTS pgcrypto;

CREATE TYPE user_role AS ENUM ('employee', 'manager', 'admin');
CREATE TYPE attendance_status AS ENUM ('on_time', 'late', 'absent', 'no_show', 'partial');
CREATE TYPE pay_period_type AS ENUM ('weekly', 'biweekly', 'semimonthly', 'monthly');
CREATE TYPE ot_status AS ENUM ('none', 'pending', 'approved', 'rejected');

CREATE TABLE company_settings (
    id SMALLINT PRIMARY KEY DEFAULT 1 CHECK (id = 1),
    company_name TEXT NOT NULL DEFAULT 'TalaSora Prime',
    break_minutes INT NOT NULL DEFAULT 60,
    ot_threshold_minutes INT NOT NULL DEFAULT 480,
    grace_minutes INT NOT NULL DEFAULT 5,
    pay_period pay_period_type NOT NULL DEFAULT 'semimonthly',
    timezone TEXT NOT NULL DEFAULT 'Asia/Manila',
    ot_requires_approval BOOLEAN NOT NULL DEFAULT TRUE
);

INSERT INTO company_settings (id) VALUES (1);

CREATE TABLE employees (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    employee_code TEXT NOT NULL UNIQUE,
    full_name TEXT NOT NULL,
    pin_hash TEXT NOT NULL,
    role user_role NOT NULL DEFAULT 'employee',
    manager_id UUID REFERENCES employees(id),
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE shift_templates (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    employee_id UUID NOT NULL REFERENCES employees(id) ON DELETE CASCADE,
    day_of_week SMALLINT NOT NULL CHECK (day_of_week BETWEEN 0 AND 6),
    start_time TIME NOT NULL,
    end_time TIME NOT NULL,
    UNIQUE (employee_id, day_of_week)
);

CREATE TABLE time_entries (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    employee_id UUID NOT NULL REFERENCES employees(id),
    work_date DATE NOT NULL,
    clock_in TIMESTAMPTZ,
    clock_out TIMESTAMPTZ,
    gross_minutes INT,
    net_minutes INT,
    regular_minutes INT,
    ot_minutes INT NOT NULL DEFAULT 0,
    ot_status ot_status NOT NULL DEFAULT 'none',
    ot_reviewed_by UUID REFERENCES employees(id),
    ot_reviewed_at TIMESTAMPTZ,
    ot_note TEXT,
    attendance attendance_status,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (employee_id, work_date)
);

CREATE TABLE correction_logs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    time_entry_id UUID NOT NULL REFERENCES time_entries(id),
    edited_by UUID NOT NULL REFERENCES employees(id),
    reason TEXT NOT NULL,
    old_clock_in TIMESTAMPTZ,
    old_clock_out TIMESTAMPTZ,
    new_clock_in TIMESTAMPTZ,
    new_clock_out TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_time_entries_employee_date ON time_entries (employee_id, work_date);
CREATE INDEX idx_time_entries_ot_pending ON time_entries (ot_status) WHERE ot_status = 'pending';
CREATE INDEX idx_employees_manager ON employees (manager_id);