-- Half-day leave requests and balance precision (tenths of a day)

CREATE TYPE leave_day_portion AS ENUM ('full_day', 'half_day');

ALTER TABLE leave_requests
    ADD COLUMN day_portion leave_day_portion NOT NULL DEFAULT 'full_day';

-- Reinterpret employee balance columns as tenths of a day (10 = one full day).
UPDATE employee_leave_balances SET balance_days = balance_days * 10;