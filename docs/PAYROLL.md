# Payroll roadmap

This document plans how TalaSora Prime DTR evolves from **time & attendance reporting** into a **payroll preparation system**. It is written against the codebase as of v0.1.0.

## What exists today (v0.1.0)

The app already handles the **inputs** payroll depends on:

| Area | Status |
|------|--------|
| Clock in/out with break deduction | Done |
| Regular vs OT split (`ot_threshold_minutes`) | Done |
| OT approval workflow (pending → approved/rejected) | Done |
| Leave & absence types (sick, vacation, official, offset, no-show) | Done |
| Pay period calendar (weekly / biweekly / semimonthly / monthly) | Done |
| Pay period **close** (freeze edits) and **reopen** | Done |
| Payroll **summary** report (hours + leave day counts) | Done |
| Per-day **detail** export (CSV) | Done |
| Summary **Excel** export | Done |
| Department / role / employee filters | Done |
| Company timezone (`Asia/Manila`, etc.) | Done |

**Not built yet:** pay rates, gross pay, statutory deductions, net pay, payslips, or remittance filings.

Today’s exports answer: *“How many payable hours and leave days did each employee accrue this period?”*  
They do **not** answer: *“How much money do we owe each employee?”*

---

## Design principle

Keep DTR as the **system of record for time**. Add payroll as a **calculation layer** on top of closed periods, not mixed into daily clocking.

```
Time entries + approvals
        ↓
  Close pay period  ← admin sign-off on attendance
        ↓
  Payroll run       ← snapshot rates + compute amounts
        ↓
  Payslips / export ← handoff to accounting or bank
```

Closing the period before a payroll run prevents retroactive clock edits from changing computed pay.

---

## Phase 1 — Compensation master data

**Goal:** Store how each employee is paid.

### Schema (proposed)

```sql
-- migration 018_compensation.sql
CREATE TYPE pay_type AS ENUM ('monthly', 'daily', 'hourly');

CREATE TABLE compensation_profiles (
    employee_id UUID PRIMARY KEY REFERENCES employees(id),
    pay_type pay_type NOT NULL DEFAULT 'monthly',
    base_amount NUMERIC(12,2) NOT NULL,  -- monthly salary, daily rate, or hourly rate
    ot_multiplier NUMERIC(4,2) NOT NULL DEFAULT 1.25,  -- PH default: 125% for ordinary OT
    effective_from DATE NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by UUID REFERENCES employees(id)
);

CREATE TABLE compensation_history (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    employee_id UUID NOT NULL REFERENCES employees(id),
    pay_type pay_type NOT NULL,
    base_amount NUMERIC(12,2) NOT NULL,
    ot_multiplier NUMERIC(4,2) NOT NULL,
    effective_from DATE NOT NULL,
    effective_to DATE,
    changed_by UUID REFERENCES employees(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

### UI

- Admin → Employee → **Compensation** tab on profile/edit
- Fields: pay type, base amount, OT multiplier, effective date
- Audit log entry on every change

### Rules

- One active profile per employee (latest `effective_from <= period_end`)
- History table for rate changes mid-year
- Managers cannot see compensation (admin only)

### Tests

- Unit: rate lookup as of a given date
- HTTP: admin can save compensation; employee cannot view

---

## Phase 2 — Payroll run engine

**Goal:** Turn closed-period hours into **gross pay** per employee.

### Schema

```sql
CREATE TYPE payroll_run_status AS ENUM ('draft', 'finalized', 'voided');

CREATE TABLE payroll_runs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    period_start DATE NOT NULL,
    period_end DATE NOT NULL,
    status payroll_run_status NOT NULL DEFAULT 'draft',
    note TEXT,
    created_by UUID NOT NULL REFERENCES employees(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    finalized_at TIMESTAMPTZ,
    UNIQUE (period_start, period_end, status)  -- one finalized run per period
);

CREATE TABLE payroll_lines (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    run_id UUID NOT NULL REFERENCES payroll_runs(id) ON DELETE CASCADE,
    employee_id UUID NOT NULL REFERENCES employees(id),
    regular_minutes INT NOT NULL DEFAULT 0,
    approved_ot_minutes INT NOT NULL DEFAULT 0,
    gross_pay NUMERIC(12,2) NOT NULL,
    -- deductions added in Phase 3
    net_pay NUMERIC(12,2) NOT NULL,
    UNIQUE (run_id, employee_id)
);
```

### Calculation logic (`src/services/payroll/compute.rs`)

| Pay type | Gross formula (simplified) |
|----------|---------------------------|
| **Hourly** | `(regular_min / 60) × hourly_rate + (ot_min / 60) × hourly_rate × ot_multiplier` |
| **Daily** | `days_worked × daily_rate + OT premium portion` |
| **Monthly** | `monthly_rate × (period_days_worked / expected_period_days)` prorated, plus OT premium |

**OT premium only:** For monthly/daily employees, OT is typically the *premium* on top of base (e.g. 25% of hourly equivalent), not full OT hours × full rate. Confirm with your accountant — make multipliers configurable per company.

### Workflow

1. Admin opens **Payroll Runs** (new page under `/admin/payroll`)
2. Select a **closed** pay period
3. **Preview** — compute lines from `payroll_summary()` + compensation profiles
4. **Finalize** — write `payroll_runs` + `payroll_lines`; block re-run unless voided
5. Export **gross pay CSV/Excel** for accounting

### Guards

- Cannot finalize if period is not fully closed
- Cannot finalize if any employee lacks compensation profile
- Warn (don't block) if pending OT exists — already excluded from payable minutes

### Tests

- Integration: monthly employee, 10 working days, 2h approved OT → expected gross
- HTTP: finalize blocked on open period

---

## Phase 3 — Deductions

**Goal:** Arrive at **net pay**.

### Approach A (recommended for v1): Manual deduction lines

```sql
CREATE TABLE deduction_types (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    code TEXT NOT NULL UNIQUE,  -- e.g. SSS, PHIC, HDMF, WHT, LOAN
    name TEXT NOT NULL,
    is_statutory BOOLEAN NOT NULL DEFAULT false
);

