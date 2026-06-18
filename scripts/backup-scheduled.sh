#!/usr/bin/env bash
set -euo pipefail

BACKUP_DIR="${BACKUP_DIR:-/backups}"
RETENTION_DAYS="${RETENTION_DAYS:-${BACKUP_RETENTION_DAYS:-14}}"
PGHOST="${PGHOST:-db}"
PGPORT="${PGPORT:-5432}"
PGUSER="${PGUSER:-dtr}"
PGDATABASE="${PGDATABASE:-dtr}"
UPLOAD_DIR="${UPLOAD_DIR:-/data/uploads}"
BACKUP_STATUS_FILE="${BACKUP_STATUS_FILE:-${BACKUP_DIR}/last-backup.status}"
TIMESTAMP="$(date +%Y%m%d_%H%M%S)"
DB_OUTPUT="${BACKUP_DIR}/dtr-${TIMESTAMP}.sql"
UPLOADS_ARCHIVE="${BACKUP_DIR}/uploads-${TIMESTAMP}.tar.gz"

write_status() {
  printf '%s %s\n' "$(date -Iseconds)" "$1" >"$BACKUP_STATUS_FILE"
}

notify_failure() {
  local message="$1"
  echo "[$(date -Iseconds)] BACKUP FAILED: $message" >&2
  write_status "failed"
  if [[ -n "${BACKUP_WEBHOOK_URL:-}" ]]; then
    curl -sf -X POST -H "Content-Type: text/plain" \
      --data-binary "$message" \
      "$BACKUP_WEBHOOK_URL" >/dev/null 2>&1 || true
  fi
}

on_error() {
  notify_failure "scheduled backup exited with error (see logs above)"
}

trap on_error ERR

mkdir -p "$BACKUP_DIR"

: "${PGPASSWORD:?PGPASSWORD must be set}"

pg_dump -h "$PGHOST" -p "$PGPORT" -U "$PGUSER" "$PGDATABASE" >"$DB_OUTPUT"
echo "[$(date -Iseconds)] Database backup written to $DB_OUTPUT"

if [[ -d "$UPLOAD_DIR" ]] && [[ -n "$(ls -A "$UPLOAD_DIR" 2>/dev/null || true)" ]]; then
  tar -czf "$UPLOADS_ARCHIVE" -C "$(dirname "$UPLOAD_DIR")" "$(basename "$UPLOAD_DIR")"
  echo "[$(date -Iseconds)] Uploads archive written to $UPLOADS_ARCHIVE"
else
  echo "[$(date -Iseconds)] No uploads directory to archive (skipped)."
fi

if [[ "$RETENTION_DAYS" =~ ^[0-9]+$ ]] && [[ "$RETENTION_DAYS" -gt 0 ]]; then
  find "$BACKUP_DIR" -maxdepth 1 -name 'dtr-*.sql' -mtime +"$RETENTION_DAYS" -delete
  find "$BACKUP_DIR" -maxdepth 1 -name 'uploads-*.tar.gz' -mtime +"$RETENTION_DAYS" -delete
fi

write_status "ok"
echo "[$(date -Iseconds)] Backup completed successfully"