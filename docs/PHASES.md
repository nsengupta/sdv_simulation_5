# SDV simulation — implementation phases

Each phase is a **review gate**: we discuss it, agree, then implement with **mandatory tests**.
Overview and target architecture: [`ARCHITECTURE-OVERVIEW.md`](ARCHITECTURE-OVERVIEW.md).

**Status key:** `Not started` · `In progress` · `Done`

---

## Roadmap at a glance

```text
Phase 1  CAN lifecycle + silent ignore          ← CAN correctness foundation
Phase 2  Emulator echo + generate on CAN
Phase 3  Observation capture library + files
Phase 4  E2E golden regression (CAN, vcan0)
Phase 5  Split Gateway ↔ Dashboard processes
Phase 6  Dashboard embeds emulator + TUI driver controls
Phase 7  Standalone replay mode
──────── Zenoh / uProtocol boundary ────────
Phase 8  Transport abstraction (CAN vs Zenoh)
Phase 9  Shutdown, disband, polish (TL-6/7/8)
```

**Current codebase** is transitional: `tui_dashboard` still hosts the twin in-process until **Phase 5**.

---

## Phase 1 — CAN lifecycle + silent ignore while `Off`

**Status:** In progress — implementation and automated tests complete; manual `vcan0` smoke pending  
**Goal:** CAN path is **semantically correct** before scenario tooling or process splits.

### Scope

1. Decode CAN **`0x100`** → `FsmEvent::PowerOn` / `PowerOff` on gateway ingress.
2. **Silent ignore** at twin boundary while FSM is **`Off`**: drop all ingress except `PowerOn`
   (no ledger, no context, no zone updates). Includes `PowerOff` while already `Off`.
3. Keep programmatic `send_power_on/off()` for transitional dashboard **`s`/`o`** and gateway CI.

### Out of scope

- Emulator transmitting `0x100` (Phase 2)
- Removing dashboard **`s`/`o`**
- Process split, observation files, Zenoh

### Touch points (expected)

| Area | Files |
|------|-------|
| CAN codec | `crates/common/src/signals.rs` |
| Ingress vocabulary + projection | `crates/common/src/domain_types.rs`, `ingress_to_fsm.rs`, `gateway/src/ingress/mapping.rs` |
| Gateway CAN reader | `crates/gateway/src/gateway_runtime.rs` |
| Silent ignore | `crates/common/src/twin_runtime/controller/virtual_car_actor.rs` |

### Tests (mandatory)

- [x] Unit: decode/encode `0x100` payloads
- [x] Contract: twin lifecycle ingress → `PowerOn`/`PowerOff`
- [x] Actor: RPM/lux while `Off` → **no** transition channel traffic; then `PowerOn` → normal hops
- [x] Actor: `PowerOff` while `Off` → silent drop (no `RejectedPowerOff` row)
- [x] Regression: existing contract tests green

### Acceptance

- [x] `cargo test --workspace` passes
- Manual smoke: pre-Start CAN does **not** climb `Seq`; `0x100` PowerOn starts session

### Design choices to confirm at kickoff

- Resolved: `TwinIngressEvent::Lifecycle(LifecycleCommand::{PowerOn, PowerOff})`; `VssSignal`
  remains telemetry-only for future KUKSA blueprint-path interpretation.
- Resolved: the `VirtualCarActor` `TwinMessage::Fsm` arm is the single enforcement point.

---

## Phase 2 — Emulator scenario runner (echo + generate)

**Status:** Not started  
**Goal:** Emulator drives full scripted runs **via CAN**, including lifecycle.

### Scope

1. CLI modes: **`--mode generate`** (default, today) and **`--mode echo --csv <path>`**.
2. CSV schema: rows for **`PowerOn`**, sensor frames (`0x102`–`0x104`), standstill, **`PowerOff`**.
3. Emulator transmits CAN **`0x100`** for lifecycle steps.
4. Echo mode: deterministic, exits when script completes (or `--loop` optional).
5. Extract **`emulator-core`** (or equivalent) library crate — shared logic for standalone binary **and** later dashboard embed (API surface only; dashboard wiring in Phase 6).

