#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
PREFIX="${PREFIX:-/opt/px}"
SERVER_IP="${SERVER_IP:-}"
SERVER_DNS="${SERVER_DNS:-localhost}"

if [[ -z "$SERVER_IP" ]]; then
  echo "SERVER_IP is required, e.g. SERVER_IP=1.2.3.4 ./deploy/generate-vps-cert.sh" >&2
  exit 1
fi

mkdir -p "$PREFIX/config"

IP_SAN="$SERVER_IP" \
DNS_SAN="$SERVER_DNS" \
OUTPUT_DIR="$PREFIX/config" \
CERT_PATH="$PREFIX/config/server-cert.pem" \
KEY_PATH="$PREFIX/config/server-key.pem" \
"$ROOT_DIR/scripts/_generate-cert.sh"
