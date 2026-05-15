#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
APP_PATH="$ROOT_DIR/px.app"

if [[ ! -d "$APP_PATH" ]]; then
  APP_PATH="$(find "$ROOT_DIR" -maxdepth 1 -name "*.app" -type d | head -n 1)"
fi

if [[ -z "${APP_PATH:-}" || ! -d "$APP_PATH" ]]; then
  echo "app bundle not found under: $ROOT_DIR" >&2
  exit 1
fi

echo "Removing quarantine from: $ROOT_DIR"
xattr -dr com.apple.quarantine "$ROOT_DIR" 2>/dev/null || true

echo "Opening: $APP_PATH"
open "$APP_PATH"