### Out of scope

- Dashboard embedding (Phase 6)
- Observation file capture (Phase 3)
- Zenoh

### Tests (mandatory)

- [ ] Unit: CSV parser → ordered frame sequence
- [ ] Integration: echo script on mock CAN socket → expected frame IDs/payloads
- [ ] Manual / optional CI: actuators → gateway/tui_dashboard → emulator echo → full Off→Idle→Off

### Acceptance

- `cargo test -p emulator` (+ new core crate) passes
- Documented CSV example under `docs/examples/` or `testdata/`

---

## Phase 3 — Observation capture library + human-readable files

**Status:** Not started  
**Goal:** Store diagnostic + ledger as **interpretable, versioned artifacts** for engineers and replay.

### Scope

1. New crate (e.g. **`observation`**) or module in `common`:
   - Schema version, run-id, timestamp, car identity, optional scenario metadata
   - Writers: append diagnostic + ledger rows (JSONL or agreed text format)
   - Readers: stream or load for tools / replay
2. Gateway and/or transitional `tui_dashboard` can tee MPSC streams to **`--observation-dir`**
   (exact CLI owner decided at kickoff).
3. Standalone pretty-print / summary utility (minimal CLI is fine).

### Out of scope

- Dashboard replay UI (Phase 7)
- Zenoh streaming

### Tests (mandatory)

- [ ] Round-trip: write N rows → read back → equality
- [ ] Schema version mismatch → clear error
- [ ] Golden file: fixed run produces stable output (deterministic run-id override in test)

### Acceptance

- Library tests pass; sample run directory committed or generated in CI artifact

### Design choices to confirm at kickoff

- File layout: single JSONL vs separate `diagnostic.jsonl` + `ledger.jsonl` + `manifest.json`
- Run-id generation (UUID vs deterministic for tests)

---

## Phase 4 — E2E golden regression (CAN defect-free gate)

**Status:** Not started  
**Goal:** Prove the **CAN-only** stack end-to-end before architectural splits.

### Scope

1. Script: setup `vcan0`, start actuators + gateway (or transitional combined app) + emulator echo.
2. Capture observation files (Phase 3).
3. **`observation-compare`** (or diff tool): compare against committed golden under `testdata/golden/`.
4. CI job (optional `vcan0` / `#[ignore]` locally) documented in README.

### Tests (mandatory)

- [ ] At least one golden scenario: PowerOn → drive → standstill → PowerOff
- [ ] Compare tool unit tests

### Acceptance

- Documented command reproduces golden match on clean tree
- **Gate:** Phase 5+ only after this passes reliably

---

## Phase 5 — Split Gateway and Dashboard processes

**Status:** Not started  
**Goal:** **Gateway** is the sole twin owner; **Dashboard** is observation consumer only.

Absorbs the intent of the former [`TODO-connect-to-twin.md`](../TODO-connect-to-twin.md) (Zenoh **not** required yet).

### Scope

1. **`gateway` binary**: full twin lifecycle — install, CAN ingress, actuation, observation tee.
2. **`tui_dashboard` binary**: **no** `TwinRuntimeBuilder` / no in-process twin.
3. Live observation link: **file tail** or **localhost IPC** (UDS/TCP) — pick one at kickoff.
4. Remove transitional dashboard **`s`/`o`** (lifecycle only via emulator on CAN).
5. `TwinRuntimeBuilder` channel ownership model unchanged (callers own receivers).

### Out of scope

- Zenoh/uProtocol (Phase 8)
- Embedded emulator in dashboard (Phase 6)

### Tests (mandatory)

- [ ] Gateway integration: install + CAN inject → observation output
- [ ] Dashboard integration: mock observation stream → UI state updates (headless or unit)
- [ ] Two-process smoke test script

### Acceptance

- Documented five-process run order works with split gateway + dashboard
- `cargo test --workspace` passes

---

## Phase 6 — Dashboard embeds emulator + TUI driver controls

