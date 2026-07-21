# Structured diagnostics design

**Date:** 2026-07-18  
**Status:** Approved  
**Related:** [Phase 5 Dashboard presentation](2026-07-18-phase-5-dashboard-presentation-design.md),
[Phase 3 observation capture](2026-07-17-phase-3-observation-capture-design.md),
[`docs/PHASES.md`](../../PHASES.md)

## Goal

Replace icon-and-prose diagnostic messages with **structured facts** (`DiagnosticKind`) so
that Twin emissions stay receiver-agnostic. Formatting (plain text today, Ratatui-compatible
icons later) is always the consumer’s job — Dashboard, CLI, contracts, capture, future Zenoh.

This design does **not** add wiper actuator ACK/NACK, publish new ledger context fields by
itself, split Gateway/Dashboard processes, or introduce Zenoh transport.

## Why

Today two paths bake presentation into emission:

1. **Twin** — `DiagnosticRecord.message` strings such as
   `[identity]: ✅💡 Front headlamp ON confirmed.`
2. **Gateway** — `println!` lines such as
   `[actuation-can-ingress session=… seq=…]: ✓ ACK_ON` on stdout while the Dashboard owns
   the TTY (stderr alternate screen), which corrupts the UI.

Principle: **emit more small pieces of data if needed; formatting is receiver-side.**
A multi-byte display string is not a wire contract and fights Zenoh later.

## Approach (chosen)

**Approach A — single stream, structured `kind`:** keep one `DiagnosticRecord` channel;
replace the prose blob with `DiagnosticKind`. Receivers filter and format.

Rejected alternatives:

| Approach | Why not |
|----------|---------|
| B — parallel typed channel beside free-form diagnostics | Two drains/capture paths; Dashboard complexity |
| C — keep `message: String`, strip icons only in the view | Still emitter-formatted; wrong for Zenoh |

## Stream vs Observer

`DiagnosticKind` is the **full fact vocabulary**. The stream carries **more than any single
observer surface** may want to show.

- Capture, contracts, engineer tools, and future Zenoh subscribers select what they need.
- A driver Notice (Observer) **must filter and format** — e.g. hide `TimerTick`, ignore
  headlamp happy-path (read zone/context instead), show headlamp lines only when actuation
  is unconfirmed.
- Code comments on `DiagnosticKind` / `DiagnosticRecord` must state this explicitly so
  contributors do not “help” by omitting noisy-but-useful variants from the enum.

## What belongs where

| Concern | Channel | Notes |
|---------|---------|--------|
| FSM transition, contexts, domain actions | **Transition ledger** | Not duplicated on diagnostics |
| Standing assembly status (headlamp on, wiper running, rain) | **Published vehicle / zone context** on ledger | Dashboard status lines; wiper publish remains a known gap / TODO |
| Rain ↔ wiper **policy proof** (causal edge) | **Diagnostics** | `RainChanged` + `WiperMotionChanged` for contracts and observers that care |
| Headlamp **failure to confirm** | **Diagnostics** | Only when Twin concludes actuator did not confirm |
| Headlamp happy-path ACK | **Not a diagnostic** | Observer sees zone/context change |
| Gateway wire trace (`session` / `seq` / ✓ ACK) | **Not Twin diagnostics** | Local adapter or drop under Dashboard; not the observation contract |
| TimerTick / Boot | **Diagnostics** | Keep in structure; Observer Notice drops TimerTick |

## Live record shape

```rust
pub struct DiagnosticRecord {
    pub level: DiagnosticLevel,
    pub source: &'static str,
    pub kind: DiagnosticKind,
    pub session_started_at: UnixTimestamp,
    pub recorded_at: UnixTimestamp,
}

/// Structured diagnostic facts emitted by the Twin.
///
/// This stream is **wider than any single observer surface**. Variants exist so
/// capture, contracts, engineer tools, and future Zenoh subscribers can select
/// what they need. A driver Notice (or similar) MUST filter and format — e.g.
/// hide `TimerTick`, ignore headlamp success (zone/context), and show headlamp
/// lines only when actuation is unconfirmed.
///
/// Emit facts only: no icons, identity prefixes, or display sentences in payloads.
/// Presentation is always receiver-side.
pub enum DiagnosticKind {
    /// Free-form only when no stable variant exists yet.
    Text { text: String },

    Boot,

    /// Heartbeat / liveness fact. Keep in the stream; Observer Notice should drop it.
    TimerTick,

    /// Twin concludes the actuator did not confirm the requested headlamp action
    /// (timeout or negative ack). Confirmed happy-path ACK is NOT a diagnostic —
    /// observers read zone/context for success.
    /// TODO: may later be encased in zone tell-back; keep as diagnostic until then.
    HeadlampActuationUnconfirmed {
        on: bool, // requested direction
        cause: FrontHeadlampIncompleteCause, // TimedOut | NegativeAck
    },

    /// Rain policy input changed (fact for rain↔wiper proof).
    RainChanged { raining: bool },

    /// Wiper motion changed (fact for rain↔wiper proof). Not an actuator ACK.
    WiperMotionChanged { wiping: bool },

    ActuationFailure { action: String, error: String },
    TransitionSinkFull,
    TransitionSinkClosed,
}
```

