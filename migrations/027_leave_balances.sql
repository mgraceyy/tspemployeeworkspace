-- Leave balance tracking (vacation + sick leave)

ALTER TABLE company_settings
    ADD COLUMN default_vacation_days INT NOT NULL DEFAULT 15 CHECK (default_vacation_days >= 0),
    ADD COLUMN default_sick_days INT NOT NULL DEFAULT 15 CHECK (default_sick_days >= 0);

CREATE TABLE employee_leave_balances (
    employee_id UUID NOT NULL REFERENCES employees(id) ON DELETE CASCADE,
    leave_type leave_request_type NOT NULL,
    balance_days INT NOT NULL DEFAULT 0 CHECK (balance_days >= 0),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (employee_id, leave_type),
    CHECK (leave_type IN ('vacation', 'sick_leave'))
);

CREATE INDEX employee_leave_balances_employee_idx ON employee_leave_balances (employee_id);