# Phase 6 — Split Gateway and Dashboard processes

**Date:** 2026-07-19  
**Status:** Approved (design dialogue)  
**Related:** [`docs/PHASES.md`](../../PHASES.md) § Phase 6,
[`docs/ARCHITECTURE-OVERVIEW.md`](../../ARCHITECTURE-OVERVIEW.md) § transitional vs target (G5),
[`DESIGN.md`](../../../DESIGN.md) §16 Gateway vs Dashboard,
[Phase 3 observation capture](2026-07-17-phase-3-observation-capture-design.md),
[Phase 5 dashboard presentation](2026-07-18-phase-5-dashboard-presentation-design.md)

## Goal

**Gateway** is the sole Digital Twin owner. **Dashboard** is an observation-only consumer over a
live link. Capture ownership moves Gateway ← Dashboard. Emulator-driven lifecycle over CAN is
unchanged. Zenoh is out of scope (Phase 9). Embedded emulator / driver UI is out of scope
(Phase 7).

## Kickoff decisions

| Topic | Choice |
|-------|--------|
| Live observation link | Unix domain socket (UDS) |
| Gateway CLI | `--uds <path>` optional (bind + connect-gated install); omit = headless file-only |
| Dashboard CLI | `--uds <path>` required for live mode (client connect) |
| Wire payloads | Same archival schema v2 JSONL DTOs |
| Capture | Gateway **tee**: `RunWriter` + live sink |
| Clients | Single client; no reconnect in Phase 6 |
| Late join / boot | **Connect-gated install** (Gateway waits for Dashboard, then installs twin) |
| Bootstrap-on-connect | Deferred (future / Zenoh) |
| Library shape | Live transport as **detachable traits** in `observation`; UDS is one impl |

Stale stub [`TODO-connect-to-twin.md`](../../../TODO-connect-to-twin.md) still mentions Phase 5
split / Phase 8 Zenoh — **trust this spec and `PHASES.md`** (split = Phase 6; Zenoh = Phase 9).
Update that stub during implementation docs.

## Non-goals

- Zenoh / uProtocol (Phase 9)
- Embedded emulator in Dashboard (Phase 7)
- Standalone replay CLI (Phase 8) — but keep file contract and `DTO → live` path reusable
- Multi-client fan-out, reconnect, or mid-session catch-up
- Changing `TwinRuntimeBuilder` channel ownership (callers still own receivers)
- Reopening Phase 5 presentation work (structured diagnostics, `PaneLine`, zoned speed bar,
  ledger `SwitchedOff`, Rain/Wipers/ROB gaps, ExtremeOperationWarning rename)

---

## 1. Topology and ownership

```text
Emulator ──CAN──► Gateway (sole Twin owner)
                    │  TwinRuntimeBuilder (unchanged channel model)
                    │  MPSC diagnostic / ledger (Gateway owns receivers)
                    ├─► RunWriter  → <observation-dir>/<run-id>/
                    └─► LiveSink (trait) → UdsLiveSink (Phase 6)
                              ▲
                              │ UDS (--uds path)
                    Dashboard (observation consumer only)
                    LiveSource (trait) → UdsLiveSource
                    no TwinRuntimeBuilder / no in-process twin
```

| Role | Owns |
|------|------|
| **Gateway** | Twin install (after client connect), CAN ingress, actuation, observation tee, UDS bind |
| **Dashboard** | UDS connect, decode → UI, observer keys (`q` quit only) |
| **Emulator** | Lifecycle + sensors on `vcan0` (unchanged) |

Dashboard `q` closes the TUI and the UDS connection only. The twin keeps running in Gateway.

---

## 2. Components and detachable live interface

### 2.1 `observation` crate (L6)

