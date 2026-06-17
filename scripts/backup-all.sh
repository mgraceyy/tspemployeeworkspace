#!/usr/bin/env bash
set -euo pipefail

TIMESTAMP="$(date +%Y%m%d_%H%M%S)"
BACKUP_DIR="${BACKUP_DIR:-./backups}"
DB_OUTPUT="${1:-${BACKUP_DIR}/dtr-${TIMESTAMP}.sql}"
UPLOADS_ARCHIVE="${BACKUP_DIR}/uploads-${TIMESTAMP}.tar.gz"
RETENTION_DAYS="${RETENTION_DAYS:-14}"

mkdir -p "$BACKUP_DIR"

if docker compose ps db --status running >/dev/null 2>&1; then
  docker compose exec -T db pg_dump -U dtr dtr > "$DB_OUTPUT"
else
  : "${DATABASE_URL:?DATABASE_URL must be set when Docker Compose db is not running}"
  pg_dump "$DATABASE_URL" > "$DB_OUTPUT"
fi

echo "Database backup written to $DB_OUTPUT"

UPLOAD_DIR="${UPLOAD_DIR:-./uploads}"
if [[ -d "$UPLOAD_DIR" ]] && [[ -n "$(ls -A "$UPLOAD_DIR" 2>/dev/null || true)" ]]; then
  tar -czf "$UPLOADS_ARCHIVE" -C "$(dirname "$UPLOAD_DIR")" "$(basename "$UPLOAD_DIR")"
  echo "Uploads archive written to $UPLOADS_ARCHIVE"
else
  echo "No uploads directory to archive (skipped)."
fi

if [[ "$RETENTION_DAYS" =~ ^[0-9]+$ ]] && [[ "$RETENTION_DAYS" -gt 0 ]]; then
  find "$BACKUP_DIR" -maxdepth 1 -name 'dtr-*.sql' -mtime +"$RETENTION_DAYS" -delete
  find "$BACKUP_DIR" -maxdepth 1 -name 'uploads-*.tar.gz' -mtime +"$RETENTION_DAYS" -delete
  echo "Pruned backups older than ${RETENTION_DAYS} days."
fi