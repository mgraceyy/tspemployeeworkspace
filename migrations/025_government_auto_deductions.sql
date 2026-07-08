-- Government benefit auto-deduction toggles (SSS, PhilHealth, Pag-IBIG, withholding tax)

ALTER TABLE company_settings
    ADD COLUMN auto_deduct_sss BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN auto_deduct_phic BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN auto_deduct_hdmf BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN auto_deduct_wht BOOLEAN NOT NULL DEFAULT FALSE;