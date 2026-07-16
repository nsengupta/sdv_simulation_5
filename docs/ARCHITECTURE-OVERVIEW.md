# SDV simulation — architecture overview

This document is the **single source of truth** for where the project is going.
Implementation is **phase-by-phase**; each phase is specified in [`PHASES.md`](PHASES.md).
We agree on a phase before coding it. **Tests are mandatory** for every phase.

Related:

- [`DESIGN.md`](../DESIGN.md) — FSM, actors, observation streams (technical design)
- [`PHASES.md`](PHASES.md) — phased checklist with acceptance criteria
- [`TODO-twin-lifecycle.md`](TODO-twin-lifecycle.md) — TL-0–TL-5 done; TL-6+ mapped to phases
- [`TODO-simulation-5.md`](TODO-simulation-5.md) — carry-forward simulation items

---

## 1. Target topology (final round)

Five **independently runnable** command-line applications share a message carrier.
**CAN (`vcan0`) is the first carrier** and must work defect-free before any Zenoh work.

```text
                    ┌─────────────────────────────────────┐
                    │  Dashboard (TUI — driver + engineer) │
                    │  • displays diagnostic + ledger      │
                    │  • embeds Emulator *core* as library │
                    │  • CSV echo OR TUI driver controls   │
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
| **Emulator** | Sends lifecycle (`PowerOn`/`PowerOff`) and sensor frames onto the bus. **Echo** mode replays a CSV script; **generate** mode produces bounded-random values (today’s behaviour). Does not know whether the twin is listening. |
| **Actuators** | Separate processes per assembly (headlamp, wiper, …). Listen for CMD frames; respond only after receiving CMD. No spontaneous bus traffic. Future actuators follow the same pattern. |
| **Dashboard** | **Driver + engineer** TUI. Displays latest diagnostic and ledger (trusts twin emissions). **Embeds Emulator core** as a component — same engine as the standalone emulator binary. Interacts with Gateway for **observation only** (not direct FSM injection in the target model). |

### 1.1 Message carrier (phased)

| Carrier | When | Scope |
|---------|------|--------|
| **CAN (`vcan0`)** | Phases 1–7 | Emulator ↔ Gateway ↔ Actuators |
| **File / simple IPC** | Phase 5–6 | Gateway observation → Dashboard (before Zenoh) |
| **Zenoh + uProtocol** | Phase 8+ | Optional runtime config for all of the above |

All binaries that touch the bus today will later gain a **transport abstraction** (CAN vs Zenoh)
selected by CLI / config. **No Zenoh until CAN path is defect-free.**

### 1.2 Dashboard operating modes (target)

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

The codebase is **mid-migration**. Treat this as temporary.

| Aspect | Today | Target |
|--------|--------|--------|
| Twin location | **In-process** inside `tui_dashboard` via `TwinRuntimeBuilder` | **Gateway** process only |
| Dashboard ↔ Twin | Tokio MPSC channels in one `main()` | Observation over file/IPC → later Zenoh |
| Lifecycle | Dashboard **`s`/`o`** (programmatic) + gateway auto-PowerOn | Emulator (CSV or TUI) → CAN **`0x100`** |
| Emulator | Separate binary, **generate** only, no lifecycle frames | Echo + generate; lifecycle on CAN |
| Observation capture | None | Phase 3 library + files |
| Replay | None | Phase 6 |

**Naming:** keep crate **`tui_dashboard`** for now. **`simulator`** is reserved for a possible future umbrella binary name.

---

## 3. Gap register (objectives vs code)

| ID | Gap | Phase |
|----|-----|-------|
| G1 | **Closed:** CAN `0x100` → PowerOn/PowerOff wired at gateway ingress | **1** |
| G2 | **Closed:** silent ignore while `Off` enforced at the twin FSM boundary | **1** |
| G3 | Emulator cannot run full scripted lifecycle on CAN | **2** |
| G4 | No CSV **echo** mode | **2** |
| G5 | Twin co-located with dashboard | **5** |
| G6 | No human-readable observation files | **3** |
| G7 | No E2E golden regression on observation artifacts | **4** |
| G8 | Dashboard cannot drive embedded emulator from TUI | **6** |
| G9 | No standalone replay mode | **7** |
| G10 | Actuators / emulator / gateway locked to CAN socket | **8** (Zenoh) |
| G11 | Quit stops in-process twin; no graceful disband | **9** (TL-6/7) |

---

## 4. Run order (CAN, multi-process)

```text
1. Setup vcan0
2. Start actuators (headlamp, wiper)
3. Start Gateway (Digital Twin)
4. Start Dashboard (live observation) OR skip if headless CI
5. Start Emulator OR use Dashboard embedded emulator / CSV mode
```

Actuators **before** emulator — actuators must not send until they receive CMD.

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

---

## 6. Key code locations

| Topic | Path |
|-------|------|
| Combined app (transitional) | `crates/tui_dashboard/src/main.rs` |
| Gateway runtime / builder | `crates/gateway/src/gateway_runtime.rs` |
| CAN reader / dispatch | `crates/gateway/src/gateway_runtime.rs` |
| Twin ingress → FSM projection | `crates/common/src/twin_runtime/connectors/ingress_to_fsm.rs` |
| CAN signal IDs | `crates/common/src/signals.rs` |
| Silent ignore enforcement (Phase 1) | `crates/common/src/twin_runtime/controller/virtual_car_actor.rs` |
| Emulator generate loop | `crates/emulator/src/main.rs` |
| Headlamp / wiper actuators | `crates/front_headlamp_actuator/`, `crates/wiper_actuator/` |

---

*Last updated: 2026-07-15 — target multi-process architecture; CAN-first roadmap.*
