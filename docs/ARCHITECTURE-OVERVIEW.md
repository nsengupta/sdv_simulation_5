# SDV simulation — architecture overview

This document is the **single source of truth** for where the project is going.
Implementation is **phase-by-phase**; each phase is specified in [`PHASES.md`](PHASES.md).
We agree on a phase before coding it. **Tests are mandatory** for every phase.

Related:

- [`DESIGN.md`](../DESIGN.md) — FSM, actors, observation streams (technical design)
- [`PHASES.md`](PHASES.md) — phased checklist with acceptance criteria
- [`TODO-twin-lifecycle.md`](TODO-twin-lifecycle.md) — TL-0–TL-5 done; TL-6+ mapped to phases
- [`TODO-simulation-5.md`](TODO-simulation-5.md) — carry-forward simulation items
- [`design-notes-pyramid-layers.md`](design-notes-pyramid-layers.md) — canonical L0–L6 dependency rules
- [Phase 3 observation-capture design](superpowers/specs/2026-07-17-phase-3-observation-capture-design.md)
  — storage format and capture ownership
- [Numeric Unix timestamps design](superpowers/specs/2026-07-18-numeric-unix-timestamps-design.md)
  — live `UnixTimestamp` and schema-v1 `{unix_seconds,nanosecond}` objects

---

## 1. Target topology (final round)

Five **independently runnable** command-line applications share a message carrier.
**CAN (`vcan0`) is the first carrier** and must work defect-free before any Zenoh work.

```text
                    ┌─────────────────────────────────────┐
                    │  Dashboard (TUI — driver + engineer) │
                    │  • displays diagnostic + ledger      │
                    │  • embeds Emulator *core* as library │
                    │  • deferred CSV echo / driver UI     │
                    └──────────┬───────────────┬───────────┘
                               │               │
              observation      │               │ lifecycle + sensors
              (live / replay)  │               ▼
                               │      ┌─────────────────┐
                               │      │ Emulator process │──┐
                               │      │ (or in-dash lib) │  │
                               │      └─────────────────┘  │
                               │                           │ CAN 0x100, 0x102…
                               ▼                           ▼
                    ┌──────────────────────────────────────────────┐
                    │              vcan0 (today)                    │
                    │         Zenoh + uProtocol (much later)        │
                    └──────▲───────────────────────▲───────────────┘
                           │                       │
              ┌────────────┴────────┐    ┌───────────┴────────────┐
              │ Gateway            │    │ Actuators (×N)         │
              │ (Digital Twin)     │    │ headlamp, wiper, …     │
              │ CAN ingress        │    │ CMD in / ACK out       │
              │ actuation egress   │    │ (carrier-adaptive)     │
              └────────────────────┘    └────────────────────────┘
```

| Application | Role |
|-------------|------|
| **Gateway** | Houses the **entire Digital Twin** (actor tree, FSM, zones, `SessionClock`). Reads driver/sensor ingress and publishes actuation. Emits **diagnostic** and **transition ledger** observation streams. |
| **Emulator** | Sends lifecycle (`PowerOn`/`PowerOff`) and bounded-random sensor frames onto the bus. Today it requires `--readings N`, writes a finite `3N + 3` frame run, and exits. It does not know whether the twin accepts its inputs. CSV/echo is a deferred future option. |
| **Actuators** | Separate processes per assembly (headlamp, wiper, …). Listen for CMD frames; respond only after receiving CMD. No spontaneous bus traffic. Future actuators follow the same pattern. |
| **Dashboard** | Today, an **observer-only** TUI (Phase 5: driver / engineer / ledger-tail panes) displaying twin-authored diagnostic and ledger output; it has no lifecycle keys. Embedded emulator / driver controls are deferred to Phase 7. It interacts with the twin for **observation only**, never direct FSM injection. |

### 1.1 Message carrier (phased)

| Carrier | When | Scope |
|---------|------|--------|
| **CAN (`vcan0`)** | Phases 1–7 | Emulator ↔ Gateway ↔ Actuators |
| **File / simple IPC** | Phase 6–7 | Gateway observation → Dashboard (before Zenoh) |
| **Zenoh + uProtocol** | Phase 9+ | Optional runtime config for all of the above |

All binaries that touch the bus today will later gain a **transport abstraction** (CAN vs Zenoh)
selected by CLI / config. **No Zenoh until CAN path is defect-free.**

### 1.2 Dashboard operating modes (future, deferred)

| Mode | User action | Behaviour |
|------|-------------|-----------|
| **Live + CSV** | Pass a CSV to the embedded emulator | Emulator core replays script → CAN → Gateway → Dashboard shows twin output |
| **Live + TUI driver** | Use TUI buttons (PowerOn, PowerOff, Drive, Park, …) | Dashboard instructs **embedded emulator core** → CAN → Gateway (dashboard never calls `send_power_on()` on the twin directly) |
| **Replay** | Pass stored observation files (`run-id`, timestamp) | Dashboard runs **standalone** — no Gateway, no Emulator, no CAN. Renders archived diagnostic + ledger for true **reproducibility**. |