CREATE TABLE payroll_deductions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    line_id UUID NOT NULL REFERENCES payroll_lines(id) ON DELETE CASCADE,
    deduction_type_id UUID NOT NULL REFERENCES deduction_types(id),
    amount NUMERIC(12,2) NOT NULL,
    note TEXT
);
```

Admin enters or imports deduction amounts per employee per run. Net pay = gross − sum(deductions).

**Why start here:** Philippine SSS/PhilHealth/Pag-IBIG tables change; withholding tax brackets are complex. Manual entry unblocks payslips while you decide on auto-calculation.

### Approach B (later): Statutory auto-calc

- Seed deduction types: SSS, PhilHealth, Pag-IBIG, Withholding Tax
- Config tables for contribution brackets (versioned by year)
- Inputs: gross pay, civil status, dependents (new profile fields)
- Output: suggested deduction amounts admin can override

### Tests

- Net pay = gross − deductions
- Finalized run totals match sum of lines

---

## Phase 4 — Payslips

**Goal:** Employees and admins can view/download a payslip per finalized run.

### Schema

```sql
CREATE TABLE payslips (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    line_id UUID NOT NULL UNIQUE REFERENCES payroll_lines(id),
    issued_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

### UI

- **Admin:** `/admin/payroll/runs/{id}` — table of employees, gross, deductions, net; download all PDFs/ZIP
- **Employee:** `/me/payslips` — list finalized payslips for that employee only

### Export format

- HTML → print/PDF (start simple: printable HTML page, same as timesheet export pattern)
- Include: company name, period, earnings breakdown, deduction breakdown, net pay

### Security

- Employees see only their own payslips
- Payslips immutable after run finalized (void run = mark payslips void, don't delete)

---

## Phase 5 — Philippine labor premiums (optional)

Add when the business needs full DOLE-style compliance beyond basic OT:

| Premium | Typical rate | Trigger |
|---------|--------------|---------|
| Ordinary OT | 125% | Already partially covered via `ot_multiplier` |
| Rest day OT | 169% | Work on scheduled rest day |
| Special non-working holiday | 130% | Holiday table + worked day |
| Regular holiday | 200% | Holiday table + worked day |
| Night differential | +10% | Clock events 10pm–6am |

Requires:

- Rest-day assignment per employee (from shift templates)
- Holiday calendar already exists — wire premiums into `hours.rs` or a new `premium.rs`
- Separate **premium minutes** columns on `payroll_lines`

**Recommendation:** Defer until Phase 2 is stable. Many small shops run on monthly + OT premium only.

---

## Phase 6 — Accounting handoff

**Goal:** Export formats your bookkeeper or bank expects.

| Export | Contents |
|--------|----------|
| **Bank upload CSV** | employee code, account number (new field), net pay |
| **Journal entry CSV** | debit salary expense, credit SSS payable, credit net wages, etc. |
| **13th month accrual** | optional report: `(basic_pay / 12) × months worked` |

---

## Suggested implementation order

```
Phase 1  Compensation profiles     ~1 week
Phase 2  Payroll run + gross       ~2 weeks
Phase 3  Manual deductions        ~1 week
Phase 4  Payslips (HTML)          ~1 week
Phase 5  PH premiums              ~2–3 weeks (optional)
Phase 6  Bank / GL exports        ~1 week
```

Total to **usable net-pay payslips**: about **5–6 weeks** of focused work after v0.1.0.

---

## Open questions (decide before Phase 2)

1. **Pay types in use** — Are all employees monthly, or mix of daily/hourly (production floor)?
2. **OT treatment for monthly staff** — Premium-only vs full hourly rate for OT hours?
3. **Absence without pay** — Do no-show days reduce monthly gross, or tracked for discipline only?
4. **Leave with pay** — Sick/vacation days: full day credit, or no impact on monthly salary?
5. **Who runs payroll** — Same admin, or separate finance role?
6. **Accounting system** — Excel handoff, or specific bank upload format?
7. **Statutory auto-calc** — Required at launch, or manual deductions for first 3 months?

---

## Routes (planned)

| Route | Role | Purpose |
|-------|------|---------|
| `/admin/payroll` | Admin | List payroll runs |
| `/admin/payroll/new` | Admin | Create draft run for closed period |
| `/admin/payroll/{id}` | Admin | Preview / finalize / export |
| `/admin/employees/{id}/compensation` | Admin | Edit pay rate |
| `/me/payslips` | Employee | View own payslips |
| `/me/payslips/{id}` | Employee | Single payslip detail |

---

## Relationship to current reports

Keep `/admin/reports` as the **attendance** view (hours, leave counts, period close).  
Add `/admin/payroll` as the **money** view (runs, gross, deductions, net).

The attendance export remains the pre-payroll sanity check; the payroll run consumes the same `payroll_summary()` data after close.