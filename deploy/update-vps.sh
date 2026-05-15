#!/usr/bin/env bash
set -euo pipefail

REPO_DIR="$(cd "$(dirname "$0")/.." && pwd)"
PREFIX="${PREFIX:-/opt/px}"
SERVICE_NAME="${SERVICE_NAME:-px}"
LEGACY_SERVICE_NAME="${LEGACY_SERVICE_NAME:-px-server}"
CONFIG_DEST="${CONFIG_DEST:-$PREFIX/config/server.toml}"

if [[ $EUID -ne 0 ]]; then
  echo "please run as root"
  exit 1
fi

source /root/.cargo/env

cd "$REPO_DIR"
git pull --ff-only
cargo build --release -p px-server

mkdir -p "$PREFIX/bin" "$(dirname "$CONFIG_DEST")" "$PREFIX/systemd"
cp "$REPO_DIR/deploy/systemd/px.service" "$PREFIX/systemd/px.service"
install -m 0644 "$PREFIX/systemd/px.service" "/etc/systemd/system/${SERVICE_NAME}.service"

if systemctl list-unit-files | grep -q '^px-server\.service'; then
  systemctl disable "${LEGACY_SERVICE_NAME}.service" || true
fi

systemctl daemon-reload
systemctl stop "${SERVICE_NAME}.service" || true
systemctl stop "${LEGACY_SERVICE_NAME}.service" || true

install -m 0755 "$REPO_DIR/target/release/px-server" "$PREFIX/bin/px-server.new"
mv -f "$PREFIX/bin/px-server.new" "$PREFIX/bin/px-server"

if systemctl list-unit-files | grep -q '^px-server\.service'; then
  rm -f /etc/systemd/system/px-server.service
  systemctl daemon-reload
fi

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