### 1.3 Twin behaviour (unchanged commitments)

| Topic | Rule |
|-------|------|
| **While FSM `Off`** | **Silent ignore** — no ledger, no context mutation, no zone side-effects. Only **`PowerOn`** is handled. |
| **Lifecycle on bus** | `PowerOn` / `PowerOff` via CAN ID **`0x100`** (see [`TODO-simulation-5.md`](TODO-simulation-5.md) §1). |
| **PowerOff guard** | Rejected unless parked (`Idle` / standstill); twin records rejections on diagnostic + ledger. |
| **Trust boundary** | Dashboard panes show **only** twin emissions (+ static install metadata). No duplicated FSM logic in the UI. |
| **Session clock** | One `SessionClock` per twin install; `PowerOn`/`PowerOff` are **not** clock anchors. |

### 1.4 Observation storage (target)

Human-readable, versioned artifacts:

- **Diagnostic stream** and **transition ledger** stored as interpretable files (format fixed in Phase 3).
- Metadata: **run-id**, **timestamp**, **schema/version**, car identity, optional scenario CSV hash.
- Optional **observation library** crate used by Gateway (writer) and Dashboard (reader / replay).
- Standalone CLI utilities may pretty-print or diff runs (hand-made tools OK).

---

## 2. Transitional state (today)

Process split is done (Phase 6). Remaining migration is embedded emulator UI (Phase 7),
replay (Phase 8), and Zenoh (Phase 9).

| Aspect | Today | Target |
|--------|--------|--------|
| Twin location | **Gateway** process via `TwinRuntimeBuilder` | **Gateway** process only |
| Dashboard ↔ Twin | Live UDS (`LiveSink`/`LiveSource`, schema v2 NDJSON) under `<cwd>/tmp/` | Same, then Zenoh (Phase 9) |
| Lifecycle | Mode 1 emulator → CAN **`0x100`**; Dashboard has no lifecycle controls | Emulator or future driver UI → CAN **`0x100`** |
| Emulator | Separate binary; `TelemetrySource` + session runner; optional `--readings N` or Ctrl+C controlled stop; live bounded-random telemetry | Mode 2 file source / generator and embedded-driver options are deferred TODOs |
| Observation capture | Gateway `ObservationTee` → `RunWriter` (+ optional UDS); Dashboard observation-only | Unchanged file contract; Phase 8 replay from run dirs |
| Dashboard presentation | Phase 5 driver / engineer / ledger-tail; footer shows UDS connected/disconnected | Honest gaps (`—`) until Twin fields are added; inline widgets later |
| Replay | None | Phase 8 |

**Naming:** keep crate **`tui_dashboard`** for now. **`simulator`** is reserved for a possible future umbrella binary name.

---

## 3. Gap register (objectives vs code)

| ID | Gap | Phase |
|----|-----|-------|
| G1 | **Closed:** CAN `0x100` → PowerOn/PowerOff wired at gateway ingress | **1** |
| G2 | **Closed:** silent ignore while `Off` enforced at the twin FSM boundary | **1** |
| G3 | **Closed:** finite emulator sends full lifecycle and telemetry on CAN | **2** |
| G4 | CSV/echo / Mode 2 file `TelemetrySource` deferred (seam exists; reader TODO) | Future / post–Phase 4 |
| G5 | **Closed:** Gateway sole twin owner; Dashboard UDS observation consumer | **6** |
| G6 | **Closed:** versioned, human-readable observation artifacts written by the L6 `observation` adapter | **3** |
| G7 | No E2E observation golden / `observation-compare` yet (Phase 4 delivered emulator session; golden remains TODO) | Later |
| G8 | Dashboard cannot drive embedded emulator from TUI | **6** |
| G9 | No standalone replay mode | **7** |
| G10 | Actuators / emulator / gateway locked to CAN socket | **8** (Zenoh) |
| G11 | No graceful twin disband on Gateway stop (Dashboard `q` no longer tears down twin) | **10** (TL-6/7) |

---

## 4. Run order (CAN, multi-process)

```bash
cargo run -p front_headlamp_actuator
cargo run -p wiper_actuator
cargo run -p gateway -- --uds observation.sock
# after Gateway prints “waiting for Dashboard”:
cargo run -p tui_dashboard -- --uds observation.sock
EMULATOR_TUNNEL_PROB=0.01 \
EMULATOR_RAIN_PROB=0.008 \
cargo run -p emulator -- --readings 30
```

UDS paths resolve under `<cwd>/tmp/` (default `./tmp/observation.sock`). Headless capture:
`cargo run -p gateway` (no `--uds`) still writes `./observations/<run-id>/`.
Automated smoke: [`scripts/smoke-phase6-two-process.sh`](../scripts/smoke-phase6-two-process.sh).