### Explicitly out of `DiagnosticKind`

- **`StateTransition { next, speed, … }`** — already on the transition ledger.
- **`HeadlampCommandConfirmed`** — success is zone/context, not Observer diagnostics.
- **`WiperCommandConfirmed` / wiper ACK** — no ACK protocol on the wiper bus today; do not invent one in the enum.
- **Identity prefixes inside payloads** — vehicle identity already rides on observation
  envelopes; live in-process consumers that need it should take it from session/runtime
  metadata, not from every diagnostic string.

## Headlamp rules

1. Observer of diagnostics is told **only** when Twin says the actuator did **not** confirm
   the action → `HeadlampActuationUnconfirmed`.
2. Confirmed ON/OFF is **not** emitted on the diagnostic stream.
3. Icons (`✅`, `✓`, etc.) never appear in Twin payloads; optional Ratatui icons later are
   view-layer only on failure (or other filtered) lines.
4. **TODO:** headlamp elements may later be encased in Zone data; until then keep the
   unconfirmed diagnostic variant.

## Rain ↔ wiper proof

Diagnostics should prove the policy coupling:

- rain becomes true → wiper becomes active  
- rain becomes false → wiper stops  

via `RainChanged` / `WiperMotionChanged` (ordering asserted in contracts).

Wiper **standing status** for the Dashboard `Wipers:` line remains published **zone/context**
(separate TODO if not yet durable on the ledger DTO). Missing wiper ACK variants in
`DiagnosticKind` is correct; the gap for the status line is context publish, not the enum.

## Gateway console

- Stop treating formatted ACK `println!` as part of the Twin/observation story.
- Under Dashboard (in-process TTY owner), do not write those lines to the shared terminal.
- If a console mode remains for standalone Gateway, it is a **local formatter** over the same
  facts (or over ingress metadata), not a second source of truth and not a big display string
  on the diagnostic record.

## Observation / archive

- Phase 3 JSONL today stores `message: String` on `DiagnosticPayloadV1`.
- Introducing `kind` is a **schema bump** (or additive optional field with deprecated
  `message` projection during migration).
- Prefer archiving the tagged `kind` union; do not treat a display string as the long-term
  wire contract.
- Exact versioning mechanics are an implementation-plan detail; compatibility tests must
  cover the bump.

## Dashboard / Phase 5 bridge

Until Twin emits `kind`:

1. **View-only:** strip icons and identity noise from driver Notice; map known prose to plain
   sentences (e.g. `Notice: Info — Front headlamp ON confirmed.` only while legacy messages
   still exist — prefer silence on happy-path ACK once Twin stops emitting them).
2. **After Twin migration:** Notice filters `DiagnosticKind` (drop `TimerTick`; show
   `HeadlampActuationUnconfirmed`, rain/wiper proof if desired, real warnings/errors).
3. Ratatui-compatible icons are a later view polish, not an emission concern.

Phase 5’s original “no Twin emission enrichments” constraint is **superseded for diagnostics
structure** by this spec; rain/wiper **context publish** and ROB remain separate follow-ups.

## Non-goals

- Zenoh transport or pub/sub mapping (shape must not block it)
- Wiper actuator ACK/NACK protocol
- Duplicating ledger transitions into diagnostics
- Colour / emoji polish in Twin strings
- Full replacement of every `Text` warning in one change-set (migrate hot paths first:
  headlamp unconfirmed, TimerTick, Boot, rain/wiper proof)

## Success criteria

- Twin diagnostic helpers emit `DiagnosticKind` facts with no icons in payloads.
- Happy-path headlamp ACK does not appear on the diagnostic stream.
- `HeadlampActuationUnconfirmed` is emitted when Twin concludes non-confirmation.
- `TimerTick` remains in the enum/stream; driver Notice does not show it.
- Rain↔wiper coupling is observable via structured kinds (contracts can assert the pair).
- Gateway does not `println!` formatted ACK lines into a Dashboard-owned TTY.
- Observation schema documents how `kind` is archived (bump or additive migration).
- Type-level comments document stream-vs-observer filtering responsibility.

## Implementation order (advisory)

1. Spec approval → implementation plan.
2. Introduce `DiagnosticKind` + migrate emit helpers (headlamp unconfirmed, TimerTick, Boot).
3. Dashboard filter/format over `kind`; strip legacy icon prose bridge.
4. Rain/wiper proof kinds + contracts.
5. Observation schema migration.
6. Silence/redirect Gateway ACK console under Dashboard.
7. Follow-ups: zone-encased headlamp TODO, wiper context publish for `Wipers:` line, Ratatui icons.
