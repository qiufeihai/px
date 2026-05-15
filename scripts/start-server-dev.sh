#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
CONFIG_PATH="${1:-$ROOT_DIR/config/server.toml}"

cd "$ROOT_DIR"
exec cargo run -p px-server -- --config "$CONFIG_PATH"
