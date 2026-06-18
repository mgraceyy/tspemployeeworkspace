-- v0.3.0 foundation: session invalidation, payroll identity fields, profile photos, PIN reset requests

ALTER TABLE employees
    ADD COLUMN session_version INT NOT NULL DEFAULT 0;

ALTER TABLE employee_profiles
    ADD COLUMN bank_account TEXT,
    ADD COLUMN tin TEXT,
    ADD COLUMN sss_number TEXT,
    ADD COLUMN philhealth_number TEXT,
    ADD COLUMN photo_path TEXT;

CREATE TYPE pin_reset_request_status AS ENUM ('pending', 'approved', 'denied', 'cancelled');

CREATE TABLE pin_reset_requests (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    employee_id UUID NOT NULL REFERENCES employees(id) ON DELETE CASCADE,
    reason TEXT,
    status pin_reset_request_status NOT NULL DEFAULT 'pending',
    requested_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    reviewed_by UUID REFERENCES employees(id),
    reviewed_at TIMESTAMPTZ,
    review_note TEXT
);

CREATE UNIQUE INDEX pin_reset_requests_one_pending_per_employee
    ON pin_reset_requests (employee_id)
    WHERE status = 'pending';

CREATE INDEX pin_reset_requests_status_requested
    ON pin_reset_requests (status, requested_at DESC);