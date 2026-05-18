#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
HELPER_DIR="$ROOT_DIR/helpers/px-tun-helper"
RUNTIME_DIR="$ROOT_DIR/apps/tauri-ui/.px-dev-runtime"
BIN_DIR="$RUNTIME_DIR/bin"
CONFIG_PATH="$RUNTIME_DIR/config/client.toml"
TARGET_PATH="$BIN_DIR/px-tun-helper"

mkdir -p "$BIN_DIR"

echo "[install-dev-px-tun-helper] building Go helper"
(
  cd "$HELPER_DIR"
  go build -o "$TARGET_PATH" ./cmd/px-tun-helper
)

chmod 0755 "$TARGET_PATH"
echo "[install-dev-px-tun-helper] installed: $TARGET_PATH"

if [[ -f "$CONFIG_PATH" ]]; then
  python3 - "$CONFIG_PATH" <<'PY'
import sys
from pathlib import Path

config_path = Path(sys.argv[1])
raw = config_path.read_text()
old = 'helper_path = "bin/tun2socks"'
new = 'helper_path = "bin/px-tun-helper"'
if old in raw:
    raw = raw.replace(old, new, 1)
elif new in raw:
    pass
else:
    sys.exit(0)
config_path.write_text(raw)
PY
  echo "[install-dev-px-tun-helper] ensured helper_path=bin/px-tun-helper in $CONFIG_PATH"
fi
