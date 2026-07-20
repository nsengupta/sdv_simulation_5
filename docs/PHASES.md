# SDV simulation — implementation phases

Each phase is a **review gate**: we discuss it, agree, then implement with **mandatory tests**.
Overview and target architecture: [`ARCHITECTURE-OVERVIEW.md`](ARCHITECTURE-OVERVIEW.md).

**Status key:** `Not started` · `In progress` · `Done`

---

## Roadmap at a glance

```text
Phase 1  CAN lifecycle + silent ignore          ← Done
Phase 2  Finite lifecycle + telemetry emulator ← Done
Phase 3  Observation capture library + files   ← Done
Phase 4  Emulator session runner (Mode 1)      ← Done (golden/CI TODOs remain)
Phase 5  Dashboard presentation rework         ← Done
Phase 6  Split Gateway ↔ Dashboard processes   ← Done
Phase 7  Embedded emulator + TUI driver        ← Cancelled (this simulation)
Phase 8  Standalone replay                     ← TBD next simulation
──────── next: observation over Zenoh ────────
Phase 9  Live observation: UDS | Zenoh         ← Next (design done; plan next)
Phase 10 Shutdown, disband, polish (TL-6/7/8)  ← Later
```

**Where we are:** Phase 6 Done — Gateway sole twin owner + file tee; Dashboard observation-only
over UDS. Vehicle bus stays **CAN** (`vcan0`). **Next work:** Phase 9 implementation plan →
implement (Zenoh as alternate live observation carrier). No push unless asked.

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
- Dashboard embedding (Phase 7)
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

**Status:** Done
**Goal:** Store diagnostic + ledger as **interpretable, versioned artifacts** for engineers and replay.

### Scope

1. The new L6 **`observation`** crate provides:
   - Schema version, run-id, numeric Unix timestamps, car identity, optional scenario metadata
   - Writers: append diagnostic + ledger rows to separate JSONL files
   - Readers: stream or load for tools / replay
2. The transitional `tui_dashboard` owns receiver-side capture of both MPSC streams. It requires
   the Twin boot diagnostic, then writes each run below the default `./observations` parent or
   the **`--observation-dir`** override.
3. Standalone pretty-print / summary utility (minimal CLI is fine).

### Out of scope

- Dashboard replay UI (Phase 8)
- Zenoh streaming

### Tests (mandatory)

- [x] Round-trip: write N rows → read back → equality
- [x] Schema version mismatch → clear error
- [x] Golden file: fixed run produces stable output (deterministic run-id override in test)

### Acceptance

- [x] Library tests pass; sample run directory committed under
  `crates/observation/testdata/golden/v1/`.
- [x] Focused `observation` and `tui_dashboard` tests, plus `cargo test --workspace`, pass.
- [x] Real default-path smoke: started actuators, Dashboard, and finite emulator on `vcan0`; quit
  Dashboard; verified all three run files and `observation-summary`.
- [x] Mark Phase 3 `Done` only after the real default-path smoke passes. Phase 4 remains
  **Not started**.

### Design choices to confirm at kickoff

- Resolved: separate `diagnostic.jsonl` + `ledger.jsonl` + `manifest.json`.
- Resolved: UUID v4 run IDs in production; explicit deterministic run IDs in tests.

---

## Phase 4 — Emulator session runner (Mode 1)

**Status:** Done  
**Goal:** Refactor the emulator around a reusable tick/`TelemetrySource` seam and a session
runner so Mode 1 stops via optional `--readings N` or Ctrl+C through one shared controlled
trailer. Prepare Mode 2 / semantic golden / CI without delivering them yet.

Design: [`docs/superpowers/specs/2026-07-18-phase-4-emulator-session-design.md`](superpowers/specs/2026-07-18-phase-4-emulator-session-design.md).

### Scope

1. `TelemetrySource` + `LivePhysicsSource` + session runner inside `crates/emulator`.
2. Optional `--readings N`; omit means run until Ctrl+C (emulator process only).
3. Shared `controlled_stop`: `EngineRpm(0)` then `PowerOff`.
4. Unit/session tests without `vcan0`.

### Deferred TODOs (not required for Phase 4 Done)

