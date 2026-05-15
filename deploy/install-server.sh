#!/usr/bin/env bash
set -euo pipefail

PREFIX="${PREFIX:-/opt/px}"
SERVICE_NAME="${SERVICE_NAME:-px-server}"
PX_USER="${PX_USER:-px}"
PX_GROUP="${PX_GROUP:-px}"

if [[ $EUID -ne 0 ]]; then
  echo "please run as root"
  exit 1
fi

if ! id "$PX_USER" >/dev/null 2>&1; then
  useradd --system --home "$PREFIX" --shell /sbin/nologin "$PX_USER"
fi

mkdir -p "$PREFIX/bin" "$PREFIX/config"

chmod 0755 "$PREFIX/bin/px-server"
chmod 0644 "$PREFIX/config/server.toml"
chmod 0644 "$PREFIX/config/server-cert.pem"
chmod 0600 "$PREFIX/config/server-key.pem"
install -m 0644 "$PREFIX/systemd/px-server.service" "/etc/systemd/system/${SERVICE_NAME}.service"

chown -R "$PX_USER:$PX_GROUP" "$PREFIX"

systemctl daemon-reload
systemctl enable --now "${SERVICE_NAME}.service"
systemctl status "${SERVICE_NAME}.service" --no-pager
