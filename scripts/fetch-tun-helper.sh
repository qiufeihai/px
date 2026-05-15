#!/usr/bin/env bash
set -euo pipefail

RUN_DIR="$(pwd)"
BIN_DIR="${BIN_DIR:-$RUN_DIR/bin}"
TUN2SOCKS_VERSION="${TUN2SOCKS_VERSION:-v2.6.0}"
WINTUN_VERSION="${WINTUN_VERSION:-0.14.1}"

OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
  Darwin)
    case "$ARCH" in
      arm64) TUN2SOCKS_ASSET="tun2socks-darwin-arm64.zip" ;;
      x86_64) TUN2SOCKS_ASSET="tun2socks-darwin-amd64.zip" ;;
      *) echo "unsupported macOS arch: $ARCH" >&2; exit 1 ;;
    esac
    HELPER_NAME="tun2socks"
    ;;
  Linux)
    case "$ARCH" in
      x86_64) TUN2SOCKS_ASSET="tun2socks-linux-amd64.zip" ;;
      aarch64|arm64) TUN2SOCKS_ASSET="tun2socks-linux-arm64.zip" ;;
      *) echo "unsupported Linux arch: $ARCH" >&2; exit 1 ;;
    esac
    HELPER_NAME="tun2socks"
    ;;
  *)
    echo "unsupported OS for this script: $OS" >&2
    echo "use scripts/fetch-tun-helper.ps1 on Windows" >&2
    exit 1
    ;;
esac

TUN2SOCKS_URL="https://github.com/xjasonlyu/tun2socks/releases/download/${TUN2SOCKS_VERSION}/${TUN2SOCKS_ASSET}"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

mkdir -p "$BIN_DIR"

curl -L "$TUN2SOCKS_URL" -o "$TMP_DIR/$TUN2SOCKS_ASSET"
unzip -o "$TMP_DIR/$TUN2SOCKS_ASSET" -d "$TMP_DIR/tun2socks"
HELPER_SOURCE="$(find "$TMP_DIR/tun2socks" -maxdepth 1 -type f -name 'tun2socks*' | head -n 1)"
if [[ -z "$HELPER_SOURCE" ]]; then
  echo "tun2socks binary not found in archive" >&2
  exit 1
fi
install -m 0755 "$HELPER_SOURCE" "$BIN_DIR/$HELPER_NAME"
echo "downloaded: $BIN_DIR/$HELPER_NAME"

if [[ "$OS" == "Linux" ]]; then
  echo "note: wintun.dll is not required on Linux"
fi

if [[ "$OS" == "Darwin" ]]; then
  echo "note: wintun.dll is not required on macOS"
fi

echo "tun2socks version: $TUN2SOCKS_VERSION"
echo "wintun version (windows only): $WINTUN_VERSION"
