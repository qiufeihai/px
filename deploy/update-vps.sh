#!/usr/bin/env bash
set -euo pipefail

REPO_DIR="$(cd "$(dirname "$0")/.." && pwd)"
PREFIX="${PREFIX:-/opt/px}"
SERVICE_NAME="${SERVICE_NAME:-px-server}"

if [[ $EUID -ne 0 ]]; then
  echo "please run as root"
  exit 1
fi

source /root/.cargo/env

cd "$REPO_DIR"
git pull --ff-only
cargo build --release -p px-server

cp "$REPO_DIR/target/release/px-server" "$PREFIX/bin/px-server"
chmod 0755 "$PREFIX/bin/px-server"

systemctl restart "${SERVICE_NAME}.service"
systemctl status "${SERVICE_NAME}.service" --no-pager
