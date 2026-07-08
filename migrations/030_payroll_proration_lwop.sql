-- Salary proration (hire / separation dates) and LWOP unpaid-leave deductions

ALTER TABLE employee_profiles
    ADD COLUMN date_separated DATE;

ALTER TYPE attendance_status ADD VALUE IF NOT EXISTS 'lwop';

INSERT INTO deduction_types (code, name, sort_order)
VALUES ('LWOP', 'Leave without pay', 45)
ON CONFLICT (code) DO NOTHING;

ALTER TABLE payroll_lines
    ADD COLUMN lwop_days INT NOT NULL DEFAULT 0,
    ADD COLUMN employed_days INT,
    ADD COLUMN period_calendar_days INT;