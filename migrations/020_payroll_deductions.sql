CREATE TABLE deduction_types (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    code TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL
);

CREATE TABLE payroll_deductions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    line_id UUID NOT NULL REFERENCES payroll_lines(id) ON DELETE CASCADE,
    deduction_type_id UUID NOT NULL REFERENCES deduction_types(id),
    amount_cents BIGINT NOT NULL CHECK (amount_cents > 0),
    note TEXT
);

CREATE INDEX payroll_deductions_line_idx ON payroll_deductions (line_id);

INSERT INTO deduction_types (code, name) VALUES
    ('SSS', 'SSS'),
    ('PHIC', 'PhilHealth'),
    ('HDMF', 'Pag-IBIG'),
    ('WHT', 'Withholding tax'),
    ('LOAN', 'Loan repayment'),
    ('OTHER', 'Other');