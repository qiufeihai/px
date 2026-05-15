#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
APP_BUNDLE="$ROOT_DIR/gui/PX 个人代理.app"
APP_BINARY="$APP_BUNDLE/Contents/MacOS/PX 个人代理"
FALLBACK_BINARY="$ROOT_DIR/gui/PX 个人代理"
LEGACY_BINARY="$ROOT_DIR/gui/tauri-ui"

cd "$ROOT_DIR"

if [[ -x "$APP_BINARY" ]]; then
  exec "$APP_BINARY"
fi

if [[ -x "$FALLBACK_BINARY" ]]; then
  exec "$FALLBACK_BINARY"
fi

if [[ -x "$LEGACY_BINARY" ]]; then
  exec "$LEGACY_BINARY"
fi

echo "未找到可启动的 GUI 程序。" >&2
echo "已检查:" >&2
echo "  $APP_BINARY" >&2
echo "  $FALLBACK_BINARY" >&2
echo "  $LEGACY_BINARY" >&2
exit 1
