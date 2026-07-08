-- PH labor premiums: rest-day, holiday, night differential (enable/disable per company)

ALTER TABLE company_settings
    ADD COLUMN premium_rest_day BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN premium_holiday BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN premium_night_diff BOOLEAN NOT NULL DEFAULT FALSE;

ALTER TABLE payroll_lines
    ADD COLUMN premium_pay_cents BIGINT NOT NULL DEFAULT 0 CHECK (premium_pay_cents >= 0);