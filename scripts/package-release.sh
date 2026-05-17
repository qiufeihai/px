#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
OUT_DIR="${OUT_DIR:-$ROOT_DIR/dist/release}"
BUILD_TAURI="${BUILD_TAURI:-0}"
TAURI_BUNDLES="${TAURI_BUNDLES:-app}"

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR/bin" "$OUT_DIR/config" "$OUT_DIR/systemd" "$OUT_DIR/scripts"

cd "$ROOT_DIR"

cargo build --release -p px-server
cargo build --release -p px-dns-helper

cp target/release/px-server "$OUT_DIR/bin/px-server"
if [[ -x "$ROOT_DIR/target/release/px-dns-helper" ]]; then
  cp "$ROOT_DIR/target/release/px-dns-helper" "$OUT_DIR/bin/px-dns-helper"
fi
cp config/server.prod.example.toml "$OUT_DIR/config/server.toml"
cp config/client.prod.example.toml "$OUT_DIR/config/client.toml"
cp deploy/systemd/px.service "$OUT_DIR/systemd/px.service"

if [[ -f "$ROOT_DIR/config/server-cert.pem" ]]; then
  cp "$ROOT_DIR/config/server-cert.pem" "$OUT_DIR/config/server-cert.pem"
fi

if [[ -f "$ROOT_DIR/config/server-key.pem" ]]; then
  cp "$ROOT_DIR/config/server-key.pem" "$OUT_DIR/config/server-key.pem"
fi

cp "$ROOT_DIR/scripts/fetch-tun-helper.sh" "$OUT_DIR/scripts/fetch-tun-helper.sh"

if [[ -f "$ROOT_DIR/scripts/open-macos-app.sh" ]]; then
  cp "$ROOT_DIR/scripts/open-macos-app.sh" "$OUT_DIR/scripts/open-macos-app.sh"
  chmod +x "$OUT_DIR/scripts/open-macos-app.sh"
fi

if [[ -f "$ROOT_DIR/scripts/macos-tun-helper.sh" ]]; then
  cp "$ROOT_DIR/scripts/macos-tun-helper.sh" "$OUT_DIR/scripts/macos-tun-helper.sh"
  chmod +x "$OUT_DIR/scripts/macos-tun-helper.sh"
fi

if [[ "$BUILD_TAURI" == "1" ]]; then
  rm -rf "$ROOT_DIR/target/release/bundle"

  (
    cd "$ROOT_DIR/apps/tauri-ui"
    npm run tauri build -- --bundles "$TAURI_BUNDLES"
  )

  if [[ -x "$ROOT_DIR/target/release/px" ]]; then
    cp "$ROOT_DIR/target/release/px" "$OUT_DIR/px"
  elif [[ -x "$ROOT_DIR/target/release/PX 个人代理" ]]; then
    cp "$ROOT_DIR/target/release/PX 个人代理" "$OUT_DIR/px"
  elif [[ -x "$ROOT_DIR/target/release/tauri-ui" ]]; then
    cp "$ROOT_DIR/target/release/tauri-ui" "$OUT_DIR/px"
  fi

  if [[ -d "$ROOT_DIR/target/release/bundle/macos" ]]; then
    find "$ROOT_DIR/target/release/bundle/macos" -mindepth 1 -maxdepth 1 -name '*.app' -print0 | while IFS= read -r -d '' path; do
      cp -R "$path" "$OUT_DIR/"
    done
  fi
fi

echo "release bundle: $OUT_DIR"
