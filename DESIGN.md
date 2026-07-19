# SDV Simulation 4 — Architecture and Design

This document is the consolidated design reference for `sdv_simulation_4`.
It captures the final architecture (after Phases 1–9 of the Brain FSM Redesign),
the reasoning behind key decisions, and the open gaps carried forward to
`sdv_simulation_5`.

Source material: `brain_fsm_redesign_plan.md`, per-phase implementation notes
(Phases 2–9), `findings/`, and `analysis_4_response.md`.

---

## 1. Why Actor + FSM?

### Actor model rationale

An actor is single-threaded and event-driven by design.  Each actor has its own
mailbox; the framework (ractor) processes exactly one message at a time, awaiting
the handler future to completion before dispatching the next message.  This gives
a **sequential, deterministic execution order** without explicit locking.

The Virtual Car Brain is a natural actor:

- It collects a large number of messages from multiple assemblies and zones of the
  physical car.
- Each message is an event in the Brain's vocabulary; processing is: one event at a
  time, all externally delivered.
- The FSM it owns must run in the same thread as `handle()` — always synchronously —
  so the FSM step and the actor's state update are atomic from the actor's perspective.

### FSM rationale

The FSM is a **pure function**: `(State, Context, Event, Instant) → (NextState, Actions)`.

- No I/O, no heap discovery, no runtime branching on external sources.
- Testable at the speed of a function call — no mailbox round-trip needed.
- The transition table (`transition_map.rs`) is the **single authoritative source**
  of the Digital Twin's mode story, including startup and shutdown.
- Detectors (e.g., `LightingUnsafe`) are also pure functions that propose additional
  internal events; they do not modify state directly.

### Actor + FSM rule

> `transition()` and `output()` are always called in strict order, exactly once
> per event, from within the actor's `handle()` thread.  They are never interleaved.

---

## 2. Library Pyramid (L0–L6)

The `common` crate follows an acyclic layer pyramid.  The critical invariant:
**L2 (`fsm`) must not import L3 or above** — the FSM has no actor, no I/O, and
no runtime dependency.

| Layer | Module(s) | Role |
|---|---|---|
| **L0** | `vehicle_physics` | Constants and pure kinematics |
| **L1** | `vehicle_state`, `domain_types`, `signals` | Zone contexts, wire vocabulary, VSS signal IDs |
| **L2** | `fsm` | Pure decision core — `step`, `transition_map` |
| **L3** | `digital_twin`, `published` | Twin capsule, serializable mirror types |
| **L4** | `transition_sink`, `diagnostic`, `twin_runtime` | Actor runtime, sinks, detectors |
| **L5** | `facade` | Public surface — the only module gateway binaries may import |
| **L6** | `gateway`, `emulator`, `front_headlamp_actuator` | Application binaries |

The gateway imports **only `common::facade`** — a single doorway enforced by a CI script.

---

## 3. FSM States

```
Off  ──PowerOn──►  PreparingToStart({Headlamp, Wiper})
                          │ each AssemblyZoneReady(id) removes id
                          ▼
                   PreparingToStart({Wiper})
                          │ AssemblyZoneReady(Wiper) → set empty
                          ▼
                        Idle  ──UpdateRpm > threshold──►  Driving
                          │                                    │
                    PowerOff                             UpdateRpm < threshold
                          │                                    │
                          ▼                                    ▼
              PreparingToStop({Headlamp, Wiper})            Idle
                          │ symmetric to start
                          ▼
                         Off

  Driving  ──Internal(LightingUnsafe)──►  DrivingDangerously
  DrivingDangerously  ──headlamp On or lux high or stationary──►  Driving or Idle
```

`ExtremeOperationWarning` and `ActuationIncomplete` are additional transient states
for unsafe operational conditions.

### Key state invariants

- External events arriving in `PreparingToStart` / `PreparingToStop` are **recorded
  in the ledger** with `applied: false` and then discarded.  They are never replayed.
  (Fresh events arrive after `Idle` is reached.)
- The FSM never transitions directly `Off ↔ Idle` in the final design.  Every
  power cycle goes through a preparing state.

---

## 4. Final FSM Type: `FsmState::PreparingToStart(BTreeSet<AssemblyId>)` (Phase 9)

### The problem that was solved

After Phase 8 introduced `PreparingToStart { assemblies: &'static [AssemblyId] }`,
a **temporal mismatch** appeared in `transition_map.rs`.

- `transition()` needed to decide "transition to Idle or self-loop?" based on how
  many assemblies had acknowledged.
- But the countdown lived in `VehicleContext::remaining_assemblies`, which was only
  mutated *later* in `step.rs`.
- `transition()` had to do peek-ahead arithmetic on a future value it did not own.

Additionally, every self-loop reset the `assemblies` field back to `ALL_ASSEMBLIES`,
discarding progress.

### The solution (Phase 9 final design)

