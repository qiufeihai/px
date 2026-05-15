#!/usr/bin/env bash
set -euo pipefail

RUN_DIR="$(pwd)"
OUT_PATH="${OUT_PATH:-$RUN_DIR/config/client.toml}"
SERVER_ADDR="${SERVER_ADDR:?SERVER_ADDR is required, e.g. 1.2.3.4:6666}"
SERVER_CERT_PATH="${SERVER_CERT_PATH:-config/server-cert.pem}"
LOCAL_SOCKS_ADDR="${LOCAL_SOCKS_ADDR:-127.0.0.1:7777}"
CONNECT_TIMEOUT_MS="${CONNECT_TIMEOUT_MS:-5000}"
LOG_LEVEL="${LOG_LEVEL:-info}"

mkdir -p "$(dirname "$OUT_PATH")"

cat > "$OUT_PATH" <<EOF
server_addr = "$SERVER_ADDR"
server_cert_path = "$SERVER_CERT_PATH"
local_socks_addr = "$LOCAL_SOCKS_ADDR"
connect_timeout_ms = $CONNECT_TIMEOUT_MS
log_level = "$LOG_LEVEL"
EOF

echo "generated: $OUT_PATH"
