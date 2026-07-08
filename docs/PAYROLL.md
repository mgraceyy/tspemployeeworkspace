# Payroll roadmap

TalaSora Prime DTR payroll policy and implementation plan. Updated for **v0.3.1** — government auto-deductions, labor premiums, leave balances, hire/last-day proration, and LWOP.

## Locked policy (TalaSora Prime)

| Decision | Choice |
|----------|--------|
| Pay type | **All monthly** — no hourly/daily employees |
| Overtime | **132%** of hourly equivalent (configurable per employee, default 132) |
| No-shows | **Reduce pay** — one daily rate deducted per no-show day |
| Sick / vacation / official / offset leave | **Informational in payroll reports** — no gross pay adjustment; vacation and sick also have **balance tracking** for requests |
| LWOP (leave without pay) | **Payroll deduction** — daily rate × LWOP days; auto-applied on draft runs (`LWOP` deduction type, note `Auto-calculated`) |
| Hire date / last day | **Prorate period base + allowances** by calendar days employed in the closed pay period (`date_hired` / `date_separated` on employee profile) |
| Government deductions (SSS, PhilHealth, Pag-IBIG, WHT) | **Optional auto-calculation** per company toggle in Admin → Settings; manual rows always win when note ≠ `Auto-calculated` |
| Other deductions (loans, etc.) | **Manual entry** per employee on draft payroll runs (plus per-employee defaults on draft creation) |
| Labor premiums (rest day, holiday, night diff) | **Optional** per company toggle in Admin → Settings; additive on top of base + ordinary OT |

### Gross pay formula (monthly employees)

Uses **26 working days / month** and **8 hours / day** (480 minutes) for rate derivation:

```
daily_rate     = monthly_salary ÷ 26
hourly_rate    = daily_rate ÷ 8

period_base    = monthly_salary × period_factor
                 (semimonthly = ½, monthly = 1, weekly/biweekly per calendar)
                 × (employed_calendar_days ÷ period_calendar_days) when hire/last day falls in period

no_show_deduction = daily_rate × no_show_days
lwop_deduction    = daily_rate × lwop_days   (payroll deduction row, not gross reduction)

ot_pay         = (approved_ot_minutes ÷ 60) × hourly_rate × (ot_rate_percent ÷ 100)

premium_pay    = rest-day + holiday + night-diff premiums (when enabled; see Phase 5)

allowance_pay  = monthly_allowances × period_factor
                 (same proration factor as period_base when hire/last day applies)

gross_pay      = period_base + allowance_pay − no_show_deduction + ot_pay + premium_pay
```

Leave day counts in payroll reports do **not** change `gross_pay`. Approved vacation/sick leave is reflected in attendance status and reports; balances are enforced when employees **request** leave (see Leave balances below).

Implementation: `src/services/payroll/compute.rs` (base/OT), `src/services/payroll/labor_premiums.rs` (premiums).

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
| Deductions → net pay | Done |
| Payslips (employee + printable) | Done |

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
| Attendance snapshot hash + stale-draft warning (finalize blocked when stale) | Done |

### v0.3.0 — Foundation ✅

| Area | Status |
|------|--------|
| Profile photo upload | Done |
| Bank account, TIN, SSS, PhilHealth on profile | Done |
| PIN reset request workflow | Done |
| Employee archive filter on admin list | Done |
| Logout everywhere + session version invalidation | Done |

### v0.3.0+ — Government auto-deductions ✅

| Area | Status |
|------|--------|
| Per-company toggles: SSS, PhilHealth, Pag-IBIG, withholding tax | Done — Admin → Settings |
| Auto-apply on draft run creation | Done |
| Recalculate on draft run (`/admin/payroll/{run_id}/recalculate-government-deductions`) | Done — refreshes government **and LWOP** auto-deductions |
| Manual override preserved (non–auto-calculated rows not overwritten) | Done |
| Preflight checks before finalize (deductions vs gross) | Done |

**Schema:** `migrations/025_government_auto_deductions.sql`  
**Implementation:** `src/services/payroll/gov_deductions.rs`, `src/services/payroll/deductions_auto.rs`

