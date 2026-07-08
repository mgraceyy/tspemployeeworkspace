-- Speed up manager leave queues and pending-balance aggregation

CREATE INDEX leave_requests_pending_idx
    ON leave_requests (employee_id, leave_type)
    WHERE status = 'pending';

CREATE INDEX leave_requests_manager_pending_idx
    ON leave_requests (status, created_at)
    WHERE status = 'pending';