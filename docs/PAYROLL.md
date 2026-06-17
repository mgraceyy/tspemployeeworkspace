# Payroll roadmap

TalaSora Prime DTR payroll policy and implementation plan. Updated after v0.1.0 with **locked business rules**.

## Locked policy (TalaSora Prime)

| Decision | Choice |
|----------|--------|
| Pay type | **All monthly** — no hourly/daily employees |
| Overtime | **132%** of hourly equivalent (configurable per employee, default 132) |
| No-shows | **Reduce pay** — one daily rate deducted per no-show day |
| Sick / vacation / official / offset leave | **Informational only** — tracked in reports, no pay adjustment |
| Deductions (SSS, PhilHealth, tax, loans) | **Manual entry** per employee on draft payroll runs |

### Gross pay formula (monthly employees)

Uses **26 working days / month** and **8 hours / day** (480 minutes) for rate derivation:

```
daily_rate     = monthly_salary ÷ 26
hourly_rate    = daily_rate ÷ 8

period_base    = monthly_salary × period_factor
                 (semimonthly = ½, monthly = 1, weekly/biweekly per calendar)

no_show_deduction = daily_rate × no_show_days

ot_pay         = (approved_ot_minutes ÷ 60) × hourly_rate × (ot_rate_percent ÷ 100)

gross_pay      = period_base − no_show_deduction + ot_pay
```

Leave day counts in payroll reports do **not** change `gross_pay`.

Implementation: `src/services/payroll/compute.rs`

---

## What exists today

### v0.1.0 — Time & attendance

| Area | Status |
|------|--------|
| Clock in/out, break, OT split | Done |
| OT approval workflow | Done |
| Leave & absence tracking | Done |
| Pay period close / reopen | Done |
| Hours + leave summary exports | Done |

### v0.2.0 — Phase 1 compensation ✅

| Area | Status |
|------|--------|
| `compensation_profiles` table | Done |
| Admin UI `/admin/employees/{id}/compensation` | Done |
| Rate history on salary change | Done |
| Gross pay calculation module (unit tested) | Done |

### v0.2.0 — Phase 2 payroll runs ✅

| Area | Status |
|------|--------|
| `payroll_runs` + `payroll_lines` tables | Done |
| Admin UI `/admin/payroll` | Done |
| Draft run from closed period | Done |
| Finalize (lock gross pay) | Done |
| Manual deductions → net pay | Done |
| Payslips | **Phase 4** |

---

## Design principle

```
Time entries + approvals
        ↓
  Close pay period
        ↓
  Payroll run (uses compensation + compute.rs)
        ↓
  Manual deductions → net pay → payslips
```

---

## Phase 1 — Compensation master data ✅

**Schema:** `migrations/018_compensation.sql`

- `compensation_profiles` — current monthly salary (cents), OT rate %, effective date
- `compensation_history` — prior rates when salary changes

**Admin UI:** Employee → Compensation tab

**Access:** Admin only (employees receive 403)

---

## Phase 2 — Payroll run engine ✅

**Goal:** Preview and finalize gross pay for a **closed** pay period.

### Schema (planned)

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
    finalized_at TIMESTAMPTZ
);

CREATE TABLE payroll_lines (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    run_id UUID NOT NULL REFERENCES payroll_runs(id) ON DELETE CASCADE,
    employee_id UUID NOT NULL REFERENCES employees(id),
    regular_minutes INT NOT NULL DEFAULT 0,
    approved_ot_minutes INT NOT NULL DEFAULT 0,
    no_show_days INT NOT NULL DEFAULT 0,
    base_pay_cents BIGINT NOT NULL,
    no_show_deduction_cents BIGINT NOT NULL,
    ot_pay_cents BIGINT NOT NULL,
    gross_pay_cents BIGINT NOT NULL,
    net_pay_cents BIGINT NOT NULL,
    UNIQUE (run_id, employee_id)
);
```

### Workflow

1. `/admin/payroll` — list runs
2. Create draft for a fully **closed** period
3. Pull `payroll_summary()` per employee + `compensation_profiles`
4. Run `gross_pay_cents()` — preview table
5. Finalize → immutable snapshot

### Guards

- Period must be closed
- Every active employee needs compensation profile
- Warn if pending OT exists (excluded from gross)

---

## Phase 3 — Manual deductions ✅

**Schema:** `migrations/020_payroll_deductions.sql`

- `deduction_types` — seeded: SSS, PhilHealth, Pag-IBIG, withholding tax, loan, other
- `payroll_deductions` — amounts per employee line on a draft run

```
net_pay = gross_pay − sum(deductions)
```

**Admin UI:** Payroll run → **Deductions** per employee (`/admin/payroll/{run_id}/lines/{line_id}`). Edits blocked after finalize. Total deductions cannot exceed gross pay.

---

## Phase 4 — Payslips

- `/admin/payroll/runs/{id}` — all employees, gross, deductions, net
- `/me/payslips` — employee view (own records only)
- Printable HTML payslip (company, period, earnings, deductions, net)

---

## Phase 5 — PH labor premiums (deferred)

Rest-day OT, holiday premiums, night differential — only if DOLE-full compliance is required beyond 132% ordinary OT.

---

## Phase 6 — Accounting handoff

- Bank upload CSV (needs `bank_account` on profile)
- Journal entry export
- 13th-month accrual report

---

## Routes

| Route | Status |
|-------|--------|
| `/admin/employees/{id}/compensation` | ✅ Done |
| `/admin/payroll` | ✅ Done |
| `/admin/payroll/{id}` | ✅ Done |
| `/admin/payroll/{id}/finalize` | ✅ Done |
| `/admin/payroll/{run_id}/lines/{line_id}` | ✅ Done |
| `/me/payslips` | Phase 4 |

---

## Operational decisions (locked)

| Topic | Choice |
|-------|--------|
| Who runs payroll | **Admin** (same role as today — no separate finance user) |
| Bank / accounting export | **Deferred** — manual handoff outside the app for now |
| 13th month | **External** — keep in spreadsheet / accounting tool |

---

## Relationship to attendance reports

`/admin/reports` = hours & leave sanity check before close  
`/admin/payroll` = money after close (Phase 2+)

Attendance exports remain the pre-payroll review; payroll runs consume the same underlying data.