| Piece | Responsibility |
|-------|----------------|
| `RunWriter` / `RunReader` | Unchanged Phase 3 archival contract (schema v2) |
| `LiveEvent` | Typed live messages: session control + diagnostic/ledger **schema v2 DTOs** |
| `LiveSink` | Trait: emit hello / run_started / events; finish. **No UDS types in the trait** |
| `LiveSource` | Trait: receive the same logical stream. **No UDS types in the trait** |
| `ObservationTee` | Drains live `common` records → convert **once** → `RunWriter` + `LiveSink` |
| `UdsLiveSink` / `UdsLiveSource` | Phase 6 transport impls; framing isolated here |
| DTO ↔ live helpers | Existing `live → DTO` for write; add `DTO → live` for Dashboard (and later replay) |

Gateway and Dashboard composition roots depend on `LiveSink` / `LiveSource`, not on socket
types. A future `ZenohLiveSink` / `ZenohLiveSource` swaps impls without rewriting the tee or
pane logic (Phase 9).

### 2.2 `gateway` binary

- CLI: `--uds <path>` (optional bind), `--observation-dir <parent>` (default `./observations`),
  `--connect-timeout <secs>` (wait for Dashboard when `--uds` is set; default e.g. 60).
- **With `--uds`:** bind → accept one client → `hello` → install twin → boot → `RunWriter` →
  `run_started` → tee (file + UDS).
- **Without `--uds` (headless / CI):** install immediately; still run `RunWriter` tee to files
  only (no live sink). Preserves today’s gateway-alone workflows and integration tests.
- Optional stdout observers remain orthogonal to the tee (existing flags).

### 2.3 `tui_dashboard` binary

- Remove in-process `TwinRuntimeBuilder` / twin install for live mode.
- CLI: `--uds <path>` (required for live mode). Live mode does **not** take
  `--observation-dir` (archive is Gateway-owned).
- UI loop consumes `LiveSource` via `DTO → live` into existing Phase 5 handlers.
- Persist-before-display becomes **apply-before-display** (no local `RunWriter`).

---

## 3. Conversion and Dashboard extract

Twin emissions stay on two Tokio channels (`DiagnosticRecord`, `PublishedTransitionRecord`).
The twin never knows about JSON, schema versions, or sockets.

```text
Twin ──MPSC──► ObservationTee
                 │  live records
                 │  ↓ convert once (shared schema helpers)
                 │  schema v2 DTO
                 ├─► RunWriter  (JSONL append)
                 └─► LiveSink   (same DTO on the wire)
```

```text
UDS ──► UdsLiveSource ──► LiveEvent (DTO)
              ↓ to_live()
         common live records
              ↓
         DashboardState / Phase 5 views
```

Dashboard never owns the MPSC pair. Structured fields (`DiagnosticKind`, FSM hops, etc.)
travel as schema v2, not prose.

---

## 4. Wire protocol and CLI

**Transport:** one Unix domain stream socket. Gateway binds; Dashboard connects. One client.
Disconnect ends the live feed; Gateway continues twin + file capture. No reconnect.

**Framing:** newline-delimited JSON (UTF-8), one object per line.

### 4.1 Line types

**Hello** — once after accept, before twin install:

```json
{
  "type": "hello",
  "schema_version": 2,
  "run_id": null,
  "observation_dir": "...",
  "vehicle": { "identity": "..." }
}
```

**Run started** — once `RunWriter` exists (after boot diagnostic):

```json
{
  "type": "run_started",
  "run_id": "...",
  "run_dir": "..."
}
```

**Events** — after install:

```json
{ "type": "event", "stream": "diagnostic", "record": { /* schema v2 diagnostic DTO */ } }
{ "type": "event", "stream": "ledger", "record": { /* schema v2 ledger DTO */ } }
```

Unknown `type` → Dashboard logs and skips (forward-compatible). No `Display`/prose on the wire.

### 4.2 CLI summary

| Binary | Flags |
|--------|--------|
| `gateway` | `--uds <path>` (optional; omit for headless file-only capture), `--observation-dir <parent>`, `--connect-timeout <secs>` |
| `tui_dashboard` | `--uds <path>` (required for live mode) |

### 4.3 Socket file hygiene

Gateway removes a stale socket path when safe to bind; best-effort unlink on clean shutdown.

---

## 5. Lifecycle and data flow

### 5.1 Gateway

