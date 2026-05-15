#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
SOCKS_ADDR="${SOCKS_ADDR:-127.0.0.1:7777}"
ITERATIONS="${ITERATIONS:-10}"
ROUNDS="${ROUNDS:-3}"

TARGETS=(
  "cloudflare.com:443"
  "github.com:443"
  "www.apple.com:443"
  "www.microsoft.com:443"
  "www.wikipedia.org:443"
)

if [[ -n "${BROWSER_TARGETS:-}" ]]; then
  read -r -a TARGETS <<< "${BROWSER_TARGETS}"
fi

TMP_FILE="$(mktemp)"
trap 'rm -f "$TMP_FILE"' EXIT

echo "browser-like bench"
echo "socks=$SOCKS_ADDR iterations=$ITERATIONS rounds=$ROUNDS"
echo

for ((round = 1; round <= ROUNDS; round++)); do
  echo "-- round $round/$ROUNDS --"
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
      printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
        "$round" "$target" "$direct_avg" "$direct_p95" "$direct_p99" \
        "$socks_avg" "$socks_p95" "$socks_p99" "$delta" >> "$TMP_FILE"
    else
      echo "$output"
      printf '%s\t%s\tFAIL\n' "$round" "$target" >> "$TMP_FILE"
    fi
    echo
  done
  echo
done

python3 - "$TMP_FILE" <<'PY'
from collections import defaultdict
import sys

rows = []
failures = []

with open(sys.argv[1], "r", encoding="utf-8") as fh:
    for line in fh:
        cols = line.rstrip("\n").split("\t")
        if len(cols) == 3 and cols[2] == "FAIL":
            failures.append((cols[0], cols[1]))
            continue
        rows.append(cols)

print("summary")
print(f"success={len(rows)} fail={len(failures)}")

rounds = sorted({int(row[0]) for row in rows}) if rows else []
if rounds:
    print(f"rounds={len(rounds)}")

if failures:
    print(
        "failed_targets="
        + " ".join(f"round{round_no}:{target}" for round_no, target in failures)
    )

if rows:
    def avg(index: int) -> float:
        return sum(float(row[index]) for row in rows) / len(rows)

    print(f"direct_avg_mean_ms={avg(2):.2f}")
    print(f"direct_p95_mean_ms={avg(3):.2f}")
    print(f"direct_p99_mean_ms={avg(4):.2f}")
    print(f"socks_avg_mean_ms={avg(5):.2f}")
    print(f"socks_p95_mean_ms={avg(6):.2f}")
    print(f"socks_p99_mean_ms={avg(7):.2f}")
    print(f"delta_mean_ms={avg(8):.2f}")

    per_target = defaultdict(list)
    for row in rows:
        per_target[row[1]].append(row)

    print("per_target_summary")
    for target in sorted(per_target):
        target_rows = per_target[target]

        def target_avg(index: int) -> float:
            return sum(float(row[index]) for row in target_rows) / len(target_rows)

        print(
            f"{target} "
            f"runs={len(target_rows)} "
            f"direct_avg_mean_ms={target_avg(2):.2f} "
            f"socks_avg_mean_ms={target_avg(5):.2f} "
            f"socks_p95_mean_ms={target_avg(6):.2f} "
            f"socks_p99_mean_ms={target_avg(7):.2f} "
            f"delta_mean_ms={target_avg(8):.2f}"
        )
PY
