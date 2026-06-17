#!/usr/bin/env bash
set -euo pipefail

BACKUP_DIR="${BACKUP_DIR:-/backups}"
RETENTION_DAYS="${RETENTION_DAYS:-14}"
PGHOST="${PGHOST:-db}"
PGPORT="${PGPORT:-5432}"
PGUSER="${PGUSER:-dtr}"
PGDATABASE="${PGDATABASE:-dtr}"
UPLOAD_DIR="${UPLOAD_DIR:-/data/uploads}"
TIMESTAMP="$(date +%Y%m%d_%H%M%S)"
DB_OUTPUT="${BACKUP_DIR}/dtr-${TIMESTAMP}.sql"
UPLOADS_ARCHIVE="${BACKUP_DIR}/uploads-${TIMESTAMP}.tar.gz"

mkdir -p "$BACKUP_DIR"

: "${PGPASSWORD:?PGPASSWORD must be set}"

pg_dump -h "$PGHOST" -p "$PGPORT" -U "$PGUSER" "$PGDATABASE" > "$DB_OUTPUT"
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