The countdown was moved **into** the FSM state variant itself:

```rust
// Phase 9 final — FSM state is the sole countdown authority
FsmState::PreparingToStart(BTreeSet<AssemblyId>)
FsmState::PreparingToStop(BTreeSet<AssemblyId>)
```

State evolution on each acknowledgement:

```
PreparingToStart({Headlamp, Wiper}) + AssemblyZoneReady(Headlamp)
    → PreparingToStart({Wiper})         // BTreeSet filtered; Headlamp removed

PreparingToStart({Wiper}) + AssemblyZoneReady(Wiper)
    → Idle                              // BTreeSet empty
```

`VehicleContext::remaining_assemblies` was deleted entirely.
`transition()` is now a fully self-contained pure function that reads and produces
the `BTreeSet` from the state variant directly.

### Why `BTreeSet` and not a static slice

| Option | Verdict |
|---|---|
| `&'static [AssemblyId]` | Can only hold `ALL_ASSEMBLIES`; cannot represent a shrinking subset |
| Fixed array + length counter | Off-by-one risk, order-dependent `PartialEq`; fine for no-alloc |
| `BTreeSet<AssemblyId>` | Shrinks naturally; `is_empty()` terminates the countdown cleanly |

For a bare-metal ECU (no allocator), replace with `arrayvec::ArrayVec<AssemblyId, MAX_ASSEMBLIES>` — a two-line change in `machineries.rs`; no logic changes elsewhere.

### `output()` intra-mode guard

`PreparingToStart({H,W}) ≠ PreparingToStart({W})` in Rust, so a naive
`(old, new) if old != new => PublishStateSync` would fire on every intermediate
acknowledgement.  Explicit guards suppress this:

```rust
(PreparingToStart(_), PreparingToStart(_)) => vec![],
(PreparingToStop(_), PreparingToStop(_))   => vec![],
```

---

## 5. Assembly Actors and the Zone Coordination Problem

### Why zone consultation is needed

Brain and each Assembly Actor (e.g., HeadlampActor, WiperActor) are separate ractor
actors with independent mailboxes.  For some FSM events, the correct FSM step depends
on what the assembly's internal state has become — but only the assembly actor knows that.

`begin_fsm_turn` answers:
> **"Can I commit the FSM step right now, or must I ask an assembly first?"**

### The four cases

**Case 1 — No zone consultation needed** (`UpdateRpm`):
The event has no zone message.  `run_to_quiescence` runs immediately with a simulated
zone reply.  Completes in a single `handle()` call.

**Case 2 — Zone consultation needed** (`UpdateAmbientLux`):
Brain tells HeadlampActor (fire-and-forget), arms a tell-back timeout timer, stashes a
`TurnBarrier` in the queue, and returns from `handle()`.  FSM commit is deferred until
`HeadlampZoneReady` arrives.

**Case 3 — Concurrent event while zone consultation is in flight**:
The second event is queued in the `VecDeque<TurnBarrier>` as a passthrough barrier.
It commits only after the first turn commits, preserving `old_ctx` accuracy in the ledger.

**Case 4 — `PowerOff` / `PowerOn`** (startup/shutdown coordination):
`PowerOn` → `PreparingToStart(ALL_ASSEMBLIES)`.  The FSM's `DomainAction::StartAssemblies`
triggers `apply_committed_quiescence` to push a `TurnBarrier` with `pending = ALL_ASSEMBLIES`
and tell each assembly `BecomeOn`.  The drain loop waits for all assemblies before
advancing to `Idle`.  Symmetric for `PowerOff`.

The old design (before Phase 4) had an `IgnitionOffReset` special case and
`fsm_step_lands_off()` which ran `zone_turn + step` twice (speculative + real).
Both were deleted.

---

## 6. The `VecDeque<TurnBarrier>` — Reorder Buffer (ROB) Pattern

### Structure

```rust
struct TurnBarrier {
    turn_id:      u64,
    event:        FsmEvent,
    now:          Instant,
    pending:      BTreeSet<ZoneId>,            // zones not yet replied
    zone_waits:   HashMap<ZoneId, TellBackWait>, // per-zone retry counters
    zone_timers:  HashMap<ZoneId, TellBackTimer>,
    replies:      HashMap<ZoneId, ZoneReply>,   // collected so far
}
```

The Brain actor holds:

```rust
barrier_queue: VecDeque<TurnBarrier>,
```

### Drain loop

```
When any ZoneReady(zone_id, turn_id, reply) arrives:
  1. Locate barrier with matching turn_id
  2. Move zone_id from pending → replies
  3. Walk from front:
       while front.pending.is_empty():
           execute completion action (commit_resolved_turn)
           pop_front()
  4. Stop at first barrier that still has pending zones
```

The front of the queue is always the oldest in-flight turn.  This enforces **in-order
commit** (ROB principle) without additional bookkeeping: zone replies are collected as
they arrive, but FSM commits happen strictly in event-arrival order.

