#!/usr/bin/env bash
set -euo pipefail

RUN_DIR="$(pwd)"
VPS_HOST="${VPS_HOST:?VPS_HOST is required}"
VPS_USER="${VPS_USER:-root}"
REMOTE_CERT_PATH="${REMOTE_CERT_PATH:-/opt/px/config/server-cert.pem}"
OUT_PATH="${OUT_PATH:-$RUN_DIR/config/server-cert.pem}"

mkdir -p "$(dirname "$OUT_PATH")"
scp "${VPS_USER}@${VPS_HOST}:${REMOTE_CERT_PATH}" "$OUT_PATH"
echo "downloaded: $OUT_PATH"