1. Parse CLI. Create MPSC channels; attach senders to `TwinRuntimeBuilder` (unchanged
   ownership model).
2. **If `--uds`:** bind; print waiting message; accept one client or exit on connect-timeout;
   send `hello`. **If no `--uds`:** skip live bind (headless).
3. `install_controller` → boot diagnostic.
4. Tee waits for boot → `RunWriter::create` → if live client present, send `run_started` →
   persist boot to file and (when present) UDS.
5. `spawn_runtime` (CAN ingress / actuation workers).
6. Tee loop: `select!` on both receivers → convert once → file + optional `LiveSink`.
7. Client disconnect: stop live emits; keep twin + archive until Gateway process exits.

### 5.2 Dashboard

1. Connect to `--uds` (fail fast on refuse/missing path — no reconnect).
2. On `hello`: update **bottom pane / keys footer** so the human observer sees that the
   Dashboard has **connected to the twin** (not only a log line). Example intent:
   `Connected to twin via <uds-path>` (exact copy flexible).
3. Wait for `run_started` and boot diagnostic (short timeout); then enter TUI loop.
4. Apply-before-display from `LiveSource`.
5. If the socket closes later: show disconnected status in the same bottom pane; do not
   reconnect. Twin may still be running.
6. `q`: restore terminal, close socket, exit.

### 5.3 Documented run order

```text
vcan0 → actuators → gateway (waits on accept) → dashboard (connects) → emulator
```

---

## 6. Error handling

| Case | Behaviour |
|------|-----------|
| Bind / accept / connect failure | Clear message; non-zero exit |
| Connect-timeout (Gateway) | Exit; no twin install |
| Capture (`RunWriter`) failure | Fail Gateway (trustworthy archive) |
| UDS write after client disconnect | Stop live emits; continue file capture |
| Corrupt / unknown JSON line (Dashboard) | Skip + log; keep TUI alive |
| Boot / `run_started` timeout (Dashboard) | Clear error; exit before or without misleading UI |

---

## 7. Testing and acceptance

### Mandatory tests (`PHASES.md`)

- [ ] **Gateway integration:** install + CAN inject (or harness) → observation files valid
      (schema v2) **and** a UDS/`LiveSource` client receives boot + later events; tee
      consistency (same logical records on file and socket).
- [ ] **Dashboard integration:** mock `LiveSource` → UI state updates (headless/unit);
      after `hello`, bottom pane shows connected status; on socket close, shows disconnected.
- [ ] **Two-process smoke script:** gateway + dashboard over UDS; emulator or minimal CAN
      inject; run dir non-empty; dashboard `q` leaves gateway/twin running.

### Acceptance

- Documented multi-process run order works with split gateway + dashboard.
- `cargo test --workspace` passes.
- Docs updated: `PHASES.md` Phase 6 Done checklist, `ARCHITECTURE-OVERVIEW.md` transitional
  table / G5 closed, stale `TODO-connect-to-twin.md` redirected to Phase 6 / Zenoh Phase 9.

### Out of test scope

Zenoh, multi-client, reconnect/bootstrap, Phase 7 embedded emulator, Phase 8 replay CLI.

---

## 8. Implementation boundaries (guidance for the plan)

1. Extract shared `live → DTO` usage so tee and `RunWriter` cannot diverge; add `DTO → live`
   for Dashboard.
2. Introduce `LiveSink` / `LiveSource` + UDS impls in `observation` without leaking socket
   types into Gateway/Dashboard business logic beyond composition roots.
3. Move capture ownership into `gateway` main (or a thin gateway-side helper that owns the
   receivers); remove twin install from `tui_dashboard`.
4. Preserve Phase 5 view modules; only change the feed into `DashboardState`.
5. Keep `TwinRuntimeBuilder` public API and caller-owned receivers.

## Open items deliberately deferred

| Item | When |
|------|------|
| Bootstrap / catch-up on connect | Future / Zenoh |
| Zenoh `LiveSink` / `LiveSource` | Phase 9 |
| Dashboard embeds emulator | Phase 7 |
| `--replay` standalone mode | Phase 8 |
|
