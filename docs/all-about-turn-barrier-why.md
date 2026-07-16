# All About Turn-Barrier — Why

## How Wiper Startup Failure Is Detected Without an ACK Timer

### The Short Answer

The **turn-barrier** detects Wiper startup failure via the **brain-level tell-back timer**,
not via an assembly-level ACK protocol. There are two independent timer layers, and they
must not be confused:

| Timer | Where | What it waits for | Wiper has it? |
|-------|-------|-------------------|---------------|
| **ACK timer** | Inside `HeadlampActor` (`AckWaitElapsed`) | Hardware ACK frame from the physical headlamp via CAN | ❌ No — Wiper has no hardware ACK by design |
| **Tell-back timer** | Inside `VirtualCarActor` (the Brain) | `ZoneReady` reply from the twinlet actor back to the Brain | ✅ Yes — fires `ZoneTellBackTimeout { zone_id: Wiper, turn_id, tell_attempt }` |

Wiper does not need an ACK timer because its actuation model has no hardware handshake:
`StartWiping`/`StopWiping` are fire-and-forget CAN commands. But the **tell-back timer**
is a brain-level mechanism that is always armed whenever the Brain sends *any* zone message
to *any* twinlet — including `BecomeOn`/`BecomeOff` lifecycle tells, and `WiperMessage::Start`/`Stop`
event tells. It is assembly-agnostic.

### Walkthrough: Wiper Fails to Start During PreparingToStart

#### Step 1 — `StartAssemblies` fans out per-assembly barriers

When `PowerOn` fires the FSM transition `Off → PreparingToStart({Headlamp, Wiper})`, the
`output()` function emits `DomainAction::StartAssemblies([Headlamp, Wiper])`. The actor's
`apply_committed_quiescence` loops over the assembly list and for each assembly:

1. Allocates a `turn_id` (monotonic, sequential)
2. Constructs `ZoneMessage::Wiper(WiperMessage::BecomeOn)`
3. Calls `tell_wiper_zone(...)` — sends the message to `WiperActor`'s mailbox
4. Creates a `TellBackWait { turn_id, tell_attempt: 0, retries_remaining: 2 }`
5. Arms a **tell-back timer** via `brain.send_after(500ms, ZoneTellBackTimeout{zone_id: Wiper, ...})`
6. Pushes a `TurnBarrier` with `Wiper` in `pending` onto `barrier_queue`

Two barriers land on the queue: Headlamp (lower `turn_id`, at the front), Wiper (higher
`turn_id`, behind it). Each carries `FsmEvent::AssemblyZoneReady(assembly_id)` as its
embedded event.

#### Step 2 — The timer fires (Wiper never replied)

If `WiperActor` has crashed, deadlocked, or is simply too slow, the Brain never receives
`ZoneReady { zone_id: Wiper, turn_id: N+1 }`. After 500ms the tell-back timer fires:

```
TwinMessage::ZoneTellBackTimeout {
    zone_id: Wiper,
    turn_id: N+1,
    tell_attempt: 0,
}
```

The Brain's `handle()` routes this to `on_zone_timeout()`, which finds the barrier by
`turn_id` and calls `barrier.act_on_zone_timeout(Wiper, 0)`. The barrier checks
`wait.retries_remaining`: it is 2 (> 0), so the outcome is `Retry { next_attempt: 1 }`.

The Brain re-tells the same `BecomeOn` message (stored in `barrier.zone_messages`) and
arms a new timer — this time with `tell_attempt: 1`.

#### Step 3 — Retry budget exhausted → GaveUp

After a second timeout (`tell_attempt: 1`), `retries_remaining` drops to 0. The third
timeout (`tell_attempt: 2`) finds `retries_remaining == 0` and returns `GaveUp`.

Total wall-clock wait: **3 × 500 ms = 1.5 seconds** (3 × 50ms in tests).

#### Step 4 — Synthetic reply injected

On `GaveUp`, the Brain calls `Self::synthetic_reply_for(ctx, AssemblyId::Wiper)`, which
delegates to `synthetic_unresponsive_wiper_reply(&ctx.wiper)`:

```rust
pub fn synthetic_unresponsive_wiper_reply(wiper_ctx: &WiperContext) -> WiperZoneReply {
    WiperZoneReply {
        ctx: wiper_ctx.clone(),                        // still Off — never transitioned
        outcomes: vec![WiperOutcome::LogWarning(format!(
            "wiper tell-back unresponsive after {ZONE_TELL_BACK_ATTEMPT_COUNT} tell attempts"
        ))],
    }
}
```

This synthetic reply carries the **current** `WiperContext` (state: `Off`) and a
`LogWarning` outcome. It is injected via `barrier.act_on_zone_reply(Wiper, synthetic_reply)`,
which removes `Wiper` from `pending` and aborts any remaining timer.