Rates follow common 2025–2026 Philippine employer practice (MSC brackets, 5% SSS employee share, 2.5% PhilHealth, Pag-IBIG tiers, TRAIN withholding). **Review with your accountant before production payroll.**

### v0.3.0+ — PH labor premiums ✅

| Area | Status |
|------|--------|
| Rest-day premium (+30% regular, +37% OT extra) | Done — toggle in Settings |
| Holiday premium (+100% regular, +128% OT extra) | Done — toggle in Settings |
| Night differential (+10% for 22:00–06:00 in company TZ) | Done — toggle in Settings |
| `premium_pay_cents` on payroll lines + payslips | Done |
| Computed from `time_entries` when draft run is created | Done |

**Schema:** `migrations/026_labor_premiums.sql`  
**Implementation:** `src/services/payroll/labor_premiums.rs`

Ordinary OT at `ot_rate_percent` (default 132%) stays in `ot_pay_cents`; premiums are **additive** in `premium_pay_cents`.

- **Rest day** = calendar day with no shift template for that `day_of_week`
- **Holiday** = date in `company_holidays`

### v0.3.0+ — Leave balances ✅

| Area | Status |
|------|--------|
| Default vacation/sick days in Admin → Settings | Done |
| Per-employee balances (vacation + sick only) | Done — seeded on employee create; editable on admin profile |
| Balance check on leave request submit (includes pending reservations) | Done |
| Atomic approve: balance deduct + attendance marks in one transaction | Done |
| Refund balance + clear attendance when approved leave is cancelled (open period) | Done |
| Deduct on manager approve | Done |
| Official leave and offset — not balance-tracked | By design |

**Schema:** `migrations/027_leave_balances.sql`, `029_leave_overlap_exclusion.sql`  
**Implementation:** `src/services/leave_balances.rs`, `src/services/leave.rs`

### v0.3.1 — Proration & LWOP ✅

| Area | Status |
|------|--------|
| Hire / last-day proration on base + allowances | Done — `date_hired` / `date_separated` on admin profile |
| LWOP attendance status + manager marking | Done |
| LWOP auto-deduction on draft runs | Done — `LWOP` deduction type, note `Auto-calculated` |
| Payslip + payroll CSV show LWOP / proration | Done |
| Preflight warnings for stale/missing LWOP | Done |
| Recalculate auto deductions includes LWOP | Done |

**Schema:** `migrations/030_payroll_proration_lwop.sql`  
**Implementation:** `src/services/payroll/compute.rs`, `src/services/payroll/deductions_auto.rs`

---

## Design principle

