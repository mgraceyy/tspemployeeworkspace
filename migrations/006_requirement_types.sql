CREATE TYPE requirement_status AS ENUM ('missing', 'submitted', 'approved', 'rejected');

CREATE TABLE requirement_types (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    is_required BOOLEAN NOT NULL DEFAULT TRUE,
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    sort_order INT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE employee_requirements (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    employee_id UUID NOT NULL REFERENCES employees(id) ON DELETE CASCADE,
    requirement_type_id UUID NOT NULL REFERENCES requirement_types(id) ON DELETE CASCADE,
    status requirement_status NOT NULL DEFAULT 'missing',
    employee_note TEXT,
    admin_note TEXT,
    submitted_at TIMESTAMPTZ,
    reviewed_by UUID REFERENCES employees(id),
    reviewed_at TIMESTAMPTZ,
    UNIQUE (employee_id, requirement_type_id)
);

CREATE INDEX idx_employee_requirements_employee ON employee_requirements (employee_id);