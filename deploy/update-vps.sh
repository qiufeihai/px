#!/usr/bin/env bash
set -euo pipefail

REPO_DIR="$(cd "$(dirname "$0")/.." && pwd)"
PREFIX="${PREFIX:-/opt/px}"
SERVICE_NAME="${SERVICE_NAME:-px-server}"
CONFIG_DEST="${CONFIG_DEST:-$PREFIX/config/server.toml}"

if [[ $EUID -ne 0 ]]; then
  echo "please run as root"
  exit 1
fi

source /root/.cargo/env

cd "$REPO_DIR"
git pull --ff-only
cargo build --release -p px-server

systemctl stop "${SERVICE_NAME}.service"

mkdir -p "$PREFIX/bin" "$(dirname "$CONFIG_DEST")"
cp "$REPO_DIR/target/release/px-server" "$PREFIX/bin/px-server"
chmod 0755 "$PREFIX/bin/px-server"

if [[ -f "$CONFIG_DEST" ]]; then
  cp "$CONFIG_DEST" "${CONFIG_DEST}.bak.$(date +%Y%m%d%H%M%S)"
fi

cat > "$CONFIG_DEST" <<EOF
listen_addr = "0.0.0.0:6666"
tls_cert_path = "$PREFIX/config/server-cert.pem"
tls_key_path = "$PREFIX/config/server-key.pem"
connect_timeout_ms = 5000
log_level = "info"
EOF
chmod 0644 "$CONFIG_DEST"

systemctl start "${SERVICE_NAME}.service"
systemctl status "${SERVICE_NAME}.service" --no-pager