- Mode 2 file-driven `TelemetrySource` (CSV/JSONL triples; G4)
- Tick-file generator (N live ticks → file → Mode 2)
- Semantic golden / `observation-compare` (G7)
- SocketCAN-capable CI job / scripted multi-process orchestration

### Tests (mandatory)

- [x] Readings-limit session: PowerOn → N triples → Rpm(0) → PowerOff (mock sink)
- [x] Stop-flag session: same trailer once (simulates Ctrl+C)
- [x] CLI: optional `--readings`; invalid forms rejected
- [x] Live source emits usual field set / wire order

### Acceptance

- [x] Session runner + `TelemetrySource` seam wired; Mode 1 live source on SocketCAN
- [x] Optional `--readings` and Ctrl+C share one controlled stop
- [x] Focused emulator tests and docs updated
- [x] Manual `vcan0` smoke: Dashboard up → `emulator` or `--readings N` → stop → trailer;
  Dashboard shows Twin accept or reject of PowerOff (rejection while not Idle is a valid
  Twin-authored outcome)
- [x] Mark Phase 4 `Done` after the manual smoke

Original roadmap “E2E observation golden on `vcan0`” remains a **later gate** (G7 TODO), not
part of this phase’s Done criteria. Prefer keeping that green before the Phase 6 process split
when the golden design lands.

---

## Phase 5 — Dashboard presentation rework

**Status:** Done  
**Goal:** Present Twin diagnostic and ledger as a clear **driver / engineer / ledger-tail** TUI
using existing records only (honest `—` gaps). Prove Twin data sufficiency before process split.

Design: [`docs/superpowers/specs/2026-07-18-phase-5-dashboard-presentation-design.md`](superpowers/specs/2026-07-18-phase-5-dashboard-presentation-design.md).  
Layout: [`assets/Dashboard-format.txt`](../assets/Dashboard-format.txt).

### Scope

1. View-model modules: DriverPane, EngineerPane, LedgerTail (`tail -20`).
2. Speed + expanding/reducing bar (not RPM); width-clipped lines; Rain/Wipers/ROB as `—`.
3. Session bar and keys unchanged (observer-only).
4. Capture / boot diagnostic / persist-before-display unchanged.

### Emission hygiene (structured diagnostics)

Diagnostics emit structured [`DiagnosticKind`](superpowers/specs/2026-07-18-structured-diagnostics-design.md)
facts (no icons in payloads). Observer Notice filters the stream (e.g. hides `TimerTick`;
headlamp happy-path ACK is zone/context only). Observation archive schema version is **2**
(`kind` tagged union). Gateway ingress ACK `println!` is opt-in and off under Dashboard.

### Follow-up (still Phase 5; before Phase 6 — phases 6+ unchanged)

Original Done criteria above stay satisfied. Prefer landing this **presentation structure**
before the process split so Dashboard does not freeze on `Vec<String>` lines.

Design detail: [`2026-07-18-phase-5-dashboard-presentation-design.md`](superpowers/specs/2026-07-18-phase-5-dashboard-presentation-design.md) § Follow-up — structured lines.  
Plan tasks: [`2026-07-18-phase-5-dashboard-presentation.md`](superpowers/plans/2026-07-18-phase-5-dashboard-presentation.md) Task 6.

- [x] Structured `PaneLine` for **every** driver / engineer / ledger line: `role` + `segments[]`
  (semantic style tokens; view stays Ratatui-free). Segment content is an enum so later
  lines can mix text with widgets (`Text` | `Swatch` | `Icon` | `SpeedBar`, …).
- [x] Zoned speed bar (**B**): cells painted by scale zone from **`common`** band constants
  (green `0..=100`, yellow `101..=150`, red `>=151`; full scale =
  `SPEED_EXTREME_OPERATION_THRESHOLD_KPH`). Empty cells stay `.`. Label `Speed:` Default;
  numeric `N/160 km/h` uses current-band colour. Ledger `>` stays Default for now.
- [ ] Heads-up (same model, not required to close the follow-up): visibility swatch
  (e.g. low brown / high bright yellow), weather glyph (rain cloud / clear day) when Twin
  fields exist; Notice colour tokens later.

