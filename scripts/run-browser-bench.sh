#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
SOCKS_ADDR="${SOCKS_ADDR:-127.0.0.1:7777}"
ITERATIONS="${ITERATIONS:-10}"

TARGETS=(
  "cloudflare.com:443"
  "github.com:443"
  "www.apple.com:443"
  "www.microsoft.com:443"
  "www.wikipedia.org:443"
)

if [[ -n "${BROWSER_TARGETS:-}" ]]; then
  # Split space-delimited targets from env for quick overrides.
  read -r -a TARGETS <<< "${BROWSER_TARGETS}"
fi

TMP_FILE="$(mktemp)"
trap 'rm -f "$TMP_FILE"' EXIT

echo "browser-like bench"
echo "socks=$SOCKS_ADDR iterations=$ITERATIONS"
echo

for target in "${TARGETS[@]}"; do
  echo "==> $target"
  if output="$(
    SOCKS_ADDR="$SOCKS_ADDR" \
    TARGET="$target" \
    ITERATIONS="$ITERATIONS" \
    "$ROOT_DIR/scripts/run-bench.sh" 2>&1
  )"; then
    echo "$output"
    direct_avg="$(printf '%s\n' "$output" | awk -F= '/^direct_avg_ms=/{print $2}')"
    direct_p95="$(printf '%s\n' "$output" | awk -F= '/^direct_p95_ms=/{print $2}')"
    direct_p99="$(printf '%s\n' "$output" | awk -F= '/^direct_p99_ms=/{print $2}')"
    socks_avg="$(printf '%s\n' "$output" | awk -F= '/^socks_avg_ms=/{print $2}')"
    socks_p95="$(printf '%s\n' "$output" | awk -F= '/^socks_p95_ms=/{print $2}')"
    socks_p99="$(printf '%s\n' "$output" | awk -F= '/^socks_p99_ms=/{print $2}')"
    delta="$(printf '%s\n' "$output" | awk -F= '/^delta_ms=/{print $2}')"
    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
      "$target" "$direct_avg" "$direct_p95" "$direct_p99" \
      "$socks_avg" "$socks_p95" "$socks_p99" "$delta" >> "$TMP_FILE"
  else
    echo "$output"
    printf '%s\tFAIL\n' "$target" >> "$TMP_FILE"
  fi
  echo
done

python3 - "$TMP_FILE" <<'PY'
import sys

rows = []
failures = []
with open(sys.argv[1], "r", encoding="utf-8") as fh:
    for line in fh:
        cols = line.rstrip("\n").split("\t")
        if len(cols) == 2 and cols[1] == "FAIL":
            failures.append(cols[0])
            continue
        rows.append(cols)

print("summary")
print(f"success={len(rows)} fail={len(failures)}")
if failures:
    print("failed_targets=" + " ".join(failures))

if rows:
    def avg(index: int) -> float:
        return sum(float(row[index]) for row in rows) / len(rows)

    print(f"direct_avg_mean_ms={avg(1):.2f}")
    print(f"direct_p95_mean_ms={avg(2):.2f}")
    print(f"direct_p99_mean_ms={avg(3):.2f}")
    print(f"socks_avg_mean_ms={avg(4):.2f}")
    print(f"socks_p95_mean_ms={avg(5):.2f}")
    print(f"socks_p99_mean_ms={avg(6):.2f}")
    print(f"delta_mean_ms={avg(7):.2f}")
PY
