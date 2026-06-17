CREATE TYPE eod_report_status AS ENUM ('draft', 'submitted');
CREATE TYPE eod_task_kind AS ENUM ('completed', 'pending', 'blocked', 'planned');

CREATE TABLE eod_reports (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    employee_id UUID NOT NULL REFERENCES employees(id) ON DELETE CASCADE,
    report_date DATE NOT NULL,
    summary TEXT NOT NULL DEFAULT '',
    status eod_report_status NOT NULL DEFAULT 'draft',
    submitted_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (employee_id, report_date)
);

CREATE TABLE eod_tasks (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    eod_report_id UUID NOT NULL REFERENCES eod_reports(id) ON DELETE CASCADE,
    kind eod_task_kind NOT NULL,
    title TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    sort_order INT NOT NULL DEFAULT 0
);

CREATE INDEX idx_eod_reports_date ON eod_reports (report_date DESC);
CREATE INDEX idx_eod_reports_employee_date ON eod_reports (employee_id, report_date DESC);