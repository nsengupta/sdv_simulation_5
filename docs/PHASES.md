# SDV simulation — implementation phases

Each phase is a **review gate**: we discuss it, agree, then implement with **mandatory tests**.
Overview and target architecture: [`ARCHITECTURE-OVERVIEW.md`](ARCHITECTURE-OVERVIEW.md).

**Status key:** `Not started` · `In progress` · `Done`

---

## Roadmap at a glance

```text
Phase 1  CAN lifecycle + silent ignore          ← CAN correctness foundation
Phase 2  Finite lifecycle + telemetry emulator
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

**Status:** Done
**Goal:** CAN path is **semantically correct** before scenario tooling or process splits.

### Scope

1. Decode CAN **`0x100`** → `FsmEvent::PowerOn` / `PowerOff` on gateway ingress.
2. **Silent ignore** at twin boundary while FSM is **`Off`**: drop all ingress except `PowerOn`
   (no ledger, no context, no zone updates). Includes `PowerOff` while already `Off`.
3. At Phase 1 completion, programmatic `send_power_on/off()` remained for the then-transitional
   Dashboard lifecycle keys and gateway CI. Phase 2 subsequently removed those Dashboard keys.

### Out of scope

- Emulator transmitting `0x100` (Phase 2)
- Dashboard lifecycle-key removal (deferred from Phase 1 and completed in Phase 2)
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
- [x] Manual `vcan0` smoke: pre-Start CAN does **not** climb `Seq`; `0x100` PowerOn starts session

### Design choices to confirm at kickoff

- Resolved: `TwinIngressEvent::Lifecycle(LifecycleCommand::{PowerOn, PowerOff})`; `VssSignal`
  remains telemetry-only for future KUKSA blueprint-path interpretation.
- Resolved: the `VirtualCarActor` `TwinMessage::Fsm` arm is the single enforcement point.

---

## Phase 2 — Finite CAN emulator and observer-only Dashboard

**Status:** Done
**Goal:** A finite emulator drives lifecycle and telemetry **via CAN** while Dashboard only
observes the actual twin outcome.

### Scope

1. Require a positive `--readings N`; one reading is one RPM, ambient-lux, and rain cycle.
2. Transmit exactly `3N + 3` frames:
   `PowerOn`, then `N × (EngineRpm, AmbientLux, RainDetected)`, then `EngineRpm(0)`, then
   `PowerOff`.
3. Keep `EMULATOR_TUNNEL_PROB` and `EMULATOR_RAIN_PROB` as optional probability controls.
4. Exit after the final PowerOff frame is written.
5. Rely on the existing twin startup barrier. Its contract test verifies that telemetry sent
   immediately after PowerOn commits FIFO only after assembly startup reaches `Idle`.
6. Remove Dashboard lifecycle keys and direct lifecycle injection. Dashboard renders twin-authored
   diagnostic and ledger output only.

The emulator guarantees frame transmission order, not PowerOff acceptance. PowerOn is first,
RPM zero is penultimate, and PowerOff is final; if FSM guards reject PowerOff, Dashboard must show
the twin's unchanged state and rejection evidence.

### Out of scope

- CSV scenario parsing, authoring, replay, and echo mode are explicitly deferred.
- Dashboard embedding (Phase 6)
- Observation file capture (Phase 3)
- Zenoh

### Tests (mandatory)

- [x] Emulator CLI, model configuration, frame ordering/count, lifecycle boundaries, and sink errors
- [x] Twin startup-barrier FIFO characterization and controlled shutdown/rejection contracts
- [x] Dashboard observer-only behavior and waiting-for-CAN rendering
- [x] Manual `vcan0`: actuators → Dashboard → `emulator --readings 30`

### Acceptance

- [x] Focused checks and `cargo test --workspace` pass
- [x] Manual `vcan0` run confirms emulator exit, observer-only Dashboard, startup ordering,
  penultimate RPM zero, final PowerOff, and the twin's actual accepted or rejected final state
- [x] Mark Phase 2 `Done` only when both acceptance items above pass

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

1. Deferred Phase 4 script: setup `vcan0`, start actuators + gateway (or transitional combined
   app) + emulator echo.
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
4. Preserve the observer-only Dashboard established in Phase 2: lifecycle remains emulator-driven
   over CAN, with no Dashboard lifecycle injection.
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

1. Deferred Phase 6 work: wire **`emulator-core`** into dashboard (spawn in-process task or thread
   sending to `vcan0`).
2. **Deferred Mode A — CSV:** CLI flag passes CSV to embedded emulator (echo behavior is not part
   of active Phase 2).
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
