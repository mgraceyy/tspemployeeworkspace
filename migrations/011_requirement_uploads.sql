ALTER TABLE requirement_types
    ADD COLUMN requires_upload BOOLEAN NOT NULL DEFAULT FALSE;

ALTER TABLE employee_requirements
    ADD COLUMN file_name TEXT,
    ADD COLUMN file_stored_path TEXT,
    ADD COLUMN file_mime TEXT,
    ADD COLUMN file_size BIGINT CHECK (file_size IS NULL OR file_size > 0);