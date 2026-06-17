CREATE INDEX idx_time_entries_work_date ON time_entries (work_date);

CREATE INDEX idx_employee_requirements_submitted
    ON employee_requirements (status, employee_id)
    WHERE status = 'submitted';

CREATE INDEX idx_leave_requests_dates ON leave_requests (start_date, end_date);