### Why ordering matters

Suppose `UpdateAmbientLux(20)` (T1) and `UpdateWindshieldRain` (T2) are both in flight.
If rain (T2) commits before lux (T1), `run_to_quiescence` for rain runs with
`initial_ctx.headlamp = Off` — stale, because lux already moved headlamp to
`OnRequested` in the assembly actor.  The `LightingUnsafe` detector could fire falsely,
producing `DrivingDangerously` when the headlamp was already lit.

With the ROB pattern the same pair of events always produces the same ledger,
regardless of which zone replies arrive first.

### Per-zone retry logic

`TellBackWait` (one per zone, inside `TurnBarrier`) tracks retry count.
`ZONE_TELL_BACK_MAX_RETRIES = 2` means three total attempts.  On exhaustion, a
synthetic reply is committed — the turn proceeds without a real zone acknowledgement.
The zone's internal state becomes `ActuationIncomplete(direction)`.

---

## 7. State-Aware Zone Routing (`zone_message_for_event`)

```rust
fn zone_message_for_event(event: &FsmEvent, state: &FsmState)
    -> Option<(ZoneId, ZoneMessage)>
{
    match state {
        PreparingToStart(_) | PreparingToStop(_) => None,  // all external events discarded
        _ => per_event_type_routing(event),
    }
}
```

During preparing states, every external event returns `None` → no zone tell →
`commit_resolved_turn` runs immediately → FSM transition table stays in
`PreparingToStart`/`Stop` → ledger records `applied: false`.

`handle()` does not consult FSM state.  It dispatches to `begin_fsm_turn`, which
calls `zone_message_for_event`.  State-awareness lives in the routing function,
not the dispatcher.

---

## 8. `handle()` Has Exactly Four Arms

```rust
match message {
    Fsm(event)                                 => begin_fsm_turn(event),
    ZoneReady { zone_id, turn_id, reply }      => on_zone_ready(zone_id, turn_id, reply),
    ZoneTellBackTimeout { zone_id, turn_id }   => on_zone_timeout(zone_id, turn_id),
    GetStatus(reply_port)                      => reply_get_status(reply_port),
}
```

This structure is stable regardless of the number of assemblies.  Adding Wiper or
Window requires:
- `ZoneId::Wiper` added to the enum
- `Wiper(WiperZoneReply)` added to `ZoneReply`
- Wiper registered in `zone_message_for_event`
- Zero new arms in `handle()`

---

## 9. Quiescence and the Detector Catalog

A single external event can trigger multiple FSM hops before the system is stable.

```
external event → hop 1 → hop 2 → ... → stable cut → apply_step → actuation
                  └── one ledger row per hop ──┘
```

`run_to_quiescence` runs the detector catalog after each hop.  If a detector returns
`Some(FsmEvent::Internal(...))`, that event is enqueued as the next hop.  The loop
terminates when no detector fires (stable cut) or `MAX_QUIESCENCE_HOPS` is reached.

Detectors are pure functions in `twin_runtime/detectors/`.  They do not modify state.
They propose; the transition table decides.

### `LightingUnsafe` detector

Fires when: `Driving` state AND `ambient_lux < LUX_ON_THRESHOLD` AND headlamp state
is `Off` or `Ready` (not `OnRequested` or `On`).

Emits: `FsmEvent::Internal(Operational::LightingUnsafe)` → next hop transitions to
`DrivingDangerously` + `StartBuzzer` action.

**Known gap (simulation-5):** After `ActuationIncomplete(Off)` the headlamp state is
neither `Off` nor `Ready`, so the detector does not re-fire even when the lamp is
physically dark.  The system enters a quiescent state until a new lux event arrives.
See `TODO-simulation-5.md` §4c.

---

## 10. Async / Single-Thread Guarantee

`ractor` processes messages one at a time; the `handle` future is awaited to
completion before the next message is dispatched.

Every `.await` inside the Brain's `handle()` chain falls into one of three categories:

| Category | Example | Risk |
|---|---|---|
| Intra-actor structural (safe) | `begin_fsm_turn`, `commit_resolved_turn` chain | None — no external I/O |
| **Actuation channel** `.await` | `actuation_manager.execute(tx.send(...).await)` | Actor-stall if channel is full (backpressure) |
| `send_after` timer | Tell-back timeout delivery | None — ractor timer wheel delivers via mailbox |

The actuation `.await` does not reorder messages (Rust's `&mut` on stack prevents
concurrent access), but it **can stall the actor** if the CAN egress channel is full.
The existing `TODO(actuation-child-actor)` comment acknowledges this; the correct fix
is to offload actuation into `HeadlampActor`'s own thread.  Tracked in `TODO-simulation-5.md` §2.

---

## 11. Key Data Types (final state after Phase 9)

### `FsmState`

