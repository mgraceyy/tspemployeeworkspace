#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 1 ]]; then
  echo "Usage: $0 <backup.sql>" >&2
  exit 1
fi

BACKUP="$1"

if [[ ! -f "$BACKUP" ]]; then
  echo "Backup file not found: $BACKUP" >&2
  exit 1
fi

if ! head -n 20 "$BACKUP" | grep -q "PostgreSQL database dump"; then
  echo "Invalid backup: missing PostgreSQL dump header" >&2
  exit 1
fi

if [[ ! -s "$BACKUP" ]]; then
  echo "Invalid backup: file is empty" >&2
  exit 1
fi

echo "Backup file looks valid: $BACKUP ($(wc -c < "$BACKUP") bytes)"