> **Note:** As of this writing, `WiperOutcome::LogWarning` exists in `zone_tell_back.rs`
> but may not yet be plumbed through `outcome_map.rs`. That is a separate gap (Tier 1 item
> #3 in the design plan). The machinery to *produce* the warning is already in place — the
> `outcome_map` just needs to map it to `DomainAction::LogWarning`.

#### Step 5 — Barrier drains; FSM reaches Idle

With `pending` now empty, `barrier.is_complete()` returns `true`. The drain loop pops it.
The embedded event `FsmEvent::AssemblyZoneReady(Wiper)` is committed through
`commit_resolved_turn` → `run_to_quiescence`. The FSM transition fires:

```
PreparingToStart({Wiper}) + AssemblyZoneReady(Wiper) → Idle
```

The Brain reaches `Idle` regardless of whether Wiper actually started. The `BTreeSet`
countdown mechanism has no concept of "failed assembly" — the set empties the same way
whether the reply was real or synthetic.

### How Do We Know It Failed? The Ledger Signal

The `RawTransitionRecord` for the `AssemblyZoneReady(Wiper)` hop captures:

| Field | Value |
|-------|-------|
| `event` | `AssemblyZoneReady(Wiper)` |
| `old_state` | `PreparingToStart({Wiper})` |
| `next_state` | `Idle` |
| `old_ctx.wiper.state` | `Off` |
| `current_ctx.wiper.state` | `Off` |

A successful startup would show `current_ctx.wiper.state = Ready`. The discrepancy —
`Idle` state with `wiper.state = Off` in the same row — is the machine-readable evidence
that startup silently failed. Additionally, if `LogWarning` is plumbed through, the
diagnostic sink emits a `⚠️ Warning` message.

---

## How `run_to_quiescence()` (twin_turn.rs:86) Upholds Design Correctness

### What `run_to_quiescence` Does

```rust
pub fn run_to_quiescence(
    initial_state: &FsmState,
    initial_ctx: &VehicleContext,
    ingress: &FsmEvent,
    now: Instant,
    zone_replies: &ZoneReplies,
) -> QuiescentResult {
```

Its job: given one external event and the zone replies it triggered, resolve the FSM to a
**stable cut** — a state where no further internal events are pending. It does this by
looping (up to `MAX_QUIESCENCE_HOPS = 8`):

1. **First hop**: merge zone replies into context via `zone_turn()` (L1), then run FSM
   `step()` (L2). Collect actions.
2. **Internal detection**: call `detect_internal_after_hop()` which examines the new state
   and context. If an internal event is warranted (e.g. `TimerTick` after entering
   `ExtremeOperationWarning`, or `LightingUnsafe` from a headlamp state mismatch), push it
   onto the event queue.
3. **Subsequent hops**: zone replies are `ZoneReplies::default()` (empty — no new external
   tell-backs arrive during internal processing). Each internal hop applies `step()` with
   no zone merge.
4. **Loop** until no more internal events are detected or `MAX_QUIESCENCE_HOPS` is hit.

### Why This Upholds Correctness

**1. Causal consistency between zones and FSM state**

Without `run_to_quiescence`, zone replies would be merged into `VehicleContext` at a
different logical time than the FSM `step()`. Consider:

- Event: `RainsStarted` → zone tell to Wiper → Wiper replies with `WiperContext { state: Running }`
- The zone reply must be merged into `VehicleContext` **before** the FSM step evaluates
  whether to transition, because the FSM transition may depend on `ctx.wiper.state`.
- `apply_external_hop` (line 152) does exactly this: `zone_turn(...)` first, then
  `step(...)`. The merge and the step are atomic within one hop.

**2. Internal events are resolved immediately, not queued for later**

If the FSM transitions into a state that has an immediate internal consequence (e.g.
`ExtremeOperationWarning` arms a timer that produces `TimerTick`, or a detector notices
`wiper.state == Off` while `current_state == Idle` and synthesises `Internal(WiperStartFailed)`),
`run_to_quiescence` processes that internal event **before** returning to the actor's
`handle()` loop. This means the ledger records a **complete multi-hop turn** as an atomic
unit — no intermediate state is externally visible.

Without quiescence, the actor would return to its mailbox after the first hop, leaving the
FSM in a transient state. A subsequent external event could arrive and interleave with the
internal event, violating causality.

**3. Zone replies are consumed only on the first hop**

The `zone_replies` parameter is passed to `apply_single_hop` only when `is_first == true`.
Internal hops receive `ZoneReplies::default()`. This prevents a zone reply from being
double-counted or applied to the wrong logical hop.

**4. The `MAX_QUIESCENCE_HOPS` bound prevents infinite loops**

If a detector or FSM transition produces an internal event that leads back to the same
state (a self-loop), the quiescence loop would spin forever. The 8-hop bound cuts off
infinite regress and returns a `QuiescentResult` with whatever hops were accumulated.
This is a safety net — well-designed FSMs should not self-loop internally without
progress.

### Concrete Example: `RainsStarted` During Normal Operation

1. Event `RainsStarted` arrives at the Brain
2. `begin_fsm_turn` creates a `TurnBarrier` with `Wiper` in `pending`
3. Tell `WiperMessage::Start` to `WiperActor`
4. Wiper replies `ZoneReady { zone_id: Wiper, reply: WiperZoneReply { ctx: Running, outcomes: [StartWiping] } }`
5. `on_zone_ready` stores the reply, barrier becomes complete
6. Drain loop pops barrier, calls `commit_resolved_turn` → `run_to_quiescence`
7. **First hop**: `zone_turn` merges the `WiperZoneReply` — Wiper context becomes `Running`,
   outcome `StartWiping` is collected. Then `step()` is called with the updated context.
   The FSM in `Idle` with `RainsStarted` and Wiper=`Running` evaluates transitions...
   (In practice `RainsStarted` is not a transition trigger for the Brain FSM — it is a
   zone-directed event that only mutates L1 context. But the same mechanism applies for
   events that *do* trigger transitions.)
8. `detect_internal_after_hop` checks the result — no internal event needed
9. `QuiescentResult` returned with one hop; `merged_actions()` yields `StartWiping`
10. `apply_committed_quiescence` maps `StartWiping` → `DomainAction::RequestWiperStart`
    → `ActuationCommand::StartWiper` → CAN frame

More complex case: `PowerOn` → `PreparingToStart` → two `AssemblyZoneReady` barriers drain
→ each commits through `run_to_quiescence` — the first produces no internal events (the
intra-preparing self-loop has no detectors that fire), the second transitions to `Idle`
which also produces no internal events. But the *actor-level* sequencing (two barriers in
order) is handled by the **drain loop**, not by `run_to_quiescence`. Quiescence resolves
each individual commit atomically; the drain loop ensures causal ordering *across* commits.

---

## What Fallout Would Occur Without Barriers

### The Core Problem: Out-of-Order Replies Corrupt the Ledger

Without `TurnBarrier`, every `ZoneReady` reply would be committed immediately when it
arrives — regardless of what other events are in flight. The ledger would record state
transitions in **mailbox arrival order**, which is not guaranteed to match **event ingress
order**.

### Fallout Scenario 1: Headlamp Reply Arrives After a Later Event

Timeline (no barrier):

```
t=0  FsmEvent::PowerOn arrives → Brain enters PreparingToStart
     → Tell BecomeOn to HeadlampActor (turn A)
     → Tell BecomeOn to WiperActor   (turn B)
t=1  FsmEvent::RainsStarted arrives (while still in PreparingToStart)
     → zone_message_for_event returns None (PreparingToStart gate)
     → event is discarded (applied: false)
t=2  ZoneReady(Headlamp, turn A) arrives → Brain commits AssemblyZoneReady(Headlamp)
     → ledger: PreparingToStart({H,W}) → PreparingToStart({W})
t=3  ZoneReady(Wiper, turn B) arrives → Brain commits AssemblyZoneReady(Wiper)
     → ledger: PreparingToStart({W}) → Idle
```

This case works fine **because** `RainsStarted` was gated by the `PreparingToStart` check
in `zone_message_for_event`. The barrier is not needed here — the gate is in the
zone-routing function.

### Fallout Scenario 2 (Illustrative): Two Consecutive Zone-Directed Events

*Note: This scenario uses a single assembly (Wiper) for simplicity. Wiper's actor mailbox guarantees reply order = send order for the same assembly, so barriers aren't strictly needed for causal ordering here. The **real** causal-inversion risk involves multiple assemblies (see Design Deep-Dive below).*

```
t=0  FsmEvent::RainsStarted arrives (Idle state)
     → Tell WiperMessage::Start to WiperActor (turn 1)
     → Barrier[1] pushed: pending={Wiper}, event=RainsStarted
t=1  FsmEvent::RainsStopped arrives (Idle state) — before Wiper replied to turn 1
     → Barrier[2] pushed: pending={Wiper}, event=RainsStopped
     Barrier queue: [ Barrier[1] pending{Wiper} | Barrier[2] pending{Wiper} ]
t=2  ZoneReady(Wiper, turn 2) arrives — Wiper replies to RainsStopped first!
```

**Without barriers**, this ZoneReady would be committed immediately. The ledger records:

```
Idle + RainsStopped → Idle (wiper.state = Ready→Ready, but Stop is processed before Start!)
```

The ledger order is **RainsStopped before RainsStarted** — causal inversion. The physical
world saw rain start then stop, but the digital ledger shows stop before start.

**With barriers**, `on_zone_ready` stores the reply in `Barrier[2]` (found by `turn_id`),
but the drain loop checks the front of the queue: `Barrier[1]` is still pending (Wiper
hasn't replied to turn 1). The drain loop **stalls**. When `ZoneReady(Wiper, turn 1)`
finally arrives, it lands in `Barrier[1]`, which becomes complete, drains, and commits
`RainsStarted`. Then `Barrier[2]` is at the front, complete, and drains — committing
`RainsStopped`. Ledger order: `RainsStarted → RainsStopped` ✅

### Fallout Scenario 3: StartAssemblies Ordering

Without barriers, if `WiperActor` replies `ZoneReady` before `HeadlampActor` during
`PreparingToStart`:

```
t=0  StartAssemblies([Headlamp, Wiper]) from output()
     → Barrier[HL]: pending={Headlamp}, event=AssemblyZoneReady(Headlamp)
     → Barrier[WP]: pending={Wiper}, event=AssemblyZoneReady(Wiper)
     Queue: [ HL pending | WP pending ]
t=1  ZoneReady(Wiper, turn=WP) arrives before Headlamp
```

**Without barriers**: Wiper's reply is committed immediately → ledger shows
`PreparingToStart({H,W}) → PreparingToStart({W})`. Then Headlamp replies → `PreparingToStart({W}) → Idle`.
Order is correct *in this case* because the `BTreeSet` countdown is idempotent. But...

**With barriers**: Wiper's reply is stored in `Barrier[WP]` but the drain loop stalls
because `Barrier[HL]` is at the front and still pending. Headlamp's reply completes
`Barrier[HL]` → drains → commits `AssemblyZoneReady(Headlamp)` → `PreparingToStart({H,W}) → PreparingToStart({W})`.
Now `Barrier[WP]` is at the front, complete, drains → `PreparingToStart({W}) → Idle`.
Same end result — but the ledger order is guaranteed to match **turn_id assignment order**
(Headlamp before Wiper), which matches `ALL_ASSEMBLIES` iteration order.

The difference matters for **deterministic replay**: if the ledger is ever used to replay
the system's state, barrier-ordered commits guarantee that the replay follows the same
sequence every time, regardless of scheduler nondeterminism.

### Fallout Scenario 4: Barrier Protects Against Stale/Retried Messages

Without the `tell_attempt_matches` guard inside the barrier, a late-arriving reply from an
old tell attempt (after a retry) could corrupt state. Example:

```
BecomeOn tell (attempt 0) → timeout → Retry → BecomeOn tell (attempt 1)
ZoneReady(attempt 0) arrives late (scheduler delay)
```

**Without barriers**: the ZoneReady from attempt 0 is applied on top of the already-retried
state. The context may reflect the attempt-0 outcome, which is stale and potentially
inconsistent with the attempt-1 retry logic.

**With barriers**: `on_zone_ready` calls `barrier.tell_attempt_matches(Wiper, 0)`. The
barrier's `zone_waits` for `Wiper` now has `tell_attempt: 1` (advanced by the retry). The
check returns `false`, and the stale reply is silently discarded. ✅

### Summary of Fallout Without Barriers

| Scenario | Consequence Without Barrier |
|----------|---------------------------|
| Two rapid zone-directed events (e.g. RainsStarted then RainsStopped) | Causal inversion in ledger — stop recorded before start |
| Assembly replies arrive out of order during PreparingToStart/Stop | Ledger order depends on scheduler timing, not deterministic |
| Late/duplicate ZoneReady from a superseded tell attempt | Stale context applied on top of retried state |
| Passthrough event queued behind a slow zone reply | Passthrough events can overtake zone-directed events in commit order |
| Deterministic replay from ledger | Impossible — replay would need to reproduce scheduler non-determinism |

The TurnBarrier — together with `run_to_quiescence` — enforces **three ordered layers**:

```
Actor mailbox order → barrier_queue (FIFO) → quiescence hops (internal ordering)
```

This is the ROB (re-order buffer) pattern: the queue absorbs out-of-order replies and
releases them in ingress order. Without it, the system's state machine would be at the
mercy of actor scheduler timing — a correctness bug waiting to happen.

---

---

## Design Deep-Dive: Could the ROB/VecDeque Be a Set?

**Question**: Because the Brain's `handle()` is single-threaded, the moment `commit_resolved_turn` finishes and DomainActions are concretely known — before the next event is popped from the mailbox — the timestamps are monotonically increasing. At that point, could the barrier queue be a `Set`/`HashMap<turn_id, TurnBarrier>` instead of a `VecDeque`?

**Short answer**: It depends on whether a ledger exists:

- **With a ledger** (current design): Only with a **two-phase ledger** — one entry at commit time (DomainActions known, state transition complete) and one that also captures the *arrival order* (the turn_id sequence from `alloc_turn_id`). With the current **single-phase ledger** (state transitions only at drain time), the VecDeque is necessary because state-transition data is only known at drain time, and without head-of-line blocking, barriers could drain in causal-inversion order (see §Concrete counterexample below).

- **Without a ledger** (no transition recording, no replay): **Yes — a Set (keyed by `turn_id`) is sufficient.** The FSM state correctness is already guaranteed by the actor's single-threaded `handle()`. Each `commit_resolved_turn` executes sequentially, reading and writing the same `FsmState`/`VehicleContext` struct, regardless of which barrier drains first. The VecDeque's ordering exists purely to produce a *deterministic ledger sequence* — if no ledger exists, there's nothing to be deterministic about. Barriers drain as soon as they're ready, with zero head-of-line blocking.

### Why the single-phase ledger needs the VecDeque

The ledger's `RawTransitionRecord` contains:

```rust
pub struct RawTransitionRecord {
    pub at: Instant,          // timestamp
    pub event: FsmEvent,
    pub old_state: FsmState,  // ← only known after zone replies
    pub next_state: FsmState, // ← only known after zone replies
    pub old_ctx: VehicleContext,
    pub current_ctx: VehicleContext,
    pub actions: Vec<DomainAction>,
}
```

The fields `old_state`, `next_state`, `old_ctx`, `current_ctx`, `actions` are only known **after** all zone replies arrive and `run_to_quiescence()` resolves. At pop-time you know only `event`.

So a single-phase ledger entry can only be created at **drain-time**, not pop-time. The drain-time timestamp records *when the barrier drained* — not *when the event arrived*. Without head-of-line blocking (VecDeque), two barriers could drain in the wrong order.

### Concrete counterexample: single-phase ledger without VecDeque

```
Mailbox: [UpdateAmbientLux(1), RainsStopped]

t=1 Brain pops UpdateAmbientLux(1)
    → sends AmbientLux(1) to HeadlampActor
    → pushes Barrier[Lux] (pending={Headlamp})
    → try_drain stalls (pending)

t=2 Brain pops RainsStopped
    → sends Stop to WiperActor
    → pushes Barrier[Rain] (pending={Wiper})
    → try_drain stalls (Barrier[Lux] still pending)
```

Now suppose WiperActor replies **before** HeadlampActor (scheduler delay, hardware wait, etc.):

```
t=3 ZoneReady(Wiper, turn=Rain) arrives at Brain mailbox
```

**Without VecDeque** (Set/HashMap based): drain loop iterates all barriers, finds Barrier[Rain] complete, drains it → ledger records `RainsStopped` → then Barrier[Lux] drains → records `UpdateAmbientLux`. **Causal inversion**: `RainsStopped` appears before `UpdateAmbientLux(1)` even though `UpdateAmbientLux` arrived first.

**With VecDeque**: `on_zone_ready` stores the reply in Barrier[Rain]. Drain loop checks the **front**: Barrier[Lux] is still pending. **Stalls**. When Headlamp replies, Barrier[Lux] drains first, then Barrier[Rain] is at the front, complete, drains. Ledger: `UpdateAmbientLux(1) → RainsStopped` ✅

### What if we add a commit-time ledger entry that also carries arrival-order data (two-phase)?

| Phase | When | Data recorded | Timestamp |
|-------|------|--------------|-----------|
| **Phase 1** (arrival ordering) | `alloc_turn_id` in `begin_fsm_turn` | `(turn_id, event, pending_zones)` | `turn_id` sequence (monotonic, allocated before zone send) |
| **Phase 2** (resolution) | Barrier drain, after `commit_resolved_turn` | `(turn_id, old_state, new_state, ctx, actions)` | `Instant::now()` at commit time |

With Phase 1 entries, causal ordering is proven by the `turn_id` allocation order (which happens before the zone message is even sent — see `begin_fsm_turn` at line 361: `let turn_id = runtime_state.alloc_turn_id();`). Phase 2 entries link back via `turn_id`.

In this design, the VecDeque **can** be replaced by a `HashMap<turn_id, TurnBarrier>`. The drain loop drains *any* complete barrier, and the ledger reader reconstructs causal order by sorting Phase-1 `turn_id`s.

### Trade-offs

| Aspect | Single-phase + VecDeque (current) | Two-phase + HashMap/Set |
|--------|----------------------------------|------------------------|
| Number of ledger records | 1 per event | 2 per event |
| Code complexity | Simpler | More complex (two emission points) |
| Head-of-line blocking | Yes — can delay drain | No — barriers drain as soon as ready |
| Causal proof | Structural (queue position) | turn_id allocation order (monotonic, precedes all zone sends) |
| Deterministic replay from ledger | Ledger order == commit order | Need to sort by Phase-1 timestamps |

### When does a single-assembly scenario NOT need barriers?

The `RainsStarted`→`RainsStopped` example in §"Fallout Scenario 2" uses a single assembly (Wiper). As you correctly observed, the Wiper actor's mailbox guarantees `Start` is received before `Stop`, so replies arrive in send order. **For a single assembly with no timeouts or retries, barriers add no causal-ordering benefit.**

Barriers become essential when:

1. **Multiple assemblies** interleave (Headlamp slow, Wiper fast — the most common case)
2. **Retries/timeouts** cause later events' replies to arrive before earlier events' retries complete (stale-tell_attempt protection)
3. **Deterministic replay** from the ledger is needed (barrier order == turn_id allocation order, independent of scheduler)

The single-assembly scenario in the main text was chosen for simplicity of illustration, but the *real* danger is cross-assembly interleaving.

---

## Ledger Completeness: Can a Replay Tool Reconstruct Everything From `PublishedTransitionRecord`?

### What the ledger contains today

Each `PublishedTransitionRecord` captures the *result* of one FSM turn (zero or more quiescence hops):

```rust
pub struct PublishedTransitionRecord {
    pub car_identity: String,
    pub session_start_unix_nanos: u128,
    pub record_seq: u64,
    pub recorded_at_unix: Duration,
    pub event: PublishedFsmEvent,
    pub old_state: PublishedFsmState,
    pub next_state: PublishedFsmState,
    pub old_ctx: PublishedVehicleContext,
    pub current_ctx: PublishedVehicleContext,
    pub actions: Vec<PublishedDomainAction>,
}
```

A replay tool reading these entries sees a sequence of state transitions:

```
[T=1] Idle + RainsStarted → Idle (wiper.state = Off→Running, actions=[StartAssemblies([Wiper])])
[T=2] Idle + RainsStopped  → Idle (wiper.state = Running→Ready, actions=[StopAssemblies([Wiper])])
```

**Question**: Can a tool reconstruct *exactly what happened* inside the Digital Twin from this data?

### What the ledger captures ✅

1. **The FSM path**: Every `event`→`old_state`→`next_state` transition, with full context deltas.
2. **The wall-clock timing**: `recorded_at_unix` gives exactly when the transition committed.
3. **The ordering**: `record_seq` is strictly monotonic — no two entries share the same seq.
4. **The domain actions**: What the FSM decided to do (start/stop assemblies, log warnings, etc.).

### What the ledger does NOT capture ❌

There are **five categories of information** that the current ledger omits, each of which would make replay incomplete or ambiguous.

---

#### Gap 1: Zone Replies Are Lost

The ledger records the *result* of applying zone replies, but not the *replies themselves*.

**Scenario**: A headlamp zone replies `ZoneReady(Headlamp, turn_id=5, reply=Ack(Ok))`. The ledger shows:

```
Idle + FrontHeadlampActuationIncomplete → Idle (headlamp.state = Requested→On, actions=[RequestFrontHeadlampOn])
```

But the *reason* the transition succeeded is the `Ack(Ok)` from the zone. For replay, a tool needs to know **what zone replies were provided** to `run_to_quiescence` — not just the resulting state.

**Why this matters for replay**: If you replay without knowing the zone replies, you can't reproduce the internal `zone_turn()` merge step. The replay tool must simulate the zone replies — but it has no data on what those replies contained. Did Headlamp give an `Ack` or a `Nack`? Was it a `ZoneReady` or a synthetic timeout reply? The ledger doesn't say.

---

#### Gap 2: Tell-Back Timeouts Are Invisible

When a zone doesn't reply within `ZONE_TELL_BACK_WAIT`, the Brain fires a `ZoneTellBackTimeout`, retries, and eventually injects a synthetic reply. The ledger shows the eventual transition, but **not that a timeout occurred**.

**Scenario**: Wiper crashes and doesn't reply. After 3 retries (500ms each), the Brain synthesizes a reply. Ledger:

```
Idle + RainsStarted → Idle (wiper.state = Off→Off, wiper.status = Unresponsive, actions=[])
```

The transition shows `wiper.status = Unresponsive`, so a human can infer something went wrong. But for replay, the tool needs to know:
- Was the reply a real `ZoneReady` or a synthetic fallback?
- How many retries happened?
- What was the actual zone reply (if any arrived after the synthetic one)?

A replay tool replaying this event would call `run_to_quiescence` with `ZoneReplies::default()` (no replies) — which is **not the same** as what actually happened (retries + synthetic reply injected at a specific wall-clock offset). The replayed state would diverge from the ledger's state.

---

#### Gap 3: Retries Are Invisible

Related to Gap 2: even if a zone eventually replies successfully, retries before the final reply are not recorded.

**Scenario**: Wiper is slow. Tell 1 times out after 500ms. Tell 2 (retry) succeeds. The ledger records:

```
Idle + RainsStarted → Idle (wiper.state = Off→Running, actions=[])
```

The tool sees: "RainsStarted → wiper went from Off→Running. Good, the Start message succeeded."

But it **doesn't know** that the first tell attempt failed and a retry was needed. For strict replay, the tool would need to:
1. Send the first tell to the zone.
2. Wait 500ms (simulate the timeout).
3. Send the retry.
4. Receive the reply.

Without knowing retry count, the replay can't reproduce the correct temporal behavior.

---

#### Gap 4: Barrier Queue Order / Causal Inversion Is Not Recordable

The ledger entries are emitted in the order barriers drain. But as established above, without the VecDeque, barriers could drain in an order different from event arrival. The *current* code has the VecDeque, so this doesn't happen. But the ledger itself has **no field** that records "this event arrived before that event."

**Scenario**:
```
Mailbox: [UpdateAmbientLux(1), RainsStopped]
Ledger: EventA(Lux) → EventB(Rain)   (correct order)
```

This works because the VecDeque enforces it. But if the VecDeque were removed and the drain order was different, the ledger would show:

```
Ledger: EventB(Rain) → EventA(Lux)   (wrong order — but tool can't detect this)
```

The tool would faithfully replay `RainsStopped` before `UpdateAmbientLux` and reach the wrong final state. **No field in the current `PublishedTransitionRecord` captures arrival order** — only commit order.

---

#### Gap 5: `FsmEvent::TimerTick` and Spontaneous Zone Events Are Mixed

TimerTick events and `ZoneSpontaneous` events (e.g., Headlamp detecting a hardware actuation completion) are events that enter the Brain's mailbox just like any other. But the ledger records them as FSM transitions, **not as actor mailbox events**.

**Scenario**: A `ZoneSpontaneous` fires (from Headlamp) that the FSM interprets as `FrontHeadlampActuationIncomplete`. The ledger shows:

```
DrivingInDark + FrontHeadlampActuationIncomplete → DrivingInDark (headlamp.state=On→Off)
```

The tool sees an FSM event, but doesn't know:
- Did this originate from a spontaneous zone message, a timer tick, or a `ZoneReady` from a previous barrier?
- Which assembly produced the spontaneous message?

Without this provenance, the replay tool can't determine whether to inject the event from a zone or from the environment.

---

### Summary: What a Complete Replay Ledger Would Need

| # | Data | Current `PublishedTransitionRecord` | Needed for replay? |
|---|------|----------------------------------|-------------------|
| 1 | FSM transition (event + old/new state + ctx) | ✅ `event`, `old_state`, `next_state`, `old_ctx`, `current_ctx` | Yes |
| 2 | Wall-clock timing of commit | ✅ `recorded_at_unix`, `session_start_unix_nanos` | Yes |
| 3 | Commit order | ✅ `record_seq` | Yes |
| 4 | Domain actions | ✅ `actions` | Yes |
| 5 | **Zone replies that caused the transition** | ❌ Not recorded | Yes — otherwise replay can't reproduce `zone_turn()` |
| 6 | **Tell-back timeouts & retry count** | ❌ Not recorded | Yes — for temporal replay accuracy |
| 7 | **Whether the zone reply was real or synthetic** | ❌ Not recorded | Yes — synthetic replies bypass normal actor behavior |
| 8 | **Event arrival order (not just commit order)** | ❌ Not recorded | Yes — without VecDeque, causal inversion is invisible |
| 9 | **Event provenance (mailbox source)** | ❌ Not recorded | Yes — distinguishes `ZoneSpontaneous` from `Fsm(timer)` from `ZoneReady` |
| 10 | **Which assemblies were told and their messages** | ❌ Not recorded | Yes — links barrier state to ledger entry |

### Counter-Cases Where Replay Fails

Here are concrete scenarios where the current ledger is insufficient for a replay tool:

#### Counter-Case A: Headlamp Nack vs. Timeout

```
Scenario: Driver turns on high beam. Headlamp zones replies Nack (hardware fault).
Ledger: Idle + FrontHeadlampActuation → Idle (headlamp.state = Requested→Off, actions=[])
```

The tool sees the transition and can replay the FSM step. But:

- **What actually happened**: The zone returned `Nack(HardwareFault)`, so `zone_turn()` merged the failure context, and the FSM stayed in `Idle` with a warning.
- **What replay would do without zone replies**: `run_to_quiescence` with `ZoneReplies::default()` returns a *different* `zone_turn()` result — the zone default is to succeed (no fault). Replay would transition `headlamp.state = Requested→On`, **diverging from the ledger**.

The ledger shows the *outcome* (Off), but replay without zone replies would produce a *different outcome* (On). The tool can't detect this mismatch unless it compares the replayed outcome against the ledger's `next_state` — and then what? It has no zone-reply data to correct the divergence.

#### Counter-Case B: Wiper Unresponsive After Retries

```
Scenario: Wiper doesn't reply. 3 retries at 500ms intervals. Synthetic reply injected.
Ledger: Idle + RainsStarted → Idle (wiper.state = Off→Off, wiper.status = Unresponsive)
```

Tool replays: calls `step()` with `RainsStarted` → output says `StartAssemblies([Wiper])`. The tool would then simulate telling Wiper... but the original code ran 3 retries with 500ms waits before synthesizing a reply. The tool has no data about:
- The retry count
- The wait duration
- The synthetic reply content
- Whether a real reply ever arrived after the synthetic fallback

Without this, the tool can't reproduce the temporal behavior. It would either skip the wait (making replay unrealistically fast) or use arbitrary wait values (making replay non-deterministically different from the original).

#### Counter-Case C: Interleaved Multi-Assembly Events

```
Scenario (without VecDeque):
Mailbox: [UpdateAmbientLux(1), RainsStopped]
Barrier[Lux] pending={Headlamp}, Barrier[Rain] pending={Wiper}
Wiper replies first. Set-drain commits RainsStopped before UpdateAmbientLux(1).

Ledger (Set-order):
[T=1] Idle + RainsStopped → Idle (wiper.state = Running→Ready)
[T=2] Idle + UpdateAmbientLux(1) → Idle (headlamp.target_lux = 1)
```

The tool replays in ledger order: `RainsStopped` first, then `UpdateAmbientLux(1)`. The final state is:

```
wiper.state = Ready, headlamp.target_lux = 1
```

**But what actually happened**: `UpdateAmbientLux(1)` arrived first (causally), set `headlamp.target_lux = 1`, and *then* `RainsStopped` arrived. The FSM processed them in arrival order. The *actual* final state is the same (both are independent state dimensions), so in this case it's fine. But:

- If the events had overlapping state dimensions (e.g., both modify `headlamp.state`), causal order would matter for correctness.
- The tool **cannot know** which events are independent vs. overlapping — it only has commit-order data.

The current code has the VecDeque precisely to prevent this. But the ledger itself doesn't prove the order was correct — the VecDeque does, structurally.

### The Minimum Viable Ledger for Replay

A replay tool needs, at minimum, the **inputs** to `run_to_quiescence`, not just the **outputs**:

```
For each turn:
  - turn_id (for causal ordering)
  - event (the FSM event that started this turn)
  - zone_replies: Vec<(AssemblyId, ZoneReply, tell_attempt, was_timeout)> — the actual zone inputs
  - retry_count: u32
  - arrival_order_hint (turn_id sequence from alloc_turn_id)
  - source: enum { MailboxFsm, MailboxZoneReady, MailboxZoneSpontaneous, MailboxTimerTick }
```

With these inputs, a replay tool can:
1. Call `run_to_quiescence(initial_state, initial_ctx, event, now, zone_replies)` for each turn.
2. Verify the output matches the recorded `PublishedTransitionRecord`.
3. If output diverges (e.g., due to a code bug), flag the ledger as corrupted or the code as changed.

Without these inputs, replay is guesswork — the tool can read `next_state` but can't *compute* it independently, which defeats the purpose of replay.

---

## Appendix: Key Code Locations

| What | File | Line |
|------|------|------|
| `run_to_quiescence()` | `crates/common/src/twin_runtime/twin_turn.rs` | 86 |
| `TurnBarrier::act_on_zone_timeout()` | `crates/common/src/twin_runtime/turn_barrier.rs` | ~193 |
| `TurnBarrier::act_on_zone_reply()` | `crates/common/src/twin_runtime/turn_barrier.rs` | ~168 |
| `TurnBarrier::tell_attempt_matches()` | `crates/common/src/twin_runtime/turn_barrier.rs` | ~117 |
| `synthetic_unresponsive_wiper_reply()` | `crates/common/src/twin_runtime/zone_tell_back.rs` | ~72 |
| `zone_message_for_event()` | `crates/common/src/twin_runtime/zone_turn.rs` | ~52 |
| `begin_fsm_turn()` | `crates/common/src/twin_runtime/controller/virtual_car_actor.rs` | ~359 |
| `try_drain_barrier_queue()` | `crates/common/src/twin_runtime/controller/virtual_car_actor.rs` | ~496 |
| `apply_committed_quiescence()` | `crates/common/src/twin_runtime/controller/virtual_car_actor.rs` | ~530 |
| `ZONE_TELL_BACK_MAX_RETRIES` | `crates/common/src/twin_runtime/constants.rs` | (2) |
| `ZONE_TELL_BACK_WAIT` | `crates/common/src/twin_runtime/constants.rs` | (500ms prod, 50ms test) |
