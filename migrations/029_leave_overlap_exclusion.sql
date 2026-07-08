-- Prevent overlapping pending/approved leave for the same employee at the database level.

CREATE EXTENSION IF NOT EXISTS btree_gist;

ALTER TABLE leave_requests
    ADD CONSTRAINT leave_requests_no_overlap_pending_approved
    EXCLUDE USING gist (
        employee_id WITH =,
        daterange(start_date, end_date, '[]') WITH &&
    )
    WHERE (status IN ('pending', 'approved'));