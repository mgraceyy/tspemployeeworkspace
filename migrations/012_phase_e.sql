CREATE TYPE leave_request_status AS ENUM ('pending', 'approved', 'rejected', 'cancelled');

CREATE TYPE leave_request_type AS ENUM ('sick_leave', 'vacation', 'official_leave', 'offset');

CREATE TABLE leave_requests (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    employee_id UUID NOT NULL REFERENCES employees(id) ON DELETE CASCADE,
    start_date DATE NOT NULL,
    end_date DATE NOT NULL,
    leave_type leave_request_type NOT NULL,
    reason TEXT,
    status leave_request_status NOT NULL DEFAULT 'pending',
    reviewer_id UUID REFERENCES employees(id),
    reviewer_note TEXT,
    reviewed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (end_date >= start_date)
);

CREATE INDEX idx_leave_requests_employee ON leave_requests (employee_id);
CREATE INDEX idx_leave_requests_status ON leave_requests (status);

CREATE TABLE notification_dismissals (
    employee_id UUID NOT NULL REFERENCES employees(id) ON DELETE CASCADE,
    notification_key TEXT NOT NULL,
    dismissed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (employee_id, notification_key)
);