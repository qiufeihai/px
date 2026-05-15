#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
BUNDLE_DIR="${BUNDLE_DIR:-$ROOT_DIR/dist/release}"
VPS_HOST="${VPS_HOST:?VPS_HOST is required}"
VPS_USER="${VPS_USER:-root}"
REMOTE_BASE_DIR="${REMOTE_BASE_DIR:-/opt/px}"
SSH_TARGET="${VPS_USER}@${VPS_HOST}"

if [[ ! -d "$BUNDLE_DIR" ]]; then
  echo "bundle directory not found: $BUNDLE_DIR"
  echo "run scripts/package-release.sh first"
  exit 1
fi

ssh "$SSH_TARGET" "mkdir -p $REMOTE_BASE_DIR/bin $REMOTE_BASE_DIR/config $REMOTE_BASE_DIR/systemd $REMOTE_BASE_DIR/deploy"

scp "$BUNDLE_DIR/bin/px-server" "$SSH_TARGET:$REMOTE_BASE_DIR/bin/px-server"
scp "$BUNDLE_DIR/config/server.toml" "$SSH_TARGET:$REMOTE_BASE_DIR/config/server.toml"
scp "$BUNDLE_DIR/systemd/px-server.service" "$SSH_TARGET:$REMOTE_BASE_DIR/systemd/px-server.service"
scp "$BUNDLE_DIR/deploy/install-server.sh" "$SSH_TARGET:$REMOTE_BASE_DIR/deploy/install-server.sh"

if [[ -f "$BUNDLE_DIR/config/server-cert.pem" ]]; then
  scp "$BUNDLE_DIR/config/server-cert.pem" "$SSH_TARGET:$REMOTE_BASE_DIR/config/server-cert.pem"
fi

if [[ -f "$BUNDLE_DIR/config/server-key.pem" ]]; then
  scp "$BUNDLE_DIR/config/server-key.pem" "$SSH_TARGET:$REMOTE_BASE_DIR/config/server-key.pem"
fi

echo "uploaded server bundle to $SSH_TARGET:$REMOTE_BASE_DIR"
