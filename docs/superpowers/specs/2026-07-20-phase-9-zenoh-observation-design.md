# Phase 9 — Live observation transport: UDS | Zenoh

**Date:** 2026-07-20  
**Status:** Approved (design dialogue)  
**Related:** [`docs/PHASES.md`](../../PHASES.md) § Phase 9,
[`docs/ARCHITECTURE-OVERVIEW.md`](../../ARCHITECTURE-OVERVIEW.md),
[Phase 6 Gateway/Dashboard split](2026-07-19-phase-6-gateway-dashboard-split-design.md)

## Goal

Operators choose an explicit live observation carrier — **UDS or Zenoh (peer)** — on both
Gateway and Dashboard. Same schema-v2 `LiveMessage` payloads and Gateway file tee. Vehicle bus
remains **CAN**. No silent defaults; CLI flags are mutually exclusive and required.

## Kickoff decisions (locked)

| Topic | Decision |
|-------|----------|
| First Zenoh slice | Observation only (Gateway → Dashboard) |
| Vehicle bus | CAN unchanged |
| Library shape | Approach 1: `ZenohLiveSink` / `ZenohLiveSource` in `observation` behind existing traits |
| CLI style | Mutually exclusive long flags; exactly one mode; **no default** |
| Gateway modes | `--uds <path>` \| `--zenoh --keyexpr <k>` \| `--no-live` |
| Dashboard modes | `--uds <path>` \| `--zenoh --keyexpr <k>` (`--no-live` rejected) |
| UDS path | **Required** with `--uds`; resolve under `<cwd>/tmp` (Phase 6 rules) |
| Zenoh keyexpr | **Required** `--keyexpr <expr>` with `--zenoh` |
| Help | `-h` / `--help` on both binaries with concrete example command lines |
| Zenoh topology | **Peer sessions** (no `zenohd` day one) |
| Topic shape | **One** keyexpr; multiplexed `hello` + `event` (schema v2) |
| Install gate | UDS: accept one client; Zenoh: **wait for first subscriber** on keyexpr |
| Timeout | Shared `--connect-timeout <secs>` (default e.g. 60) for both waits |
| Multi-subscriber / reconnect / bootstrap | Not day one |
| uProtocol | Out of this phase |
| File capture | Always via `ObservationTee` → `RunWriter` when twin runs |

## Non-goals

- Zenoh or uProtocol for emulator ↔ gateway ↔ actuators
- `zenohd` router as required smoke dependency
- Phase 8 replay CLI
- Phase 7 embedded emulator (cancelled)
- Changing archival schema or convert-once tee model

---

## 1. Topology and ownership

```text
Emulator / actuators ──CAN──► Gateway (sole Twin owner)
                                │  ObservationTee
                                ├─► RunWriter  → ./observations/<run-id>/
                                └─► LiveSink
                                      ├─ UdsLiveSink
                                      └─ ZenohLiveSink  (peer)
                                              ▲
                                Dashboard     │ schema-v2 LiveMessage
                                      LiveSource
                                      ├─ UdsLiveSource
                                      └─ ZenohLiveSource
```

Dashboard `q` closes the live session only. Twin and archive continue in Gateway.

---

## 2. CLI and components

### 2.1 Flag rules

**Gateway — exactly one of:**

- `--uds <path>`
- `--zenoh` together with `--keyexpr <expr>`
- `--no-live`

**Dashboard — exactly one of:**

- `--uds <path>`
- `--zenoh` together with `--keyexpr <expr>`

**Shared / other:**

- `--connect-timeout <secs>` — UDS accept wait and Zenoh first-subscriber wait (default 60)
- Gateway: `--observation-dir`, existing debug flags as today
- `-h` / `--help` — usage plus at least three examples:
  1. UDS pair (gateway + dashboard)
  2. Zenoh pair (same `--keyexpr`)
  3. Gateway `--no-live` (headless archive)

Omit mode flag, combine modes, bare `--uds`, or `--zenoh` without `--keyexpr` → usage error.

### 2.2 `observation` crate