```rust
pub enum FsmState {
    Off,
    PreparingToStart(BTreeSet<AssemblyId>),   // shrinking set of pending assemblies
    Idle,
    Driving,
    DrivingDangerously,
    ExtremeOperationWarning,
    PreparingToStop(BTreeSet<AssemblyId>),    // symmetric to PreparingToStart
}
```

### `AssemblyId` and `ALL_ASSEMBLIES`

```rust
pub enum AssemblyId { Headlamp, Wiper }

pub(crate) const ALL_ASSEMBLIES: &[AssemblyId] = &[AssemblyId::Headlamp, AssemblyId::Wiper];
```

`ALL_ASSEMBLIES` is the single declaration of the Digital Twin's assembly topology.
`FsmState::PreparingToStart` seeds its `BTreeSet` from this constant on every
`Off + PowerOn` transition.

### `VehicleContext`

Carries only assembly-domain state (sensors, actuators).  Zero FSM lifecycle
bookkeeping.  The `remaining_assemblies` field (Phase 8 intermediate design) was
deleted in Phase 9.

### `DomainAction`

```rust
DomainAction::StartAssemblies(Vec<AssemblyId>)  // on PreparingToStart entry
DomainAction::StopAssemblies(Vec<AssemblyId>)   // on PreparingToStop entry
DomainAction::RequestFrontHeadlampOn
DomainAction::RequestFrontHeadlampOff
DomainAction::StartBuzzer
DomainAction::PublishStateSync
// ...
```

### `HeadlampState`

```rust
pub enum HeadlampState {
    Off,              // assembly not started
    Ready,            // assembly active; lamp dark; awaiting lux
    OnRequested,      // ON CMD sent; awaiting ACK
    On,               // ACK received; lamp confirmed lit
    OffRequested,     // OFF CMD sent; awaiting ACK
    ActuationIncomplete(FrontHeadlampSwitchDirection),  // max retries exhausted
}
```

`AckOff` lands in `Ready` (not `Off`) because the assembly is still active.
`ActuationIncomplete(On)` recovers to `Ready` (assembly active, lamp dark).

---

## 12. Headlamp Assembly Actor — Known Test Gaps

`headlamp_lifecycle_contract.rs` tests `HeadlampContext::on_receiving_message()` in
isolation (no actor, no Brain).  Coverage gaps:

| Missing test | Scenario |
|---|---|
| `actuation_incomplete_off_*` | What state after NACK/timeout on an OFF command |
| `nack_for_off_while_on_requested` | ON in flight; off-direction NACK arrives |
| `off_cmd_happy_path` | `OffRequested → AckOff → Ready` |

No test spawns `HeadlampActor` in isolation (without `VirtualCarActor`).

The **hang scenario** (ON → NACK-for-OFF → retry → drop → system quiets) is not
covered by any test at any level.  See `TODO-simulation-5.md` §4 for the full
breakdown.

---

## 13. What Was Deliberately Not Done in simulation-4

These items are documented in `TODO-simulation-5.md` and `brain_fsm_redesign_impl_Phase_10.md`
(now subsumed here):

| Item | Why deferred |
|---|---|
| **CAN emulation** for `PowerOn` / `PowerOff` (CAN ID `0x100`) | Architecture exists; wiring not implemented |
| **Non-blocking actuation** (offload `execute()` to child actor) | Correct fix known; risk of regression without actor-level tests |
| **Code commenting pass** over `begin_fsm_turn` call tree | Clean but undocumented; not blocking |
| **Actor-level fuzz/steady-state tests** | FSM-level `proptest` exists; actor-level stress tests missing |
| **HeadlampActor isolation tests** + `ActuationIncomplete(Off)` coverage | Identified from observed hang; not yet written |
| **`ArrayVec` migration** (no-alloc embedded target) | Two-line change; waiting for embedded target requirement |

---

## 14. Design Decisions Resolved During simulation-4

| Decision | Chosen direction |
|---|---|
| `PreparingToStart` payload type | `BTreeSet<AssemblyId>` (shrinking) over `&'static [AssemblyId]` (static) |
| Countdown ownership | Inside `FsmState` (deleted `VehicleContext::remaining_assemblies`) |
| `handle()` growth with assemblies | Generic zone envelope (`ZoneId` as data, not message name) |
| Event ordering across zones | ROB pattern (`VecDeque<TurnBarrier>`) — fire tells immediately, commit in arrival order |
| External events during startup/shutdown | `applied: false` ledger record + discard (never replay stale sensor data) |
| Speculative FSM execution for PowerOff | Deleted (`fsm_step_lands_off`, `IgnitionOffReset`) — explicit `PreparingToStop` state instead |
| Actuation blocking | Known risk; deferred to simulation-5 |
| `DomainAction` as actor intent signal | FSM emits `StartAssemblies`/`StopAssemblies`; actor executes — FSM does not inspect state-transition pairs |

