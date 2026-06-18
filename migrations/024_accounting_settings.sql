-- P2: configurable journal GL accounts for payroll exports
ALTER TABLE company_settings
    ADD COLUMN journal_salary_expense_account TEXT NOT NULL DEFAULT '5100',
    ADD COLUMN journal_net_payable_account TEXT NOT NULL DEFAULT '2100',
    ADD COLUMN journal_salary_expense_label TEXT NOT NULL DEFAULT 'Salaries expense',
    ADD COLUMN journal_net_payable_label TEXT NOT NULL DEFAULT 'Net pay payable';