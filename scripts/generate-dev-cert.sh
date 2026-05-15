#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"

IP_SAN="${IP_SAN:-127.0.0.1}" \
DNS_SAN="${DNS_SAN:-localhost}" \
OUTPUT_DIR="$ROOT_DIR/config" \
CERT_PATH="$ROOT_DIR/config/server-cert.pem" \
KEY_PATH="$ROOT_DIR/config/server-key.pem" \
"$ROOT_DIR/scripts/_generate-cert.sh"