With `--readings N`, the emulator sends PowerOn, then `N` RPM/lux/rain cycles, then RPM zero and
PowerOff (`3N + 3` frames). Without `--readings`, it runs until Ctrl+C on the emulator process,
then sends the same trailer. PowerOff transmission does not guarantee acceptance: if FSM guards
reject it, the observer-only Dashboard displays the twin's actual unchanged state and rejection
evidence. The existing twin startup barrier, verified by contract test, orders immediate
post-PowerOn readings behind assembly startup.

### 4.1 Observation capture (Phase 3)

The `observation` crate is an L6 persistence adapter. It depends downward only on
`common::facade`, which exposes the live diagnostic and transition-record types; `common` never
depends on `observation`. The detailed pyramid boundary is documented in
[`design-notes-pyramid-layers.md`](design-notes-pyramid-layers.md).

**Gateway** owns capture via `ObservationTee` (convert once → `RunWriter` + optional `LiveSink`).
Each run has a versioned `manifest.json` and separate `diagnostic.jsonl` / `ledger.jsonl`
streams. Dashboard consumes the live UDS feed only (apply-before-display). See the
[Phase 3 design](superpowers/specs/2026-07-17-phase-3-observation-capture-design.md) and
[Phase 6 design](superpowers/specs/2026-07-19-phase-6-gateway-dashboard-split-design.md).

---

## 5. Decisions recorded

| Date | Decision |
|------|----------|
| 2026-07-15 | Lifecycle on bus via **emulator** (not dashboard → twin direct injection). |
| 2026-07-15 | **Silent ignore** while FSM `Off`. |
| 2026-07-15 | Keep **`tui_dashboard`** crate name. |
| 2026-07-15 | CAN first; **Zenoh/uProtocol deferred** until CAN defect-free. |
| 2026-07-15 | Dashboard **embeds emulator core**; TUI driver buttons control emulator, not twin mailbox. |
| 2026-07-15 | **Replay** consumes stored observation files — standalone dashboard, no live twin. |
| 2026-07-15 | Phases agreed **one at a time** before implementation. |
| 2026-07-16 | Canonical external twin input is `TwinIngressEvent`; `VssSignal` remains telemetry-only for future KUKSA path interpretation. |
| 2026-07-16 | `VirtualCarActor` silently drops every non-PowerOn FSM event while `Off`. |
| 2026-07-16 | Phase 2 uses a required finite `--readings N`; CSV/echo is deferred. |
| 2026-07-16 | Dashboard lifecycle keys were removed; it observes twin-authored outcomes only. |
| 2026-07-17 | Phase 3 stores a separate `manifest.json`, `diagnostic.jsonl`, and `ledger.jsonl`; production run IDs are UUID v4 while tests inject deterministic IDs; transitional Dashboard capture ownership moves to Gateway in Phase 6. |
| 2026-07-18 | Phase 5 reworks Dashboard presentation (driver/engineer/ledger tail); Gateway↔Dashboard split deferred to Phase 6. |
| 2026-07-18 | Twin-authored wall times use a live `UnixTimestamp` (`Duration` since Unix Epoch). Schema v1 stores `{unix_seconds,nanosecond}` objects; summary/UI presentation is `yyyy-mm-dd | HH:mm:ss:nnnnnnnnn (UTC)`. Manifest keeps capture `created_at` and Twin `session_started_at`; Dashboard requires the boot diagnostic before creating a run. |
| 2026-07-19 | Phase 6: Gateway sole twin owner + capture tee; Dashboard UDS consumer; sockets under `<cwd>/tmp/`; detachable `LiveSink`/`LiveSource`. |

---

## 6. Key code locations

| Topic | Path |
|-------|------|
| Observation-only Dashboard | `crates/tui_dashboard/src/main.rs` |
| Gateway binary (capture + optional UDS) | `crates/gateway/src/main.rs` |
| Gateway runtime / builder | `crates/gateway/src/gateway_runtime.rs` |
| CAN reader / dispatch | `crates/gateway/src/gateway_runtime.rs` |
| Twin ingress → FSM projection | `crates/common/src/twin_runtime/connectors/ingress_to_fsm.rs` |
| CAN signal IDs | `crates/common/src/signals.rs` |
| Silent ignore enforcement (Phase 1) | `crates/common/src/twin_runtime/controller/virtual_car_actor.rs` |
| Finite emulator composition | `crates/emulator/src/main.rs`, `crates/emulator/src/runner.rs` |
| Headlamp / wiper actuators | `crates/front_headlamp_actuator/`, `crates/wiper_actuator/` |
| Observation schema, tee, live UDS | `crates/observation/` |
| Phase 6 smoke | `scripts/smoke-phase6-two-process.sh` |

---

*Last updated: 2026-07-19 — Phase 6 Gateway/Dashboard process split.*
