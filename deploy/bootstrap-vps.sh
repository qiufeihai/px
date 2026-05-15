#!/usr/bin/env bash
set -euo pipefail

REPO_DIR="$(cd "$(dirname "$0")/.." && pwd)"
PREFIX="${PREFIX:-/opt/px}"
SERVICE_NAME="${SERVICE_NAME:-px}"
PX_USER="${PX_USER:-px}"
PX_GROUP="${PX_GROUP:-px}"
SERVER_IP="${SERVER_IP:-}"
SERVER_DNS="${SERVER_DNS:-localhost}"
SKIP_CERT_GEN="${SKIP_CERT_GEN:-0}"
CONFIG_SRC="${CONFIG_SRC:-$REPO_DIR/config/server.prod.example.toml}"
CONFIG_DEST="${CONFIG_DEST:-$PREFIX/config/server.toml}"

if [[ $EUID -ne 0 ]]; then
  echo "please run as root"
  exit 1
fi

if [[ -z "$SERVER_IP" && "$SKIP_CERT_GEN" != "1" ]]; then
  echo "SERVER_IP is required when generating certificate"
  echo "example: sudo SERVER_IP=1.2.3.4 ./deploy/bootstrap-vps.sh"
  exit 1
fi

dnf install -y \
  git curl openssl ca-certificates \
  gcc gcc-c++ make cmake perl-core pkgconfig

if [[ ! -x /root/.cargo/bin/cargo ]]; then
  curl https://sh.rustup.rs -sSf | sh -s -- -y
fi

source /root/.cargo/env

cd "$REPO_DIR"
cargo build --release -p px-server

mkdir -p "$PREFIX/bin" "$PREFIX/config" "$PREFIX/systemd"

if [[ "$SKIP_CERT_GEN" != "1" ]]; then
  IP_SAN="$SERVER_IP" \
  DNS_SAN="$SERVER_DNS" \
  OUTPUT_DIR="$PREFIX/config" \
  CERT_PATH="$PREFIX/config/server-cert.pem" \
  KEY_PATH="$PREFIX/config/server-key.pem" \
  "$REPO_DIR/scripts/generate-cert.sh"
fi

cp "$REPO_DIR/target/release/px-server" "$PREFIX/bin/px-server"
cp "$REPO_DIR/deploy/systemd/px.service" "$PREFIX/systemd/px.service"

if [[ -f "$CONFIG_SRC" ]]; then
  cat > "$CONFIG_DEST" <<EOF
listen_addr = "0.0.0.0:6666"
tls_cert_path = "$PREFIX/config/server-cert.pem"
tls_key_path = "$PREFIX/config/server-key.pem"
connect_timeout_ms = 5000
log_level = "info"
EOF
else
  echo "missing config template: $CONFIG_SRC"
  exit 1
fi

if ! id "$PX_USER" >/dev/null 2>&1; then
  useradd --system --home "$PREFIX" --shell /sbin/nologin "$PX_USER"
fi

chmod 0755 "$PREFIX/bin/px-server"
chmod 0644 "$CONFIG_DEST"
chmod 0644 "$PREFIX/config/server-cert.pem"
chmod 0600 "$PREFIX/config/server-key.pem"
install -m 0644 "$PREFIX/systemd/px.service" "/etc/systemd/system/${SERVICE_NAME}.service"
chown -R "$PX_USER:$PX_GROUP" "$PREFIX"

systemctl daemon-reload
systemctl enable --now "${SERVICE_NAME}.service"
systemctl status "${SERVICE_NAME}.service" --no-pager

echo
echo "server installed under: $PREFIX"
echo "client should download cert: $PREFIX/config/server-cert.pem"
