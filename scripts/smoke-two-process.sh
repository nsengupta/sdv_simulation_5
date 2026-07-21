#!/usr/bin/env bash
# Two-process UDS smoke: Gateway (twin + UDS tee) + live UDS client + emulator.
#
# Prerequisites (from repo root):
#   sudo ip link add dev vcan0 type vcan 2>/dev/null || true
#   sudo ip link set up vcan0
#
# Documented operator order:
#   1) gateway --uds observation.sock
#   2) tui_dashboard --uds observation.sock   # separate terminal
#   3) emulator --readings N
#
# This script automates a headless client (connect-gate) + emulator so CI/smoke
# can verify archive output without a TTY. For a full TUI check, run dashboard
# manually in step 2 while this Gateway is waiting, or use two terminals.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

if ! ip link show vcan0 >/dev/null 2>&1; then
  echo "error: vcan0 is not available. Create it before running this smoke test." >&2
  exit 1
fi

mkdir -p tmp observations
SOCK="$ROOT/tmp/observation.sock"
rm -f "$SOCK"

cargo build -p gateway -p emulator

cargo run -p gateway --quiet -- --uds observation.sock --observation-dir observations --connect-timeout 60 &
GW_PID=$!

cleanup() {
  kill "$GW_PID" 2>/dev/null || true
  wait "$GW_PID" 2>/dev/null || true
  rm -f "$SOCK"
}
trap cleanup EXIT

for _ in $(seq 1 100); do
  if [[ -S "$SOCK" ]]; then
    break
  fi
  sleep 0.1
done
if [[ ! -S "$SOCK" ]]; then
  echo "error: Gateway did not create $SOCK" >&2
  exit 1
fi

# Headless UDS client: satisfy connect-gated install and stay up through the emulator run.
python3 - "$SOCK" <<'PY' &
import socket, sys, time
path = sys.argv[1]
s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
deadline = time.time() + 10
while True:
    try:
        s.connect(path)
        break
    except OSError:
        if time.time() > deadline:
            raise SystemExit(f"could not connect to {path}")
        time.sleep(0.1)
# Consume hello
s.makefile("rb").readline()
time.sleep(12)
s.close()
PY
CLIENT_PID=$!

sleep 0.5
cargo run -p emulator --quiet -- --readings 1

sleep 1
kill "$CLIENT_PID" 2>/dev/null || true
wait "$CLIENT_PID" 2>/dev/null || true

RUNS=$(find observations -mindepth 1 -maxdepth 1 -type d | wc -l)
if [[ "$RUNS" -lt 1 ]]; then
  echo "error: expected at least one observation run under ./observations" >&2
  exit 1
fi

if ! kill -0 "$GW_PID" 2>/dev/null; then
  echo "error: Gateway exited early" >&2
  exit 1
fi

echo "Two-process smoke OK: $RUNS observation run(s); Gateway still running after client disconnect."