### Deferred TODOs (not required for Phase 5 Done or follow-up gate)

- Twin fields: rain, wiper status on Dashboard lines, ROB depth, assembly actors (after live use)
- Colour / Ratatui icons on filtered Notice lines (beyond speed zones)
- Process split (Phase 6)
- Zone-encased headlamp unconfirmed (TODO on `DiagnosticKind`)

### Tests (mandatory)

- [x] Speed bar helper in `common`
- [x] Driver / engineer / ledger-tail view-model unit tests
- [x] Capture and workspace tests remain green

### Acceptance

- [x] Layout matches Session / Diagnostic-Telemetry / State Transitions / Ledger / Keys
- [x] Manual `vcan0` smoke: Speed bar moves; ledger tails with `>`; gaps show `—`
- [x] Mark Phase 5 `Done` only after the manual smoke

---

## Phase 6 — Split Gateway and Dashboard processes

**Status:** Done  
**Goal:** **Gateway** is the sole twin owner; **Dashboard** is observation consumer only.

Design: [`docs/superpowers/specs/2026-07-19-phase-6-gateway-dashboard-split-design.md`](superpowers/specs/2026-07-19-phase-6-gateway-dashboard-split-design.md).  
Plan: [`docs/superpowers/plans/2026-07-19-phase-6-gateway-dashboard-split.md`](superpowers/plans/2026-07-19-phase-6-gateway-dashboard-split.md).

Absorbs the former [`TODO-connect-to-twin.md`](../TODO-connect-to-twin.md). Zenoh remains **Phase 9**.

### Delivered

1. **`gateway`**: install, CAN ingress, actuation, `ObservationTee` (`RunWriter` + optional UDS).
2. **`tui_dashboard`**: no `TwinRuntimeBuilder`; connects with `--uds` (default `./tmp/observation.sock`).
3. Live link: **UDS** under `<cwd>/tmp/` (never system `/tmp`); schema-v2 NDJSON `hello` + `event`.
4. Connect-gated install when `--uds` is set; headless Gateway (no `--uds`) still archives to files.
5. Observer-only Dashboard; lifecycle remains emulator-driven over CAN.
6. `TwinRuntimeBuilder` channel ownership unchanged (callers own receivers).
7. Detachable `LiveSink` / `LiveSource` in `observation` (UDS now; Zenoh later).

### Out of scope (unchanged)

- Zenoh/uProtocol (Phase 9)
- Embedded emulator in dashboard (Phase 7)
- Run-directory replay CLI (Phase 8)

### Tests (mandatory)

- [x] Gateway integration: install + tee → observation files and UDS client (`observation_capture_headless`, `observation_tee_uds`)
- [x] Dashboard integration: mock `LiveSource` → UI state; footer connected/disconnected
- [x] Two-process smoke script: [`scripts/smoke-phase6-two-process.sh`](../scripts/smoke-phase6-two-process.sh) (requires `vcan0`)

### Acceptance

- Documented multi-process run order: actuators → **gateway** (waits on UDS) → **dashboard** → emulator
- `cargo test --workspace` passes

---

## Phase 7 — Dashboard embeds emulator + TUI driver controls

**Status:** Cancelled (this simulation)  
**Decision (2026-07-19):** Drop embedded emulator / TUI driver controls. Lifecycle and sensors
stay on the **standalone emulator → CAN → Gateway** path. Dashboard remains observation-only.

Former intent (not scheduled here): in-process `emulator-core`, CSV Mode A, TUI keys → CAN
`0x100`. May be reconsidered in a future simulation if product needs a single-binary operator UI.

---

## Phase 8 — Standalone replay mode

**Status:** TBD — next simulation  
**Goal (deferred):** True **reproducibility** — dashboard renders stored runs without live
Gateway/Emulator/CAN (`tui_dashboard --replay …`, Phase 3 `RunReader`, golden frame checks).

Not in scope for the current simulation’s remaining work. File capture from Phase 6 remains
the archive that a future replay phase will consume.

---

## Phase 9 — Live observation transport: UDS | Zenoh *(next)*

