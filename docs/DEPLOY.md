# Production deployment guide

TalaSora Prime DTR **v0.3.1** — timekeeping, leave, compensation, and payroll for a single company.

Migrations run **automatically on app startup** (`sqlx::migrate!` in `src/lib.rs`). You do not need a separate migrate step unless you prefer `cargo run --bin dtr-migrate` before starting the app.

---

## Pre-flight checklist

| Item | Action |
|------|--------|
| Secrets | Generate unique `SESSION_SECRET` (64+ chars) and `METRICS_TOKEN`; strong `POSTGRES_PASSWORD` |
| Seeding | `SEED_DEFAULT_ADMIN=false`, `SEED_E2E_FIXTURES` unset in production |
| HTTPS | `APP_ENV=production` or `SESSION_SECURE_COOKIES=true` behind TLS |
| Proxy | `TRUST_PROXY_HEADERS=true` only when **all** traffic passes through Caddy/nginx that **overwrites** client IP headers |
| Replicas | `SHARED_RATE_LIMITS=true` when running more than one app container |
| Pool size | `DATABASE_MAX_CONNECTIONS` × replica count &lt; PostgreSQL `max_connections` (default pool: **15** per process) |
| Backups | Enable `ops` profile or cron; test restore at least once |
| Uploads | Persistent volume for `UPLOAD_DIR` (requirement files are not in SQL dumps) |
| Payroll | Review gov deduction rates with your accountant before enabling auto-deductions ([PAYROLL.md](PAYROLL.md)) |

---

## First-time production deploy (Docker Compose)

1. Copy production env:

   ```bash
   cp .env.prod.example .env
   ```

2. Edit `.env` — set `POSTGRES_PASSWORD`, `SESSION_SECRET`, `METRICS_TOKEN`; keep `SEED_DEFAULT_ADMIN=false`.

3. Start stack with reverse proxy:

   ```bash
   docker compose -f docker-compose.prod.yml --profile proxy up -d --build
   ```

4. Optional — daily backups:

   ```bash
   docker compose -f docker-compose.prod.yml --profile ops up -d backup
   ```

5. Create the first admin (no seed in prod):

   - Temporarily set `SEED_DEFAULT_ADMIN=true`, restart app once, sign in as `ADMIN` / `1234`, change PIN, set `SEED_DEFAULT_ADMIN=false`, restart again; **or**
   - Insert an admin via SQL / internal script (not shipped).

6. Post-deploy smoke test (see below).

---

## Upgrade existing deployment

1. **Back up** database and uploads:

   ```bash
   ./scripts/backup-all.sh
   ```

2. Pull new code / image and rebuild:

   ```bash
   docker compose -f docker-compose.prod.yml --profile proxy up -d --build
   ```

3. Migrations **001–030** apply on next app start. Current schema additions through **030**:

   | Migration | Purpose |
   |-----------|---------|
   | 025 | Government auto-deduction toggles |
   | 026 | Labor premiums |
   | 027 | Leave balances |
   | 028 | Leave request indexes |
   | 029 | Leave overlap exclusion (`btree_gist`) |
   | 030 | `date_separated`, LWOP attendance, LWOP deduction type, proration columns on payroll lines |

4. Follow [PAYROLL.md — Upgrade from pre-025](PAYROLL.md#upgrade-from-pre-025-gov-deductions-premiums-leave-proration-lwop) for Settings, leave balances, and draft payroll runs.

5. Run smoke test.

---

## Post-deploy smoke test

| Step | Expected |
|------|----------|
| `curl -sf https://your-host/health` | `{"status":"ok","database":"ok"}` |
| Login as admin | Redirect to home; no seed warnings in logs |
| `/admin/settings` | Save pay period / timezone |
| Create employee + compensation | Profile + salary effective date |
| Employee clock in/out | Time entry on today’s work date |
| Manager OT / leave | Approve flow works |
| Close pay period | `/admin/reports` → close canonical range |
| Draft payroll | `/admin/payroll` → create draft; lines populated |
| Recalculate auto deductions | Gov + LWOP rows refresh on draft (if applicable) |
| Finalize (test period) | Payslip + CSV/bank/journal exports |
| `/metrics` with token | Prometheus text; not public without token |

---

## Go-live payroll (HR)

1. **Settings** — pay period, timezone, optional gov deductions / labor premiums / leave defaults.
2. **Profiles** — `date_hired`, bank account, TIN/SSS/PhilHealth; set **`date_separated` (Last day)** when an employee leaves.
3. **Compensation** — every active employee has a rate effective on period end.
4. **Attendance** — close period only after OT approved and absences/LWOP marked.
5. **Draft run** — review proration notes `(employed_days/period_days)` and LWOP deductions.
6. **Finalize** — void and recreate if attendance snapshot is stale.
7. **Exports** — bank CSV (employees need bank account), journal CSV for accounting.

---

## Rollback

- **App only:** deploy previous image/tag; schema is forward-only — do not downgrade DB without restore.
- **Bad migration / data:** stop app → `restore-db.sh` + `restore-uploads.sh` from last good backup → start app.

---

## Manual deploy (no Compose)

```bash
cargo build --release
export DATABASE_URL=postgres://...
export SESSION_SECRET=...
export APP_ENV=production
./target/release/dtr
```

Ensure `migrations/`, `templates/`, and `static/` sit next to the binary (Docker layout) or run from repo root.

---

## Release tag (after CI green)

```bash
git tag -a v0.3.1 -m "Payroll: proration, LWOP, leave overlap, deployment docs"
git push origin v0.3.1
```

GitHub Actions `release` job attaches a Docker image tarball to the tag.