**Status:** Not started  
**Goal:** Dashboard is **driver + engineer** UI; lifecycle and sensors flow **Dashboard → embedded emulator core → CAN → Gateway**.

### Scope

1. Wire **`emulator-core`** into dashboard (spawn in-process task or thread sending to `vcan0`).
2. **Mode A — CSV:** CLI flag passes CSV to embedded emulator (same as Phase 2 echo).
3. **Mode B — TUI driver:** buttons/keys — **PowerOn**, **PowerOff**, **Drive**, **Park**, … map to
   emulator commands (which emit CAN frames, including `0x100`).
4. Dashboard **never** calls `VehicleController::send_power_on/off()` on the twin.
5. Engineer panes unchanged (diagnostic + ledger from Gateway observation link).

### Tests (mandatory)

- [ ] Embedded emulator unit: TUI command → CAN frame sequence
- [ ] Integration: button PowerOn → gateway ledger shows PowerOn hop (with split processes)

### Acceptance

- Operator can run full session from dashboard TUI without standalone emulator binary
- Standalone emulator binary still works for headless CI

---

## Phase 7 — Standalone replay mode

**Status:** Not started  
**Goal:** True **reproducibility** — dashboard renders stored runs without live Gateway/Emulator/CAN.

### Scope

1. CLI: `tui_dashboard --replay <observation-dir>` (or `--replay-run <run-id>`).
2. Reuse Phase 3 reader; drive TUI from archived streams (time-aware or step-through — decide at kickoff).
3. Engineer workflow: store run → share by run-id → replay later for demos / regression analysis.

### Tests (mandatory)

- [ ] Replay golden run → snapshot hash of rendered state (or key frame assertions)
- [ ] Missing/corrupt file → clear error

### Acceptance

- Replay matches live capture for at least one golden scenario

---

## Phase 8 — Transport abstraction: CAN vs Zenoh + uProtocol *(later)*

**Status:** Not started  
**Prerequisite:** Phases 1–4 green (CAN defect-free).

### Scope

1. Shared **transport trait** for ingress/egress (emulator, gateway, actuators, dashboard↔gateway observation).
2. Runtime selection: `--transport can` (default) vs `--transport zenoh` + router config.
3. uProtocol message mapping documented alongside existing CAN IDs.

### Tests (mandatory)

- [ ] Parity tests: same scenario over CAN vs Zenoh produces equivalent twin ledger (modulo timing)

---

## Phase 9 — Shutdown, disband, polish *(later)*

**Status:** Not started  
**Maps to:** [`TODO-twin-lifecycle.md`](TODO-twin-lifecycle.md) TL-6, TL-7, TL-8

### Scope

1. **TL-6:** Quit / signal → Stop if needed → disband actors + ingress workers.
2. **TL-7:** Disband on FSM `Off` without process exit.
3. **TL-8:** Gateway `--auto-power-on` CLI documented.
4. Actuator carrier config parity with Phase 8 when available.

### Tests (mandatory)

- [ ] Coordinator tests from TL-6 spec
- [ ] PowerOn → Idle → PowerOff → actor stopped

---

## Completed prerequisite work (reference)

| Item | Doc |
|------|-----|
| TL-0 – TL-5 twin lifecycle in combined app | [`TODO-twin-lifecycle.md`](TODO-twin-lifecycle.md) |
| Dashboard observer refactor | [`ARCHITECTURE-OVERVIEW.md`](ARCHITECTURE-OVERVIEW.md) §2 |
| TwinRuntimeBuilder + channel ownership | former `TODO-connect-to-twin.md` (superseded by Phase 5) |

---

## How to use this document

1. Pick the **next not-started phase**.
2. Review scope, out-of-scope, and open design choices in chat.
3. Agree → implement → check all boxes → mark **Done**.
4. Update [`ARCHITECTURE-OVERVIEW.md`](ARCHITECTURE-OVERVIEW.md) gap register and [`DESIGN.md`](../DESIGN.md) if behaviour changed.

---

*Last updated: 2026-07-15*
