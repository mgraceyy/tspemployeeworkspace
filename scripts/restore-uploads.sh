#!/usr/bin/env bash
set -euo pipefail

YES=false
ARCHIVE=""
TARGET_DIR=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    -y|--yes)
      YES=true
      shift
      ;;
    *)
      if [[ -z "$ARCHIVE" ]]; then
        ARCHIVE="$1"
        shift
      elif [[ -z "$TARGET_DIR" ]]; then
        TARGET_DIR="$1"
        shift
      else
        echo "Unexpected argument: $1" >&2
        exit 1
      fi
      ;;
  esac
done

if [[ -z "$ARCHIVE" ]]; then
  echo "Usage: $0 <uploads-backup.tar.gz> [target-upload-dir] [--yes]" >&2
  exit 1
fi

TARGET_DIR="${TARGET_DIR:-${UPLOAD_DIR:-./uploads}}"

if [[ ! -f "$ARCHIVE" ]]; then
  echo "Archive not found: $ARCHIVE" >&2
  exit 1
fi

if [[ "$YES" != "true" ]]; then
  echo "WARNING: This will replace files in $TARGET_DIR"
  echo "Archive: $ARCHIVE"
  read -r -p "Type RESTORE to continue: " confirm
  if [[ "$confirm" != "RESTORE" ]]; then
    echo "Aborted."
    exit 1
  fi
fi

mkdir -p "$(dirname "$TARGET_DIR")"
rm -rf "$TARGET_DIR"
tar -xzf "$ARCHIVE" -C "$(dirname "$TARGET_DIR")"

echo "Uploads restored to $TARGET_DIR from $ARCHIVE"