# TalaSora Prime DTR

Employee timekeeping (Daily Time Record) web app built with Rust, Axum, PostgreSQL, and MiniJinja.

## Features

- **Employees** — clock in/out, personal timesheet
- **Managers** — team dashboard, timesheets, time corrections, no-show marking, OT approval
- **Admins** — employee management, shift schedules, company settings, payroll reports (CSV/Excel)

## Prerequisites

- [Rust](https://rustup.rs/) 1.75+ (for local development)
- [Docker](https://www.docker.com/) (for containerized setup)

## Quick start (Docker)

1. Copy the example environment file:

   ```bash
   cp .env.example .env
   ```

2. Edit `.env` and set a strong `SESSION_SECRET` (at least 64 characters).

3. Start the stack:

   ```bash
   docker compose up --build
   ```

4. Open [http://localhost:8080/login](http://localhost:8080/login).

   On first run with `SEED_DEFAULT_ADMIN=true`, sign in with `ADMIN` / `1234` and change your PIN when prompted.

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
| `RUST_LOG` | No | Log filter (default: `dtr=debug,tower_http=info,sqlx=warn`) |

## Health check

```
GET /health
```

Returns `200` when the database is reachable:

```json
{ "status": "ok", "database": "ok" }
```

Returns `503` if the database is unavailable.

## User roles

| Role | Access |
|------|--------|
| Employee | Clock in/out, personal timesheet |
| Manager | Team oversight, corrections, OT review |
| Admin | Employees, shifts, settings, payroll reports |

## Project layout

```
src/           Rust application code
migrations/    SQL schema migrations
templates/     MiniJinja HTML templates
static/        CSS and static assets
```

## Production notes

- Set `SEED_DEFAULT_ADMIN=false` (or unset) in production.
- Use a strong, unique `SESSION_SECRET`.
- Put the app behind HTTPS; enable secure session cookies for production.
- Back up the PostgreSQL volume (`pgdata`) regularly.

## Tests

```bash
cargo test
```