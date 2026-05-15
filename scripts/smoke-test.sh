#!/usr/bin/env bash
set -euo pipefail

SOCKS_ADDR="${SOCKS_ADDR:-127.0.0.1:7777}"
TARGET_URL="${TARGET_URL:-https://example.com}"
MAX_TIME="${MAX_TIME:-10}"

echo "testing via socks5h://$SOCKS_ADDR -> $TARGET_URL"
curl --fail --silent --show-error \
  --max-time "$MAX_TIME" \
  --proxy "socks5h://$SOCKS_ADDR" \
  "$TARGET_URL" \
  > /dev/null

echo "smoke test passed"