---

## 15. Time, Clocks, and Observation Streams

> **Note for README / blog reuse:** session timing, gateway tick removal, stream
> contracts, and test strategy without UI.

### 15.1 `SessionClock` — the shared twin clock anchor

**When initialized:** once at `VirtualCarActor` install (`SessionClock::capture()` in
`pre_start`), before the first diagnostic or ledger emission.

**What it is (not a timestamp):** a monotonic↔wall anchor pair (`started_at_instant`,
`started_at_unix`) used to **project** monotonic [`Instant`]s into serializable wall times.
The session **start time** is exposed separately as `session_start_unix_nanos()`.

**How it is used:**

| Consumer | Field(s) | Meaning |
|---|---|---|
| Transition ledger | `session_start_unix_nanos`, `recorded_at_unix` | Run id + when this hop committed |
| Diagnostic log | `session_start_unix_nanos`, `recorded_at_unix` | Same pair on every driver-facing line |
| Actuation session id | `session_start_unix_nanos()` | Correlates CAN egress with this run |
| FSM internals | `Instant` + `clock.project(&instant)` | Monotonic inside; projected at emit boundary |

Elapsed since session start (both streams):

```text
elapsed = recorded_at_unix - Duration::from_nanos(session_start_unix_nanos)
```

**Naming rationale:** avoid "epoch" for the type — that overloads Unix epoch with "session
start moment". `SessionClock` is the projector; `session_start_unix_nanos` is the run id;
`recorded_at_unix` is when this observation was stamped on the wall clock.

**Wall-time field convention (all published types):**

| Role | Rust type | Suffix | Example |
|---|---|---|---|
| Run id / correlation | `u128` | `_unix_nanos` | `session_start_unix_nanos` |
| Wall-clock instant | `Duration` | `_at_unix` | `recorded_at_unix`, `entered_at_unix`, `ack_pending_since_at_unix` |

Every `_at_unix` value is a [`Duration`] since [`UNIX_EPOCH`], projected through
[`SessionClock::project`]. Names use `_at_unix` even when the meaning is “since when” —
the suffix marks **type and coordinate system**, not “at this calendar second” only.

**Intentional asymmetry on the session pair:** both fields describe time since Unix epoch,
but they use different Rust types on purpose:

- `session_start_unix_nanos` (`u128`) — compact, copyable run identifier; reused for actuation
  correlation and serde-friendly session grouping.
- `recorded_at_unix` (`Duration`) — supports `saturating_sub` and elapsed math without manual
  nanos conversion at every consumer.

Consumers derive elapsed as
`recorded_at_unix - Duration::from_nanos(session_start_unix_nanos)`. Do not add a second
`session_start_at_unix: Duration` field “for symmetry” — that duplicates the anchor and
invites drift between two representations of the same instant.

**Inside vs outside the process:** pure FSM / vehicle context keeps monotonic [`Instant`]
fields (e.g. `ack_pending_since`, `ExtremeOperationWarning(at)`). Published ledger types
project those at the emit boundary using the `_at_unix` suffix above. See §15.8.

### 15.2 Process topology and co-located clocks

**Tomorrow:** Gateway may run as a separate process (CAN ingress, actuation egress, IPC
to observers). See [`docs/PHASES.md`](docs/PHASES.md) Phases 3–7 for capture and replay.

**Today:** `VirtualCarActor`, `HeadlampActor`, and `WiperActor` run on the **same
physical machine/OS**. Gateway and Dashboard binaries may be separate processes; the
twin actor tree is co-located. We do **not** model clock skew between Brain and
child assembly actors.

**When Gateway splits off:** CAN and actuation become IPC. The twin runtime stays
co-located; domain timing must **not** depend on the gateway wall clock.

### 15.3 Gateway `TimerTick` removed

The unconditional 100 ms gateway loop that injected `FsmEvent::TimerTick` has been
**removed** from `TwinRuntimeBuilder::spawn_runtime`.

| Concern | Owner | Status |
|---|---|---|
| Headlamp ACK deadline | `HeadlampActor` (`send_after` → `ZoneSpontaneous`) | Done |
| Zone tell-back deadline | `VirtualCarActor` (`send_after`) | Done |
| FSM cooldown (`ExtremeOperationWarning`) | Brain-owned periodic `TimerTick` while in state | **TODO** |
| Engineer session heartbeat | Low-rate **ledger** row (not diagnostic) | **TODO** — see §15.5 |
| Dashboard elapsed display | Derived from twin-emitted `recorded_at_unix` / session pair | Done (freezes in idle until next emit) |

Gateway must **not** be the twin's heartbeat. Assembly deadlines must **not** rely on
gateway poll (Stage III).

### 15.4 Two streams, one session clock, different stories