| Piece | Role |
|-------|------|
| `LiveSink` / `LiveSource` / `LiveMessage` | Unchanged contract |
| `UdsLiveSink` / `UdsLiveSource` | Phase 6; keep |
| `ZenohLiveSink` / `ZenohLiveSource` | Peer session; put/sub NDJSON lines on one keyexpr |
| `ObservationTee` | Unchanged convert-once → file + optional sink |

`ZenohLiveSink` readiness mirrors `UdsLiveSink::bind_and_accept`: open peer session, wait until
at least one subscriber matches `--keyexpr` (or timeout), put `hello`, then accept further
`emit` calls as puts.

### 2.3 Binaries

Composition roots parse CLI, construct the matching sink/source, then reuse Phase 6 install /
tee / UI apply paths. Footer shows connection text including UDS path or `zenoh:<keyexpr>`.

---

## 3. Lifecycle and errors

### Gateway `--uds`

Bind → accept (timeout) → `hello` → install → boot → `RunWriter` → tee.

### Gateway `--zenoh`

Peer session → wait first subscriber (timeout) → `hello` put → install → boot → `RunWriter` →
tee puts. Last subscriber gone / session loss: stop live emits; keep twin + files.

### Gateway `--no-live`

Install immediately; tee to files only (no live sink).

### Dashboard

Connect/subscribe → `hello` → footer connected → boot diagnostic event → TUI apply-before-display.
`q` exits TUI and closes live session.

### Errors

| Case | Behaviour |
|------|-----------|
| Missing / conflicting flags | Usage error; hint `-h` |
| UDS path outside `<cwd>/tmp` | Reject (`InvalidUdsPath`) |
| Accept / subscriber timeout | Exit before twin install |
| Zenoh session failure | Clear message; non-zero exit |
| `RunWriter` failure | Fail Gateway |
| Live disconnect after start | Stop live emits; continue archive |

---

## 4. Testing and acceptance

### Mandatory

- [ ] CLI: omit/conflict/`--uds` without path/`--zenoh` without keyexpr/Dashboard `--no-live`
- [ ] `-h` includes example lines
- [ ] UDS regression: connect-gated Gateway + Dashboard
- [ ] Zenoh: subscriber-wait then `hello` + event; timeout with no subscriber
- [ ] Dashboard (or unit path) applies boot from Zenoh/`LiveSource`
- [ ] `cargo test --workspace` passes

### Acceptance

- Documented matching pairs: UDS+UDS, Zenoh+Zenoh, Gateway `--no-live`
- Full emulator session with Dashboard on Zenoh; CAN remains vehicle bus
- Phase 9 marked Done in `PHASES.md` / gap G10 closed when implementation lands

### Out of test scope

uProtocol, required `zenohd`, vehicle-bus Zenoh, multi-subscriber fan-out, reconnect/bootstrap,
Phase 8 replay.

---

## 5. Implementation guidance (for the plan)

1. Add `zenoh` dependency to `observation` (pin compatible with workspace Tokio).
2. Implement Zenoh sink/source; keep framing identical to UDS (`LiveMessage::to_json_line`).
3. Refactor Gateway/Dashboard CLI to required mutually exclusive mode flags + help examples.
4. Wire composition; preserve `--no-live` headless path.
5. Docs + optional smoke script for Zenoh peer pair.

## Example commands (also for `-h`)

```bash
# UDS pair (from repo root)
cargo run -p gateway -- --uds observation.sock --connect-timeout 60
cargo run -p tui_dashboard -- --uds observation.sock

# Zenoh peer pair (same keyexpr)
cargo run -p gateway -- --zenoh --keyexpr sdv/twin/observation --connect-timeout 60
cargo run -p tui_dashboard -- --zenoh --keyexpr sdv/twin/observation

# Headless archive only
cargo run -p gateway -- --no-live --observation-dir observations
```

## Open items left to the plan (non-blocking)

- Exact Zenoh Rust API for “wait until subscriber count ≥ 1” (crate version may dictate polling
  vs callback); behaviour is mandatory even if implementation uses a short poll loop.
- Peer scouting defaults: use crate defaults unless local smoke fails, then add minimal knobs.
