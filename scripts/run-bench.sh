#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"

SOCKS_ADDR="${SOCKS_ADDR:-127.0.0.1:7777}"
TARGET="${TARGET:-example.com:80}"
ITERATIONS="${ITERATIONS:-20}"

cd "$ROOT_DIR"

exec cargo run -p px-bench -- \
  --socks "$SOCKS_ADDR" \
  --target "$TARGET" \
  --iterations "$ITERATIONS" \
  "$@"