| | Transition ledger | Diagnostic log |
|---|---|---|
| **Audience** | Automobile / twin engineers | Driver / operator |
| **Emits on** | Every committed FSM hop | Curated events (state change, warnings, ACKs, init) |
| **Timing pair** | `session_start_unix_nanos` + `recorded_at_unix` | Same pair (unified via `SessionClock`) |
| **Full story?** | Yes — event, states, contexts, actions, seq | No — human message only; not a ledger mirror |
| **Idle** | Silent (no commits) | Silent unless something driver-relevant occurs |

Ledger activity does **not** always translate to a new diagnostic. That is intentional.

**Heartbeat is not on the diagnostic stream.** Engineer periodic timing belongs on the
**ledger** (TODO), keeping driver diagnostics curated.

### 15.5 TODO — engineer ledger heartbeat

A low-rate twin-owned **ledger** emission (not gateway `TimerTick`, not diagnostic)
so engineers can observe that the twin clock is still running during FSM idle, and
to support future ingress→egress latency measurement on one engineering timeline.

Candidate shape: special ledger row or tagged event with session pair only — details
TBD. Dashboard and offline tools consume it from the transition channel.

### 15.6 Testing observation streams without Dashboard

Both streams are testable **entirely from the twin side** by wiring
`VehicleControllerRuntimeOptions` with tokio mpsc receivers — no UI, no gateway tick:

```rust
let (diag_tx, mut diag_rx) = mpsc::unbounded_channel();
let (trans_tx, mut trans_rx) = mpsc::channel(16);
VehicleController::install_and_start_with_options(id, VehicleControllerRuntimeOptions {
    diagnostic_tx: Some(diag_tx),
    transition_tx: Some(trans_tx),
    ..Default::default()
}).await?;
// drive with power_on_to_idle / FsmEvent; assert on rx only
```

Contract tests live in `crates/common/src/test/observation_streams_contract.rs`:

- Ledger and diagnostic rows share `session_start_unix_nanos`
- `recorded_at_unix` non-decreasing within each stream
- `elapsed_since_session` derivable and consistent on both streams
- Transition-only wiring still carries the session pair on every ledger row

This pattern is the supported way to validate twin emission before any Dashboard or
Gateway UI exists.

### 15.7 Dashboard (consumer only)

The Dashboard is a **passive consumer**: it reads whatever arrives on the diagnostic and
transition receivers that `main()` wired during setup. It **trusts the twin completely** for
display — session clock, FSM state, warnings, and ledger rows all come from twin emissions.
**Elapsed time** comes from the twin-emitted pair (latest diagnostic or ledger
`recorded_at_unix` minus session), not a local monotonic clock.

After **install**, the boot diagnostic populates the status bar even before Start — see §16.
If the twin runtime stops (process exit, dropped ingress handle), **no new emissions arrive**
and the dashboard shows the last received data or empty panes — it does not invent state.

Full application composition and lifecycle keys are §16.

### 15.8 Two time axes

The twin uses **two deliberate time axes**. They must not be conflated.

| Axis | Representation | Where | Answers |
|---|---|---|---|
| **Runtime monotonic** | [`Instant`], injected `now` at actor edge | FSM, zone context, turn barrier | “Has this deadline elapsed?” “How long since warning began?” |
| **Observable session** | [`SessionClock`] → `session_start_unix_nanos`, `recorded_at_unix` | Ledger, diagnostics, Dashboard | “Which run?” “Twin T+?” “When on the wall clock?” |

**Runtime monotonic is not wall clock.** It is the host process timeline for interval logic
inside the co-located actor tree. It is **not** “what time the machine thinks it is” in the
calendar sense.

**Observable session is not stored inside the FSM.** The FSM stores monotonic anchors;
projection to `_at_unix` happens at the emit boundary via `SessionClock::project`.

**Lifecycle events (`PowerOn`, `PowerOff`) do not re-anchor either axis.** They are FSM
transitions into `PreparingToStart` / `PreparingToStop` (see §16). `SessionClock` is
captured once at **install**, before Start.

**Replay:** deferred to a future design note (not in scope for twin-lifecycle work).

---

## 16. Twin lifecycle (Install → Start → Operate → Stop → Disband)

> **Note for README / blog reuse:** we do **not** model “accessory power” — dashboard
> indicators alive on install while the car is not yet ignited. CAN frames before Start
> are **harmless noise**; the FSM remains `Off` until `PowerOn`.

### 16.0 Application composition — target vs transitional

**Target (final round):** **separate processes** — Gateway owns the Digital Twin; Dashboard
consumes observation streams; Emulator and actuators are independent CLIs on CAN (Zenoh later).
Dashboard **embeds Emulator core** for CSV echo or TUI driver controls. Overview:
[`docs/ARCHITECTURE-OVERVIEW.md`](docs/ARCHITECTURE-OVERVIEW.md). Phased delivery:
[`docs/PHASES.md`](docs/PHASES.md).

