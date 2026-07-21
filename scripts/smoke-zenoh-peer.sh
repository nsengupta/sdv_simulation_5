#!/usr/bin/env bash
# Zenoh peer smoke: Gateway (twin + Zenoh tee) + hold subscriber + emulator.
#
# Prerequisites (from repo root):
#   sudo ip link add dev vcan0 type vcan 2>/dev/null || true
#   sudo ip link set up vcan0
#
# Documented operator order:
#   1) gateway --zenoh --keyexpr sdv/twin/observation
#   2) tui_dashboard --zenoh --keyexpr sdv/twin/observation
#   3) emulator --readings N [--tick-ms MS]
#
# This script holds a peer subscriber (install gate) + runs the emulator so smoke
# can verify archive output without a TTY.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

if ! ip link show vcan0 >/dev/null 2>&1; then
  echo "error: vcan0 is not available. Create it before running this smoke test." >&2
  exit 1
fi

KEY="sdv/twin/observation/smoke"
mkdir -p observations

cargo build -p gateway -p emulator -p observation

cargo run -p observation --quiet --example zenoh_hold_subscriber -- \
  --keyexpr "$KEY" --hold-secs 30 &
SUB_PID=$!

cargo run -p gateway --quiet -- \
  --zenoh --keyexpr "$KEY" --observation-dir observations --connect-timeout 60 &
GW_PID=$!

cleanup() {
  kill "$GW_PID" 2>/dev/null || true
  kill "$SUB_PID" 2>/dev/null || true
  wait "$GW_PID" 2>/dev/null || true
  wait "$SUB_PID" 2>/dev/null || true
}
trap cleanup EXIT

# Give Gateway time to see the subscriber, install, and create a run directory.
for _ in $(seq 1 40); do
  if find observations -mindepth 1 -maxdepth 1 -type d 2>/dev/null | grep -q .; then
    break
  fi
  sleep 0.25
done

cargo run -p emulator --quiet -- --readings 1

# Assert at least one observation run directory exists.
RUNS="$(find observations -mindepth 1 -maxdepth 1 -type d | wc -l)"
if [[ "$RUNS" -lt 1 ]]; then
  echo "error: expected observation run directory under observations/" >&2
  exit 1
fi

# Gateway should still be alive after subscriber may exit later.
if ! kill -0 "$GW_PID" 2>/dev/null; then
  echo "error: gateway exited early" >&2
  exit 1
fi

echo "smoke-zenoh-peer: ok ($RUNS run dir(s))"
