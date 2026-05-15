#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
OUT_DIR="${OUT_DIR:-$ROOT_DIR/dist/release}"
BUILD_TAURI="${BUILD_TAURI:-0}"
TAURI_BUNDLES="${TAURI_BUNDLES:-app}"

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR/bin" "$OUT_DIR/config" "$OUT_DIR/systemd" "$OUT_DIR/deploy" "$OUT_DIR/scripts"

cd "$ROOT_DIR"

cargo build --release -p px-server

cp target/release/px-server "$OUT_DIR/bin/px-server"
cp config/server.prod.example.toml "$OUT_DIR/config/server.toml"
cp config/client.prod.example.toml "$OUT_DIR/config/client.toml"
cp deploy/systemd/px.service "$OUT_DIR/systemd/px.service"
cp deploy/install-server.sh "$OUT_DIR/deploy/install-server.sh"
cp scripts/create-client-prod-config.sh "$OUT_DIR/scripts/create-client-prod-config.sh"
cp scripts/fetch-server-cert.sh "$OUT_DIR/scripts/fetch-server-cert.sh"
cp scripts/start-gui.sh "$OUT_DIR/scripts/start-gui.sh"

if [[ -f "$ROOT_DIR/config/server-cert.pem" ]]; then
  cp "$ROOT_DIR/config/server-cert.pem" "$OUT_DIR/config/server-cert.pem"
fi

if [[ -f "$ROOT_DIR/config/server-key.pem" ]]; then
  cp "$ROOT_DIR/config/server-key.pem" "$OUT_DIR/config/server-key.pem"
fi

if [[ "$BUILD_TAURI" == "1" ]]; then
  rm -rf "$ROOT_DIR/target/release/bundle"

  (
    cd "$ROOT_DIR/apps/tauri-ui"
    npm run tauri build -- --bundles "$TAURI_BUNDLES"
  )

  if [[ -x "$ROOT_DIR/target/release/PX 个人代理" ]]; then
    mkdir -p "$OUT_DIR/gui"
    cp "$ROOT_DIR/target/release/PX 个人代理" "$OUT_DIR/gui/PX 个人代理"
  elif [[ -x "$ROOT_DIR/target/release/tauri-ui" ]]; then
    mkdir -p "$OUT_DIR/gui"
    cp "$ROOT_DIR/target/release/tauri-ui" "$OUT_DIR/gui/tauri-ui"
  fi

  if [[ -d "$ROOT_DIR/target/release/bundle/macos" ]]; then
    mkdir -p "$OUT_DIR/gui"
    find "$ROOT_DIR/target/release/bundle/macos" -mindepth 1 -maxdepth 1 -name '*.app' -print0 | while IFS= read -r -d '' path; do
      cp -R "$path" "$OUT_DIR/gui/"
    done
  fi
fi

echo "release bundle: $OUT_DIR"
