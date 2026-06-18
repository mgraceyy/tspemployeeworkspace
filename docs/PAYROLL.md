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

allowance_pay  = monthly_allowances × period_factor

gross_pay      = period_base + allowance_pay − no_show_deduction + ot_pay
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

### v0.3.0 — Payroll pack ✅

| Area | Status |
|------|--------|
| Transport + meal allowances on compensation profiles | Done |
| Allowances in gross pay + payroll lines + payslips | Done |
| Compensation CSV import (`/admin/compensation/import`) | Done |
| Per-employee deduction defaults (auto-applied on draft creation) | Done |
| Deduction types admin (`/admin/deduction-types`) | Done |
| Bank upload CSV (`/admin/payroll/{run_id}/export-bank.csv`) | Done |
| Journal entry CSV (`/admin/payroll/{run_id}/export-journal.csv`) | Done |
| PDF payslips (employee + admin) | Done |
| Attendance snapshot hash + stale-draft warning | Done |

### v0.3.0 — Foundation ✅

| Area | Status |
|------|--------|
| Profile photo upload | Done |
| Bank account, TIN, SSS, PhilHealth on profile | Done |
| PIN reset request workflow | Done |
| Employee archive filter on admin list | Done |
| Logout everywhere + session version invalidation | Done |

### v0.2.0 — Phase 2 payroll runs ✅

| Area | Status |
|------|--------|
| `payroll_runs` + `payroll_lines` tables | Done |
| Admin UI `/admin/payroll` | Done |
| Draft run from closed period | Done |
| Finalize (lock gross pay) | Done |
| Manual deductions → net pay | Done |
| Payslips (employee + printable) | Done |

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

### Schema (implemented — `migrations/019_payroll_runs.sql`, `020_payroll_deductions.sql`, `021_payroll_integrity.sql`)

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
2. Create draft for a fully **closed** canonical pay period
3. Pull `payroll_summary()` per employee + compensation as of `period_end`
4. Run `gross_pay_cents()` — preview table
5. Enter deductions per employee
6. Finalize → immutable snapshot (blocked while pending OT remains)
7. **Void draft** if the run was created in error

### Guards

- Period must be closed and match a full configured pay period (semimonthly, monthly, etc.)
- Every active employee needs compensation effective on `period_end` (current profile or history row)
- Pending OT blocks finalize (excluded from gross until approved)
- Period reopen blocked while a draft or finalized payroll run exists
- Draft creation is transactional; one active run per period (DB-enforced)

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

## Phase 4 — Payslips ✅

- `/admin/payroll/{run_id}` — run summary with **Payslip** link per employee (finalized runs)
- `/admin/payroll/{run_id}/lines/{line_id}/payslip` — printable admin view
- `/me/payslips` — employee list (finalized runs only, own records)
- `/me/payslips/{line_id}` — printable HTML payslip (earnings, deductions, net)
- Print via browser (Ctrl+P); nav hidden in print layout

---

## Phase 5 — PH labor premiums (deferred)

Rest-day OT, holiday premiums, night differential — only if DOLE-full compliance is required beyond 132% ordinary OT.

---

## Salary and hire-date policy (locked)

| Topic | Policy |
|-------|--------|
| Pay type | All employees are **monthly**; weekly/biweekly/semimonthly settings only change the **period factor** applied to monthly salary |
| New hires (`date_hired`) | **No proration** — active employees receive the full period base for the pay period, regardless of hire date or days worked in the range |
| Mid-period salary change | Rate effective on **`period_end`** only — no within-period proration; prior rates live in `compensation_history` |
| Draft run lines | **Snapshot at creation** — gross pay does not auto-refresh; void the draft, reopen the period in Reports, fix data, close again, then create a new run |

---

## Phase 6 — Accounting handoff (partial ✅)

| Item | Status |
|------|--------|
| Bank upload CSV (`export-bank.csv`, uses `bank_account` on profile) | ✅ Done |
| Journal entry CSV (`export-journal.csv`) | ✅ Done |
| 13th-month accrual report | Deferred — external spreadsheet |

---

## Routes

| Route | Status |
|-------|--------|
| `/admin/employees/{id}/compensation` | ✅ Done |
| `/admin/payroll` | ✅ Done |
| `/admin/payroll/{id}` | ✅ Done |
| `/admin/payroll/{id}/finalize` | ✅ Done |
| `/admin/payroll/{run_id}/export.csv` | ✅ Done (includes allowances) |
| `/admin/payroll/{run_id}/export-bank.csv` | ✅ Done |
| `/admin/payroll/{run_id}/export-journal.csv` | ✅ Done |
| `/admin/payroll/{run_id}/lines/{line_id}/payslip.pdf` | ✅ Done |
| `/me/payslips/{line_id}/payslip.pdf` | ✅ Done |
| `/admin/compensation/import` | ✅ Done |
| `/admin/deduction-types` | ✅ Done |
| `/admin/payroll/{run_id}/void` | ✅ Done |
| `/admin/payroll/{run_id}/lines/{line_id}` | ✅ Done |
| `/me/payslips` | ✅ Done |
| `/me/payslips/{line_id}` | ✅ Done |
| `/admin/payroll/{run_id}/lines/{line_id}/payslip` | ✅ Done |

---

## Operational decisions (locked)

| Topic | Choice |
|-------|--------|
| Who runs payroll | **Admin** (same role as today — no separate finance user) |
| Bank / journal export | **In-app CSV** on finalized runs; accounting still reviews before upload |
| 13th month | **External** — keep in spreadsheet / accounting tool |

---

## Relationship to attendance reports

`/admin/reports` = hours & leave sanity check before close  
`/admin/payroll` = money after close (Phase 2+)

Attendance exports remain the pre-payroll review; payroll runs consume the same underlying data.