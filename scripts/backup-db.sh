#!/usr/bin/env bash
set -euo pipefail

TIMESTAMP="$(date +%Y%m%d_%H%M%S)"
OUTPUT="${1:-backup-dtr-${TIMESTAMP}.sql}"

if docker compose ps db --status running >/dev/null 2>&1; then
  docker compose exec -T db pg_dump -U dtr dtr > "$OUTPUT"
else
  : "${DATABASE_URL:?DATABASE_URL must be set when Docker Compose db is not running}"
  pg_dump "$DATABASE_URL" > "$OUTPUT"
fi

echo "Backup written to $OUTPUT"