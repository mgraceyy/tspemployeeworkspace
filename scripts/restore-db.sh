#!/usr/bin/env bash
set -euo pipefail

YES=false
BACKUP=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    -y|--yes)
      YES=true
      shift
      ;;
    *)
      if [[ -z "$BACKUP" ]]; then
        BACKUP="$1"
        shift
      else
        echo "Unexpected argument: $1" >&2
        exit 1
      fi
      ;;
  esac
done

if [[ -z "$BACKUP" ]]; then
  echo "Usage: $0 <backup.sql> [--yes]" >&2
  exit 1
fi

if [[ ! -f "$BACKUP" ]]; then
  echo "Backup file not found: $BACKUP" >&2
  exit 1
fi

if [[ "$YES" != "true" ]]; then
  echo "WARNING: This will replace data in the target database."
  echo "Backup file: $BACKUP"
  read -r -p "Type RESTORE to continue: " confirm
  if [[ "$confirm" != "RESTORE" ]]; then
    echo "Aborted."
    exit 1
  fi
fi

if docker compose ps db --status running >/dev/null 2>&1; then
  docker compose exec -T db psql -U dtr -d postgres -v ON_ERROR_STOP=1 \
    -c "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname = 'dtr' AND pid <> pg_backend_pid();"
  docker compose exec -T db psql -U dtr -d postgres -v ON_ERROR_STOP=1 -c "DROP DATABASE IF EXISTS dtr;"
  docker compose exec -T db psql -U dtr -d postgres -v ON_ERROR_STOP=1 -c "CREATE DATABASE dtr;"
  docker compose exec -T db psql -U dtr -d dtr -v ON_ERROR_STOP=1 < "$BACKUP"
else
  : "${DATABASE_URL:?DATABASE_URL must be set when Docker Compose db is not running}"
  psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -f "$BACKUP"
fi

echo "Restore completed from $BACKUP"