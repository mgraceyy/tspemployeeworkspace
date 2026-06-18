# TalaSora Prime DTR

[![CI](https://github.com/mgraceyy/tspemployeeworkspace/actions/workflows/ci.yml/badge.svg)](https://github.com/mgraceyy/tspemployeeworkspace/actions/workflows/ci.yml)

Employee timekeeping (Daily Time Record) and lightweight employee workspace for **TalaSora Prime** — built with Rust, Axum, PostgreSQL, and MiniJinja.

> **Repository:** [github.com/mgraceyy/tspemployeeworkspace](https://github.com/mgraceyy/tspemployeeworkspace)  
> **License:** MIT · **Version:** v0.3.0

## Features

### Timekeeping (core)

- **Employees** — clock in/out, personal timesheet, voluntary PIN change
- **Managers** — team dashboard, timesheets, time corrections, no-show marking, OT approval
- **Admins** — employee management, shift schedules, company settings, payroll reports (CSV/Excel), compensation import, payroll runs, deduction types

### Employee workspace

- **Profiles** — employees edit contact number, personal email, and profile photo; admins manage full work profile (department, job title, date hired, bank/TIN/SSS/PhilHealth, etc.)
- **PIN reset** — employees request a reset; managers/admins approve with a temporary PIN (must change on next login)
- **Requirements** — admin-defined checklist types with optional expiry and file uploads (PDF, images, Word); employees submit, admins approve/reject; expired items can be re-submitted
- **EOD (End of Day)** — required on days the employee clocks in; department-scoped Team EOD feed; submit lock with admin unlock for corrections
- **EOD history** — employees browse past submitted reports
- **Weekly EOD export** — managers/admins download a 7-day CSV

### Production readiness

- PostgreSQL-backed sessions (survive restarts, 12h idle timeout)
- CSRF protection on all form POSTs
- Login, POST, and change-PIN rate limiting
- Honest session expiry messaging (12h server idle timeout; no client activity tracking)
- Admin audit log for sensitive actions
- Secure session cookies (`APP_ENV=production`)
- Reverse proxy examples (Caddy, nginx), health check (`/health`), Prometheus metrics (`/metrics`), Docker Compose stack, CI, backup/restore scripts

## Prerequisites

- [Rust](https://rustup.rs/) 1.75+ (for local development)
- [Docker](https://www.docker.com/) (for containerized setup)
- [Node.js](https://nodejs.org/) 22+ (optional, for Playwright E2E tests)

## Quick start (Docker)

1. Copy the example environment file:

   ```bash
   cp .env.example .env
   ```

2. Edit `.env` and set a strong `SESSION_SECRET` (at least 64 characters). Docker Compose **requires** this variable — the app will not start without it.

3. For a fresh database in development, set `SEED_DEFAULT_ADMIN=true` in `.env` to create a default admin on first run.

4. Start the stack:

   ```bash
   docker compose up --build
   ```

5. Open [http://localhost:8080/login](http://localhost:8080/login).

   When seeding is enabled, sign in with `ADMIN` / `1234` and change your PIN when prompted.

## Local development

1. Start PostgreSQL:

   ```bash
   docker compose up db -d
   ```

2. Copy and configure environment:

   ```bash
   cp .env.example .env
   ```

3. Run the app:

   ```bash
   cargo run
   ```

4. Open [http://localhost:8080](http://localhost:8080).

## Environment variables

| Variable | Required | Description |
|----------|----------|-------------|
| `DATABASE_URL` | Yes | PostgreSQL connection string |
| `SESSION_SECRET` | Yes | Signing key for session cookies (64+ characters) |
| `SEED_DEFAULT_ADMIN` | No | Create `ADMIN` / `1234` when no employees exist (dev only) |
| `SEED_E2E_FIXTURES` | No | Create E2E test users (`E2E001`, `E2MGR`) when missing — CI/E2E only |
| `APP_ENV` | No | Set to `production` to enable secure session cookies |
| `SESSION_SECURE_COOKIES` | No | Override cookie security (`true` / `false`) |
| `TRUST_PROXY_HEADERS` | No | Use `X-Forwarded-For` / `X-Real-IP` for POST/login rate limiting behind a proxy |
| `SHARED_RATE_LIMITS` | No | Store login/POST rate limits in PostgreSQL (set `true` when running multiple app replicas) |
| `METRICS_TOKEN` | No | Require `Authorization: Bearer …` or `?token=` to scrape `/metrics` |
| `BIND_ADDR` | No | Listen address (default: `0.0.0.0`) |
| `PORT` | No | Listen port (default: `8080`) |
| `DATABASE_MAX_CONNECTIONS` | No | SQLx pool size per app process (default: `5`) |
| `UPLOAD_DIR` | No | Directory for requirement file uploads (default: `./uploads`) |
| `MAX_UPLOAD_BYTES` | No | Max upload size in bytes (default: 10 MB) |
| `RUST_LOG` | No | Log filter (default: `dtr=debug,tower_http=info,sqlx=warn`) |
| `LOG_FORMAT` | No | Set to `json` for structured JSON logs (production aggregation) |

## Health check

```
GET /health
```

Returns `200` when the database is reachable:

```json
{ "status": "ok", "database": "ok" }
```

Returns `503` if the database is unavailable.

### Metrics

```
GET /metrics
```

Prometheus-style text exposition:

- `dtr_http_requests_total` — all routed requests (including `/health`, `/static/`, `/metrics`)
- `dtr_http_errors_total` — HTTP 5xx responses
- `dtr_http_request_duration_seconds` — request latency histogram (sum, count, buckets)

When `METRICS_TOKEN` is set, pass `Authorization: Bearer <token>` or `?token=<token>`.

## User roles

| Role | Access |
|------|--------|
| Employee | Clock, timesheet, profile (limited), requirements, EOD, team EOD feed |
| Manager | Team oversight, corrections, OT review, team EOD status and export |
| Admin | Full employee/profile/requirement management, payroll reports, audit log, EOD unlock |

## Key routes

| Area | Routes |
|------|--------|
| Public | `/login`, `/change-pin`, `/logout`, `/health`, `/metrics` |
| Employee | `/`, `/clock/in`, `/clock/out`, `/me/timesheet`, `/me/timesheet/export.csv`, `/me/leave`, `/me/holidays`, `/me/profile`, `/me/requirements`, `/me/eod`, `/me/eod/history`, `/me/payslips`, `/me/team/eod`, `/notifications` |
| Manager | `/manager`, `/manager/team`, `/manager/team/{id}/export.csv`, `/manager/correct`, `/manager/absence`, `/manager/ot/{id}/review`, `/manager/pin-resets`, `/manager/eod`, `/manager/eod/export.csv`, `/manager/leave`, `/manager/requirements` |
| Admin | `/admin/employees`, `/admin/employees/{id}/compensation`, `/admin/compensation/import`, `/admin/deduction-types`, `/admin/payroll` (CSV/bank/journal/PDF exports), `/admin/requirements`, `/admin/shifts`, `/admin/settings`, `/admin/holidays`, `/admin/reports` (CSV/Excel, close/reopen period), `/admin/corrections`, `/admin/audit`, `/admin/eod` |

## Admin onboarding tips

1. **Set departments** — use bulk assign on the Employees page so Team EOD works.
2. **Define requirement types** — checklist items are auto-assigned to all active employees.
3. **Complete profiles** — the employee list shows requirements progress and profile completeness %.

## Project layout

```
src/           Rust application code
migrations/    SQL schema migrations
templates/     MiniJinja HTML templates
static/        CSS and static assets
scripts/       Database and uploads backup/restore scripts
e2e/           Playwright browser tests
docs/          Reverse proxy examples, payroll roadmap (PAYROLL.md), Prometheus scrape + alert examples
```

## Payroll roadmap

**v0.3.0** adds transport/meal allowances, compensation CSV import, per-employee deduction defaults, bank upload + journal CSV exports, PDF payslips, attendance snapshot staleness warnings, profile photos, PIN reset, and employee archive filtering — on top of **v0.2.0** compensation, payroll runs, HTML payslips, and CSV export, and **v0.1.0** time & attendance reporting. See [docs/PAYROLL.md](docs/PAYROLL.md).

**Ops (locked):** Admin runs payroll in-app; 13th-month accrual stays outside the app for now. Bank upload and journal CSV exports are available for finalized runs.

## Security headers

The app sets `Content-Security-Policy`, `X-Content-Type-Options`, `X-Frame-Options`, `Referrer-Policy`, and `Permissions-Policy` on HTML responses. Static assets under `/static/` are served with `Cache-Control: public, max-age=86400`. CSP allows inline styles (templates) but blocks scripts — navigation uses a CSS-only mobile menu toggle.

Authenticated sessions are **revalidated against the database** on each request: deactivated accounts, PIN resets, and role changes take effect immediately (stale cookies are cleared or redirected to `/change-pin`).

## Production notes

- Set `SEED_DEFAULT_ADMIN=false` (or unset) in production — the app refuses to start if `SEED_DEFAULT_ADMIN=true` with `APP_ENV=production`.
- Never enable `SEED_E2E_FIXTURES` in production.
- Use a strong, unique `SESSION_SECRET` (64+ characters).
- Put the app behind HTTPS and set `APP_ENV=production` or `SESSION_SECURE_COOKIES=true`.
- When using a reverse proxy, set `TRUST_PROXY_HEADERS=true` so POST/login rate limiting uses the real client IP. The proxy must **overwrite** `X-Real-IP` and `X-Forwarded-For` with the client address (`$remote_addr` in nginx, `{remote_host}` in Caddy) — do not forward client-supplied `X-Forwarded-For` chains. See `docs/nginx.conf.example` and `docs/Caddyfile`.
- **Metrics:** `/metrics` exposes HTTP counters, DB pool gauges, and payroll run counters (`dtr_payroll_runs_*`). Use `docs/prometheus.yml.example` and `docs/alerts.yml.example` with your Prometheus stack. Set `METRICS_TOKEN` in production.
- **Backups:** the `ops` profile writes `last-backup.status` under `./backups/` and can POST to `BACKUP_WEBHOOK_URL` on failure.
- **Production Compose:** copy `.env.prod.example` to `.env`, set strong secrets, then:
  ```bash
  docker compose -f docker-compose.prod.yml --profile proxy up -d
  ```
  The app is not published on the host; only Caddy exposes ports 80/443. PostgreSQL is internal-only (no host port).
- Sessions expire after **12 hours without server activity** (each page load or form submit extends the session). The UI explains this honestly — the app does not monitor mouse or keyboard activity.
- **Company timezone** is configured in Admin → Settings (`company_settings.timezone`, IANA name such as `Asia/Manila`). Clock events, pay periods, EOD due dates, and reports use this timezone.
- **Pay period close** freezes all payroll-relevant changes for dates in a closed range (clock in/out, corrections, absences, leave approval, OT review, EOD edits). Overlapping close ranges are rejected — reopen existing closes first. Reopen from Admin → Reports when adjustments are needed.
- Login and POST rate limiters are **in-memory per app process** by default. Set `SHARED_RATE_LIMITS=true` when running **multiple app replicas** so limits are stored in PostgreSQL (`rate_limit_events` table). Expired rows are pruned by a background task (not on every request).
- **Scaling replicas:** `docker compose -f docker-compose.prod.yml up -d --scale app=3` with `SHARED_RATE_LIMITS=true`. Sessions are in PostgreSQL (no sticky sessions required). Set `DATABASE_MAX_CONNECTIONS` so total pool usage (`replicas × connections`) stays within PostgreSQL `max_connections`.
- All HTML forms include a CSRF token; POSTs without a valid token are rejected.
- POST requests are limited to **120 per minute per IP**.
- Login is limited to **5 failed attempts per account** and **20 failed attempts per IP** within 15 minutes (shared across replicas when `SHARED_RATE_LIMITS=true`).
- Voluntary PIN change is limited to **5 failed attempts per account** and **15 failed attempts per IP** within 15 minutes.
- Requirement uploads are stored on disk (`UPLOAD_DIR`); Docker Compose mounts a persistent `uploads` volume. Back up this directory with your database.
- Back up the PostgreSQL database regularly (see below) and test restores.
- Review the **Admin Audit Log** (`/admin/audit`) for settings changes, employee lifecycle actions, OT reviews, and EOD unlocks.

## Backups and disaster recovery

Back up **both** the PostgreSQL database and the `uploads` volume — requirement files live on disk and are not included in a SQL dump alone.

### One-off full backup (database + uploads)

With Docker Compose running:

```bash
./scripts/backup-all.sh
```

This writes `backups/dtr-YYYYMMDD_HHMMSS.sql` and `backups/uploads-YYYYMMDD_HHMMSS.tar.gz`.

Database-only shortcuts:

```bash
./scripts/backup-db.sh
```

On Windows (PowerShell):

```powershell
.\scripts\backup-db.ps1
```

Without Docker, set `DATABASE_URL` and run `pg_dump` directly, or use the shell script which falls back to `pg_dump "$DATABASE_URL"`.

Uploads backup:

```bash
./scripts/backup-uploads.sh
```

On Windows:

```powershell
.\scripts\backup-uploads.ps1
```

### Scheduled backups in Compose

The `backup` service (ops profile) runs daily and archives **both** the database and uploads volume:

```bash
docker compose --profile ops up -d backup
```

Outputs land in `./backups/` on the host.

Store backups off the server and test restores periodically. The Compose volumes `pgdata` and `uploads` hold live data; backups are your recovery path if those volumes are lost.

### Restore

Database restore (destructive — replaces the `dtr` database):

```bash
./scripts/restore-db.sh backups/dtr-20260617_120000.sql
```

Pass `--yes` to skip the confirmation prompt (automation/CI):

```bash
./scripts/restore-db.sh backups/dtr-20260617_120000.sql --yes
```

Uploads restore (destructive — replaces files in `UPLOAD_DIR`):

```bash
./scripts/restore-uploads.sh backups/uploads-20260617_120000.tar.gz
```

With Docker Compose, restore the database first, then uploads, then restart the app:

```bash
docker compose stop app
./scripts/restore-db.sh backups/dtr-20260617_120000.sql
UPLOAD_DIR=./uploads ./scripts/restore-uploads.sh backups/uploads-20260617_120000.tar.gz
docker compose up -d app
```

Schedule daily backups with cron — see `scripts/backup-cron.example`.

After each backup, verify the archive when possible:

```bash
./scripts/verify-backup.sh backups/dtr-20260617_120000.sql
```

### Operations runbook

| Task | Steps |
|------|--------|
| **Restore drill** | Stop app → `restore-db.sh` → `restore-uploads.sh` → start app → log in and spot-check requirements + reports |
| **Rotate `SESSION_SECRET`** | Schedule maintenance, deploy new secret, all users re-login (existing signed cookies invalidate) |
| **Scale out** | Set `SHARED_RATE_LIMITS=true`, tune `DATABASE_MAX_CONNECTIONS`, `docker compose up --scale app=N` |
| **Incident: compromised admin** | Deactivate account (immediate session invalidation), reset PIN, review `/admin/audit` |

On Windows:

```powershell
.\scripts\restore-db.ps1 backups\dtr-20260617_120000.sql -Yes
.\scripts\restore-uploads.ps1 backups\uploads-20260617_120000.tar.gz -Yes
```

## CI

GitHub Actions (`.github/workflows/ci.yml`) runs on every push and pull request:

| Job | What it checks |
|-----|----------------|
| **test** | `cargo fmt`, Clippy (`-D warnings`), `cargo audit`, `cargo deny`, `cargo build`, `cargo test` with `RUST_TEST_THREADS=1` and a matrix of `SHARED_RATE_LIMITS=false/true` against PostgreSQL 16 |
| **docker** | Builds the image; smoke-tests `/health`, `/login`, `/static/style.css`, and `/metrics` |
| **e2e** | Runs Playwright against the **Docker image** (same artifact path as the docker job), matrixed with `SHARED_RATE_LIMITS=false/true` |
| **release** | On version tags (`v*`), runs tests, builds a release binary and Docker image, and attaches `dtr-<tag>-docker-image.tar.gz` to the GitHub Release |
| **verify-backup** | Migrates via `dtr-migrate`, backs up and verifies SQL, runs a **database restore drill**, an **uploads restore drill**, then a **full DR drill** (start app on restored DB + uploads, curl `/health`) |

The audit/deny steps ignore [RUSTSEC-2023-0071](https://rustsec.org/advisories/RUSTSEC-2023-0071) (`rsa` via `sqlx`) — no upstream fix is available; see `.cargo/audit.toml` and `deny.toml`.

## Tests

The suite has **175+ Rust tests** across unit, integration, and HTTP layers.

Unit tests (no database required):

```bash
cargo test --lib
```

Integration tests use `DATABASE_URL` and skip automatically when Postgres is unavailable:

```bash
cargo test --test integration
```

HTTP integration tests (Axum router, no TCP bind) live in `tests/http.rs`, `http_extra.rs`, `http_workflows.rs`, `http_coverage.rs`, and `http_payroll.rs`:

```bash
cargo test --test http
cargo test --test http_extra
cargo test --test http_workflows
cargo test --test http_coverage
cargo test --test http_payroll
```

Run all tests with a local database:

```bash
docker compose up db -d
cargo test
```

Before committing:

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test
```

### Browser E2E (Playwright)

CI starts the app with `SEED_DEFAULT_ADMIN=true` and `SEED_E2E_FIXTURES=true` so fixtures match local parity. For local runs, enable the same flags in `.env` (or export them) so `E2E001` / `E2MGR` exist.

With the app running locally (`cargo run` or Docker) and `DATABASE_URL` set:

```bash
cd e2e
npm ci
npx playwright install chromium
E2E_BASE_URL=http://127.0.0.1:8080 npm test
```

E2E specs cover login, health/metrics, auth flows, manager actions, requirements upload, admin employee creation, authorization boundaries, closed pay-period blocking, and the payroll happy path (close period → draft → finalize → payslips).

## Contributing

### Git commit identity

Use your GitHub email so commits link to your profile (Settings → Emails → `id+username@users.noreply.github.com`):

```bash
git config user.name "Grace"
git config user.email "221118937+mgraceyy@users.noreply.github.com"
```

### Releases

Tag production baselines after CI is green:

```bash
git tag -a v0.3.0 -m "Payroll pack: allowances, import, exports, PDF payslips; foundation: photo, PIN reset, archive"
git push origin v0.3.0
```

### Suggested GitHub repository settings

On [github.com/mgraceyy/tspemployeeworkspace](https://github.com/mgraceyy/tspemployeeworkspace/settings):

- **Description:** `TalaSora Prime employee timekeeping & workspace (Rust/Axum)`
- **Topics:** `rust`, `axum`, `postgresql`, `timekeeping`, `dtr`, `hr`, `playwright`
- **Visibility:** Private recommended for internal company deployment (code has no secrets, but ops details are exposed)