```
Time entries + approvals
        ↓
  Close pay period
        ↓
  Payroll run (compensation + compute.rs + optional labor_premiums.rs)
        ↓
  Auto government deductions (if enabled) + manual deductions → net pay → payslips
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

### Schema (implemented — `migrations/019_payroll_runs.sql`, `020_payroll_deductions.sql`, `021_payroll_integrity.sql`, `026_labor_premiums.sql`)

Key `payroll_lines` amounts (simplified):

```sql
-- base_pay_cents, allowance_cents, no_show_deduction_cents,
-- ot_pay_cents, premium_pay_cents, gross_pay_cents, net_pay_cents
```

### Workflow

1. `/admin/payroll` — list runs
2. Create draft for a fully **closed** canonical pay period
3. Pull `payroll_summary()` per employee + compensation as of `period_end`
4. Compute gross (base, allowances, no-show, OT, optional premiums)
5. Apply enabled government auto-deductions + per-employee deduction defaults
6. Enter or adjust manual deductions per employee
7. Finalize → immutable snapshot (blocked while pending OT remains)
8. **Void draft** if the run was created in error

### Guards

- Period must be closed and match a full configured pay period (semimonthly, monthly, etc.)
- Every active employee needs compensation effective on `period_end` (current profile or history row)
- Pending OT blocks finalize (excluded from gross until approved)
- **Stale attendance blocks finalize** — void the draft, fix attendance, close the period again, and create a new draft if timesheets changed after the snapshot
- Period reopen blocked while a draft or finalized payroll run exists
- Draft creation is transactional; government auto-deductions run after commit and **void the draft** if they fail; one active run per period (DB-enforced)

---

## Phase 3 — Deductions ✅

**Schema:** `migrations/020_payroll_deductions.sql`, `025_government_auto_deductions.sql`

- `deduction_types` — seeded: SSS, PhilHealth, Pag-IBIG, withholding tax, loan, other
- `payroll_deductions` — amounts per employee line on a draft run

```
net_pay = gross_pay − sum(deductions)
```

**Government auto-deductions:** When a toggle is on in Settings, draft runs insert or refresh rows with codes `SSS`, `PHIC`, `HDMF`, `WHT` and note `Auto-calculated`. Rows edited manually (any other note) are never overwritten. Use **Recalculate government deductions** on the payroll run page to refresh auto rows after settings or compensation changes.

**Admin UI:** Payroll run → **Deductions** per employee (`/admin/payroll/{run_id}/lines/{line_id}`). Edits blocked after finalize. Total deductions cannot exceed gross pay.

---

## Phase 4 — Payslips ✅

- `/admin/payroll/{run_id}` — run summary with **Payslip** link per employee (finalized runs)
- `/admin/payroll/{run_id}/lines/{line_id}/payslip` — printable admin view
- `/me/payslips` — employee list (finalized runs only, own records)
- `/me/payslips/{line_id}` — printable HTML payslip (earnings, premiums when present, deductions, net)
- PDF variants at `…/payslip.pdf`
- Print via browser (Ctrl+P); nav hidden in print layout

---

## Phase 5 — PH labor premiums ✅

Rest-day, holiday, and night-differential premium pay — individually enable/disable in Admin → Settings.

| Premium | Regular minutes | OT minutes (extra on top of 132% OT) |
|---------|-----------------|--------------------------------------|
| Rest day | +30% hourly | +37% hourly |
| Holiday | +100% hourly | +128% hourly |
| Night diff | +10% hourly for 22:00–06:00 work (company timezone) | Same night minutes |

See **v0.3.0+ — PH labor premiums** above for schema and implementation paths.

---

## Phase 6 — Accounting handoff (partial ✅)

| Item | Status |
|------|--------|
| Bank upload CSV (`export-bank.csv`, uses `bank_account` on profile) | ✅ Done — omits employees with blank/missing bank account; payroll run page shows count |
| Journal entry CSV (`export-journal.csv`) | ✅ Done — salary expense + net payable GL accounts/labels configurable in `/admin/settings` |
| 13th-month accrual report | Deferred — external spreadsheet |

---

## Salary and hire-date policy (locked)

| Topic | Policy |
|-------|--------|
| Pay type | All employees are **monthly**; weekly/biweekly/semimonthly settings only change the **period factor** applied to monthly salary |
| New hires (`date_hired`) | **Prorate** period base and allowances by calendar days employed in the closed pay period |
| Last day (`date_separated`) | **Prorate** final period the same way; separated employees remain on payroll summary when `date_separated` falls in the period |
| Mid-period salary change | Rate effective on **`period_end`** only — no within-period proration; prior rates live in `compensation_history` |
| Government deductions | Still based on **full monthly salary** from compensation (not prorated gross) |
| Draft run lines | **Snapshot at creation** — gross pay does not auto-refresh; void the draft, reopen the period in Reports, fix data, close again, then create a new run. LWOP and government auto-deductions can be refreshed on draft runs via **Recalculate auto deductions**. |

---

## Routes

| Route | Status |
|-------|--------|
| `/admin/employees/{id}/compensation` | ✅ Done |
| `/admin/payroll` | ✅ Done |
| `/admin/payroll/{id}` | ✅ Done |
| `/admin/payroll/{id}/finalize` | ✅ Done |
| `/admin/payroll/{run_id}/recalculate-government-deductions` | ✅ Done (POST, draft only — refreshes government auto-deductions and LWOP) |
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
| Email notifications | **Deferred** — in-app notifications at `/notifications` for now |

---

## Relationship to attendance reports

`/admin/reports` = hours & leave sanity check before close  
`/admin/payroll` = money after close (Phase 2+)

Attendance exports remain the pre-payroll review; payroll runs consume the same underlying data.

---

## Upgrade from pre-025 (gov deductions, premiums, leave, proration, LWOP)

Use this checklist when moving an existing deployment to migrations **025**–**030**. All new Settings toggles default to **off**; leave defaults default to **15** vacation / **15** sick days.

### 1. Run migrations

```bash
cargo run --bin dtr-migrate
```

Docker Compose (app service or one-off):

```bash
docker compose run --rm app dtr-migrate
```

Applies:

| Migration | Adds |
|-----------|------|
| `025_government_auto_deductions.sql` | `auto_deduct_sss`, `auto_deduct_phic`, `auto_deduct_hdmf`, `auto_deduct_wht` on `company_settings` |
| `026_labor_premiums.sql` | Premium toggles on `company_settings`; `premium_pay_cents` on `payroll_lines` |
| `027_leave_balances.sql` | `default_vacation_days`, `default_sick_days`; `employee_leave_balances` table |
| `028_leave_request_indexes.sql` | Partial indexes on pending leave requests (performance) |
| `029_leave_overlap_exclusion.sql` | `btree_gist` exclusion constraint — no overlapping pending/approved leave per employee |
| `030_payroll_proration_lwop.sql` | `date_separated` on profiles; `lwop` attendance status; `LWOP` deduction type; `lwop_days` / `employed_days` / `period_calendar_days` on payroll lines |

### 2. Configure Admin → Settings

Open `/admin/settings` and set:

- **Government auto-deductions** — enable SSS, PhilHealth, Pag-IBIG, and/or withholding tax individually (review rates with your accountant first).
- **Labor law premiums** — enable rest-day, holiday, and/or night differential as needed.
- **Leave policy defaults** — starting vacation/sick balances for **new** employees (default 15 / 15 after migration).

### 3. Existing employees

- **Leave balances** — not bulk-seeded at migration time. Balances are created when an employee opens Leave, a manager reviews leave, or an admin opens the employee profile (uses current Settings defaults). For a one-time rollout, edit balances on **Admin → Employees → Profile** or have each employee visit `/me/leave` once after you set defaults.
- **Manager assignment** — ensure each employee has a `manager_id` if you use Team EOD, OT approval, or leave workflows (independent of this migration, but required for team visibility).

### 4. Existing draft payroll runs

| Feature | Action |
|---------|--------|
| Government auto-deductions | Open the draft run → **Recalculate auto deductions** (or POST `/admin/payroll/{run_id}/recalculate-government-deductions`). Only auto rows (note `Auto-calculated`) are refreshed; manual deduction amounts are kept. |
| LWOP deductions | Same **Recalculate auto deductions** action — refreshes `LWOP` rows from `lwop_days` on each line and current compensation. |
| Proration / hire & last day | Set `date_hired` / `date_separated` on employee profiles **before** creating a new draft; existing draft lines keep snapshot amounts until voided and recreated. |
| Labor premiums | `premium_pay_cents` on existing lines stays **0** until you **void** the draft, fix attendance if needed, close the period again, and **create a new draft** with premium toggles enabled. |
| Finalized runs | Unchanged — historical snapshots are not retrofitted. |

### 5. Smoke check

1. Settings saved with intended toggles.
2. Create a test **draft** payroll run for a closed period — confirm gross lines show `premium_pay_cents` when premiums are on, and deduction rows appear when gov toggles are on.
3. Submit a vacation leave request — confirm balance check and manager approve deducts days.

---

## Deferred / not planned

| Item | Notes |
|------|--------|
| 13th-month accrual | External spreadsheet |
| Separate finance role | Admin runs payroll in-app |
| Email / SMS alerts | In-app notifications sufficient for now |
| Prorate government deductions for partial periods | Gov deductions use full monthly salary today |