CREATE TABLE employee_profiles (
    employee_id UUID PRIMARY KEY REFERENCES employees(id) ON DELETE CASCADE,
    contact_number TEXT,
    personal_email TEXT,
    birthdate DATE,
    address TEXT,
    emergency_contact_name TEXT,
    emergency_contact_phone TEXT,
    job_title TEXT,
    department TEXT,
    employment_type TEXT,
    date_hired DATE,
    work_location TEXT,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by UUID REFERENCES employees(id)
);

INSERT INTO employee_profiles (employee_id)
SELECT id FROM employees
ON CONFLICT (employee_id) DO NOTHING;