**Transitional (today):** **`tui_dashboard`** = one process, one `main()` — twin installed
in-process via `TwinRuntimeBuilder` + dashboard UI. This is **Phase 0 debt** until
[`PHASES.md` Phase 6](docs/PHASES.md#phase-6--split-gateway-and-dashboard-processes).
The name **simulator** is reserved for a possible future umbrella binary.

| Part | Target | Today (transitional) |
|---|---|---|
| **Digital Twin** | **`gateway` binary** only | In-process inside `tui_dashboard` |
| **Dashboard** | Observation consumer + embedded emulator | Twin + UI in same process (Phase 5 presentation rework) |
| **Emulator** | Standalone or embedded in dashboard | Standalone Mode 1 session |
| **Observation** | Versioned files + live link | Tokio MPSC in one process |

**Setup call-tree** (before the dashboard loop):

1. `main()` creates diagnostic and transition channels (sender + receiver ends).
2. `TwinRuntimeBuilder::install_controller()` installs the twin and attaches the **senders**.
3. `spawn_runtime()` starts CAN reader, dispatch loop, and actuation publishers.
4. The dashboard loop holds the **receivers** and drains them each frame.

**Lifecycle (target):** **PowerOn** / **PowerOff** arrive on **CAN `0x100`** from the
**Emulator** (CSV script or dashboard-embedded emulator / TUI driver buttons). The dashboard
does **not** inject lifecycle into the twin mailbox directly. Headless gateway may still
auto-`PowerOn` for CI. Dashboard lifecycle keys are removed; CAN / emulator drive PowerOn/PowerOff.

Once **PowerOn** has been processed, the twin handles CAN ingress and emits **diagnostics**
and **ledger** rows. The dashboard renders the latest of each — it does not poll snapshots,
interpret FSM rules, or send vehicle commands.

**While `Off` (design commitment):** the twin is **installed and ready** but **silently ignores**
all driver-side ingress (RPM, lux, rain, …) until **PowerOn** — no ledger rows, no context
mutation. Only **`PowerOn`** is accepted. *Not fully implemented yet* — see refactoring doc G2.

**Trust boundary:** everything shown in Session / Diagnostic / Transition panes originates
from twin emissions (plus static car identity configured at install). Rejected `PowerOff`,
warnings, and state changes appear on the twin’s diagnostic and ledger streams; the dashboard
does not duplicate that logic.

**Ingress silence:** if the twin runtime is not running or CAN dispatch has stopped, the twin
receives no events and emits nothing new; the dashboard faithfully shows stale or empty data.

### 16.1 Session scope

**One application run → one twin session → one `SessionClock`.**

| Milestone | What happens |
|---|---|
| **Install** | `install_controller()` spawns actor tree; `SessionClock::capture()`; boot diagnostic; FSM `Off` |
| **Start** | **PowerOn** via CAN (emulator) or transitional dashboard **`s`** / headless auto-start → `PreparingToStart` → … → `Idle` |
| **Operate** | Emulators / actuators on CAN; twin processes ingress |
| **Stop** | **PowerOff** via CAN when twin is **`Idle`**, or transitional **`o`** → `PreparingToStop` → `Off` |
| **Disband** | Actor tree and ingress workers torn down; session ends |
| **New twin** | Requires **restarting the application** — no in-process reinstall |

`PowerOn` / `PowerOff` are **lifecycle FSM events** (future CAN `0x100` may map to them).
They are **not** clock anchors and must not re-capture `SessionClock`.

`PowerOn` / `PowerOff` are **lifecycle FSM events** (CAN ID `0x100` — see
`docs/TODO-simulation-5.md` §1). They are **not** clock anchors and must not re-capture
`SessionClock`.

### 16.2 Dashboard (driver + engineer)

The Dashboard is a **faithful observer** of twin diagnostic and ledger streams. In the **target**
architecture it is also the **driver console**: TUI buttons (PowerOn, PowerOff, Drive, Park, …)
instruct the **embedded Emulator core**, which emits CAN frames — not direct twin FSM calls.
Vehicle physics and actuation still flow **emulators + actuators → CAN → Gateway**.

**Engineer panes:** latest diagnostic and transition ledger (trust boundary unchanged).

**Operating modes (target):** see [`docs/ARCHITECTURE-OVERVIEW.md` §1.2](docs/ARCHITECTURE-OVERVIEW.md#12-dashboard-operating-modes-target).

Dashboard keys **`s`/`o`** exist **transitionally** in the combined app today.

**Layout** (`tui_dashboard`, Phase 5 — see `assets/Dashboard-format.txt`):

```text
┌─ Session ─────────────────────────────────────────────────────┐
│ Car: …  │  Twin T+: …  │  Session start  │  FSM: …  │  ledger │
└───────────────────────────────────────────────────────────────┘
┌─ Diagnostic/Telemetry ────┬─ State Transitions ───────────────┐
│ (driver: notice, speed…)  │ (engineer: state, ROB —, …)       │
├─ Deterministic Transition Ledger (live, tail −20) ────────────┤
│ > [seq] event  old → next                                     │
└───────────────────────────────────────────────────────────────┘
┌───────────────────────────────────────────────────────────────┐
│ Keys: 'q' quit                                                │
└───────────────────────────────────────────────────────────────┘
```

Before the **first post-PowerOn ledger row**, panes show a short install placeholder; the
Session row still reflects the boot diagnostic from the twin.

**Phase 5 follow-up (before Phase 6; phases 6+ unchanged):** structured `PaneLine` /
segment model for every pane line (semantic style tokens; optional future inline widgets
such as visibility swatches and weather icons); zoned speed bar colours from **`common`**
band constants (green ≤100, yellow ≤150, red above; full scale 160). See
[`docs/PHASES.md`](docs/PHASES.md) Phase 5 follow-up and the Phase 5 presentation design
§ Follow-up.

**Target flow:**

```text
App launch (tui_dashboard main)
  → setup: channels + TwinRuntimeBuilder::install + spawn_runtime
  → Dashboard loop drains receivers (twin Off — ingress silently ignored)
  → actuators on vcan0
  → emulator echo: PowerOn → sensor rows → standstill → PowerOff (all via CAN)
  → [q] quit → see §16.6
```

**Stop from non-`Idle`:** the twin **rejects** `PowerOff` and records it on ledger +
diagnostic. Reach **`Idle` via CAN / emulators** before a successful stop.

### 16.3 CAN before Start — silent ignore while `Off`

We **do not** emulate a real vehicle where the battery powers the dashboard but ignition
is off. The twin is **created and ready** (like an ECU waiting for ignition) but **does not
respond to driver input until PowerOn**:

- CAN reader may be running after install (implementation detail).
- While FSM is **`Off`**, ingress frames are **silently ignored** — no ledger, no context
  change, no “partially on” semantics. Only **`PowerOn`** is handled.
- **Target:** emulator sends PowerOn on CAN when the scenario starts; actuators before emulator.

*Implemented in Phase 1:* `VirtualCarActor` drops every non-`PowerOn` FSM event while `Off`
before turn allocation, so pre-Start CAN cannot mutate context, contact zones, or emit ledger
traffic. See [`docs/PHASES.md` Phase 1](docs/PHASES.md#phase-1--can-lifecycle--silent-ignore-while-off).

**Operator-facing detail:** [`README.md` — Dashboard app and twin lifecycle](README.md#dashboard-app-and-twin-lifecycle).

### 16.4 Runtime split (design commitment)

**Target:** Gateway process owns install, ingress workers, and observation export. Dashboard
process owns UI + embedded emulator; **no** in-process twin. Inter-process observation uses
file tail or simple IPC first ([`PHASES.md` Phase 6](docs/PHASES.md)); Zenoh/uProtocol is
[`PHASES.md` Phase 9](docs/PHASES.md).

`TwinRuntimeBuilder` (Gateway-side) separates:

1. **Install** — actor tree + channels + `SessionClock`.
2. **Ingress workers** — CAN reader + dispatch loop.
3. **Start / Stop** — via CAN lifecycle from Emulator (not dashboard → twin direct calls).

**Observation capture:** human-readable diagnostic + ledger files with run-id and schema version
([`PHASES.md` Phase 3](docs/PHASES.md)). **Replay:** dashboard standalone from stored files
([`PHASES.md` Phase 8](docs/PHASES.md)).

**Disband on Stop** — [`PHASES.md` Phase 10](docs/PHASES.md) / [`TODO-twin-lifecycle.md`](docs/TODO-twin-lifecycle.md) TL-6/7.

### 16.5 Gateway vs Dashboard

| Mode | Twin location | Start trigger | Use |
|---|---|---|---|
| **Target — Gateway** | Gateway process | Emulator CAN `0x100` | Production twin host |
| **Target — Dashboard** | None (observation only) | Embedded emulator / CSV | Driver + engineer TUI |
| **Target — Replay** | None | N/A (stored observation) | Reproducibility |
| **Today — `tui_dashboard`** | None (UDS observation only) | Emulator CAN `0x100` | Observer TUI |
| **Gateway headless** | Gateway process | Opt-in `auto_power_on` | CI / scripts |

Implementation tasks: `docs/TODO-twin-lifecycle.md`.

### 16.6 Quit (`q`) — current behaviour

**`q` / Esc** ends the Dashboard loop, closes the UDS connection, and exits the TUI process.
The **Gateway twin keeps running** (Phase 6 split). Emulators and actuators on `vcan0` are
unaffected.

- **Target (TL-6 / Phase 10):** Gateway Stop / disband sequence when the twin host itself exits.
- **Today:** Dashboard quit is observation-only teardown.
