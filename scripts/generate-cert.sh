#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
OUTPUT_DIR="${OUTPUT_DIR:-$ROOT_DIR/config}"
CERT_PATH="${CERT_PATH:-$OUTPUT_DIR/server-cert.pem}"
KEY_PATH="${KEY_PATH:-$OUTPUT_DIR/server-key.pem}"
IP_SAN="${IP_SAN:-127.0.0.1}"
DNS_SAN="${DNS_SAN:-localhost}"
DAYS="${DAYS:-3650}"
SUBJECT="${SUBJECT:-/CN=px-server}"

mkdir -p "$OUTPUT_DIR"

tmp_conf="$(mktemp)"
trap 'rm -f "$tmp_conf"' EXIT

cat > "$tmp_conf" <<EOF
[req]
default_bits = 2048
prompt = no
default_md = sha256
x509_extensions = v3_req
distinguished_name = dn

[dn]
CN = px-server

[v3_req]
subjectAltName = @alt_names
basicConstraints = critical, CA:TRUE
keyUsage = critical, digitalSignature, keyEncipherment, keyCertSign
extendedKeyUsage = serverAuth
subjectKeyIdentifier = hash
authorityKeyIdentifier = keyid:always,issuer

[alt_names]
IP.1 = ${IP_SAN}
DNS.1 = ${DNS_SAN}
EOF

openssl req -x509 -nodes -newkey rsa:2048 \
  -keyout "$KEY_PATH" \
  -out "$CERT_PATH" \
  -days "$DAYS" \
  -subj "$SUBJECT" \
  -config "$tmp_conf" \
  -extensions v3_req

echo "cert: $CERT_PATH"
echo "key:  $KEY_PATH"
echo "ip_san: $IP_SAN"
echo "dns_san: $DNS_SAN"