**Status:** Design approved — **implementation plan next**  
**Prerequisite:** Phase 6 Done. Vehicle bus remains CAN for this phase.  
**Design:**
[`2026-07-20-phase-9-zenoh-observation-design.md`](superpowers/specs/2026-07-20-phase-9-zenoh-observation-design.md)  
**Related:** Phase 6 design
[`2026-07-19-phase-6-gateway-dashboard-split-design.md`](superpowers/specs/2026-07-19-phase-6-gateway-dashboard-split-design.md)
(`LiveSink` / `LiveSource` seam).

**Goal:** Operators choose an explicit live observation carrier — **UDS or Zenoh** — on both
Gateway and Dashboard. Same schema-v2 observation payloads; file archive tee unchanged.
Emulator/actuators stay on CAN.

### Kickoff decisions (locked)

See design § Kickoff decisions. Summary:

| Topic | Decision |
|-------|----------|
| First Zenoh slice | **Observation only** (Gateway → Dashboard live link) |
| Vehicle bus | **CAN unchanged** this phase |
| CLI style | **Mutually exclusive** long flags — **exactly one** required; **no default** |
| Gateway live flags | `--uds <path>` \| `--zenoh` \| `--no-live` (exactly one) |
| Dashboard live flags | `--uds <path>` \| `--zenoh` (exactly one; `--no-live` rejected) |
| UDS path | **Required** with `--uds`; resolve under `<cwd>/tmp` as in Phase 6 |
| Zenoh keyexpr | **Required** `--keyexpr <expr>` whenever `--zenoh` is set (both binaries) |
| Help | Both binaries: `-h` / `--help` with concrete example command lines |
| Zenoh topology (day one) | **Peer sessions** (no `zenohd` required) |
| Zenoh install gate | **Wait for first subscriber** on `--keyexpr`, then install twin |
| Wait timeout | Shared **`--connect-timeout <secs>`** (default e.g. 60) |
| Topic shape | **One** keyexpr; multiplexed schema-v2 `LiveMessage` |
| Impl approach | `ZenohLiveSink` / `ZenohLiveSource` in `observation` |
| File capture | Gateway `RunWriter` tee **always** |
| uProtocol / vehicle-bus Zenoh | Out of this phase |

### Acceptance (when implementation lands)

- Documented command pairs: UDS+UDS, Zenoh+Zenoh, and Gateway `--no-live`
- No silent defaults; exactly one live-mode flag per process
- Full emulator session with live Dashboard on Zenoh; CAN remains the bus
- Implementation plan filed under `docs/superpowers/plans/` before coding

---

## Phase 10 — Shutdown, disband, polish *(later)*

**Status:** Not started  
**Maps to:** [`TODO-twin-lifecycle.md`](TODO-twin-lifecycle.md) TL-6, TL-7, TL-8

### Scope

1. **TL-6:** Quit / signal → Stop if needed → disband actors + ingress workers.
2. **TL-7:** Disband on FSM `Off` without process exit.
3. **TL-8:** Gateway `--auto-power-on` CLI documented.
4. Actuator carrier config parity with Phase 9 when available.

### Tests (mandatory)

- [ ] Coordinator tests from TL-6 spec
- [ ] PowerOn → Idle → PowerOff → actor stopped

---

## Completed prerequisite work (reference)

| Item | Doc |
|------|-----|
| TL-0 – TL-5 twin lifecycle in combined app | [`TODO-twin-lifecycle.md`](TODO-twin-lifecycle.md) |
| Dashboard observer refactor | [`ARCHITECTURE-OVERVIEW.md`](ARCHITECTURE-OVERVIEW.md) §2 |
| TwinRuntimeBuilder + channel ownership | former `TODO-connect-to-twin.md` (superseded by Phase 6) |

---

## How to use this document

1. Pick the **next not-started phase**.
2. Review scope, out-of-scope, and open design choices in chat.
3. Agree → implement → check all boxes → mark **Done**.
4. Update [`ARCHITECTURE-OVERVIEW.md`](ARCHITECTURE-OVERVIEW.md) gap register and [`DESIGN.md`](../DESIGN.md) if behaviour changed.

---

*Last updated: 2026-07-20 — Phase 9 design approved; implementation plan next.*
