# Structured Diagnostics Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace prose/icon `DiagnosticRecord.message` with structured `DiagnosticKind` facts, filter them at Observer surfaces, silence Gateway ACK `println!` under Dashboard, and archive `kind` in observation schema v2.

**Architecture:** Twin emits `DiagnosticRecord { level, source, kind, … }` only. Ledger keeps transitions/context. Dashboard Notice filters (drop `TimerTick`, ignore headlamp success, show unconfirmed + real warnings). Observation bumps `CURRENT_SCHEMA_VERSION` to `2` with a tagged `kind` payload. Gateway ingress console logging is opt-in and off for Dashboard.

**Tech Stack:** Rust, existing `common` diagnostic sink + `virtual_car_actor`, `observation` JSONL schema, `tui_dashboard` view layer, `gateway` `TwinRuntimeBuilder`.

## Global Constraints

- Spec: [`docs/superpowers/specs/2026-07-18-structured-diagnostics-design.md`](../specs/2026-07-18-structured-diagnostics-design.md) (Approved).
- Emit facts only: no icons, identity prefixes, or display sentences inside `DiagnosticKind` payloads.
- Stream is wider than Observer UI — keep `TimerTick` in the enum; filter at Notice.
- Headlamp happy-path ACK is **not** a diagnostic; only `HeadlampActuationUnconfirmed`.
- No wiper ACK variants; rain↔wiper proof via `RainChanged` / `WiperMotionChanged`.
- Do not duplicate ledger `StateTransition` onto diagnostics (stop emitting `diag_state_transition`).
- Do not stage or commit unless the user separately requests it.
- Prefer inline execution over subagents unless the user asks otherwise.
- Every task implicitly includes this section.

## File structure

| Path | Responsibility |
|------|----------------|
| `crates/common/src/observation_records/diagnostic/mod.rs` | `DiagnosticKind`, `DiagnosticRecord` with `kind`, constructors, `Display` formats from kind |
| `crates/common/src/observation_records/diagnostic/sink.rs` | Emit helpers → `DiagnosticKind`; remove confirmed helper |
| `crates/common/src/twin_runtime/controller/virtual_car_actor.rs` | Boot / TimerTick / unconfirmed / rain↔wiper; drop confirmed + state-transition diags |
| `crates/common/src/facade.rs` / `lib.rs` | Re-export `DiagnosticKind` |
| `crates/common/src/test/*_contract.rs` | Assert on `kind`, not `message.contains(…)` |
| `crates/observation/src/schema/mod.rs` | `CURRENT_SCHEMA_VERSION = 2` |
| `crates/observation/src/schema/v1.rs` | `DiagnosticKindV1` + payload `kind` (file name kept; version constant is 2) |
| `crates/observation/testdata/golden/v2/…` | Updated golden run (replace v1 path in tests) |
| `crates/tui_dashboard/src/view/driver.rs` | Filter + format Notice from `kind` |
| `crates/gateway/src/gateway_runtime.rs` | Opt-in ingress console; Dashboard leaves it off |
| `docs/PHASES.md` / gap notes as needed | Point at structured diagnostics |

---

### Task 1: `DiagnosticKind` + record shape in `common`

**Files:**
- Modify: `crates/common/src/observation_records/diagnostic/mod.rs`
- Modify: `crates/common/src/facade.rs` (re-export `DiagnosticKind`)
- Modify: `crates/common/src/lib.rs` if it re-exports diagnostic types

**Interfaces:**
- Produces:

```rust
/// Structured diagnostic facts emitted by the Twin.
///
/// This stream is **wider than any single observer surface**. Variants exist so
/// capture, contracts, engineer tools, and future Zenoh subscribers can select
/// what they need. A driver Notice (or similar) MUST filter and format — e.g.
/// hide [`DiagnosticKind::TimerTick`], ignore headlamp success (zone/context),
/// and show headlamp lines only when actuation is unconfirmed.
///
/// Emit facts only: no icons, identity prefixes, or display sentences in payloads.
/// Presentation is always receiver-side.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagnosticKind {
    Text { text: String },
    Boot,
    TimerTick,
    HeadlampActuationUnconfirmed {
        on: bool,
        cause: FrontHeadlampIncompleteCause,
    },
    RainChanged { raining: bool },
    WiperMotionChanged { wiping: bool },
    ActuationFailure { action: String, error: String },
    TransitionSinkFull,
    TransitionSinkClosed,
}

pub struct DiagnosticRecord {
    pub level: DiagnosticLevel,
    pub source: &'static str,
    pub kind: DiagnosticKind,
    pub session_started_at: UnixTimestamp,
    pub recorded_at: UnixTimestamp,
}
```

- Constructors take `kind: DiagnosticKind` instead of `message: impl Into<String>`.
- `Display` formats a plain ASCII line from `kind` (receiver-side for stdout observer); **no emoji in `kind` data**. Import `FrontHeadlampIncompleteCause` from `crate::vehicle_state` / fsm re-export already used by diagnostic module graph — use the same path zone code uses (`crate::fsm::FrontHeadlampIncompleteCause` or vehicle_state; match existing `sink.rs` imports).

- [ ] **Step 1: Write failing unit tests at bottom of `mod.rs`**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::fsm::FrontHeadlampIncompleteCause;
    use crate::observation_records::transition::SessionClock;

    #[test]
    fn record_stores_kind_not_prose_blob() {
        let clock = SessionClock::capture();
        let rec = DiagnosticRecord::warning(
            &clock,
            "VirtualCarActor",
            DiagnosticKind::HeadlampActuationUnconfirmed {
                on: true,
                cause: FrontHeadlampIncompleteCause::TimedOut,
            },
        );
        assert!(matches!(
            rec.kind,
            DiagnosticKind::HeadlampActuationUnconfirmed {
                on: true,
                cause: FrontHeadlampIncompleteCause::TimedOut
            }
        ));
    }

    #[test]
    fn display_has_no_ack_emoji_for_unconfirmed() {
        let clock = SessionClock::capture();
        let rec = DiagnosticRecord::warning(
            &clock,
            "VirtualCarActor",
            DiagnosticKind::HeadlampActuationUnconfirmed {
                on: true,
                cause: FrontHeadlampIncompleteCause::NegativeAck,
            },
        );
        let s = rec.to_string();
        assert!(!s.contains('✅'));
        assert!(!s.contains('✓'));
        assert!(s.contains("unconfirmed") || s.contains("Headlamp") || s.contains("NACK") || s.contains("NegativeAck"));
    }
}
```

- [ ] **Step 2: Run tests — expect compile/fail on missing `kind`**

```bash
cargo test -p common --lib observation_records::diagnostic::tests -- --nocapture
```

Expected: FAIL (no `DiagnosticKind` / `message` field still present).

- [ ] **Step 3: Implement `DiagnosticKind` + switch `DiagnosticRecord` to `kind`**

Replace `message` with `kind`. Update `at_session` / `info` / `action` / `alert` / `warning` / `error` to accept `DiagnosticKind`. Implement `Display` by matching on `kind` with plain ASCII (e.g. `headlamp actuation unconfirmed on=true cause=TimedOut`, `timer tick`, `boot`, `rain raining=true`, `wiper wiping=true`, `text: …`).

- [ ] **Step 4: Re-export and fix compile errors only as needed to unlock Task 2**

```bash
cargo test -p common --lib observation_records::diagnostic::tests -- --nocapture
```

Expected: PASS for these two tests. Workspace may still fail elsewhere until later tasks.

- [ ] **Step 5: Stop — do not commit unless user asks**

---

### Task 2: Sink helpers

**Files:**
- Modify: `crates/common/src/observation_records/diagnostic/sink.rs`
- Modify: `crates/common/src/lib.rs` exports (remove `diag_front_headlamp_confirmed`, add `diag_headlamp_actuation_unconfirmed`)

**Interfaces:**
- Produces:

```rust
pub fn diag_boot(clock: &SessionClock) -> DiagnosticRecord; // Info + Boot
pub fn diag_timer_tick(clock: &SessionClock) -> DiagnosticRecord; // drop identity arg
pub fn diag_headlamp_actuation_unconfirmed(
    clock: &SessionClock,
    on: bool,
    cause: FrontHeadlampIncompleteCause,
) -> DiagnosticRecord; // Warning or Alert — use Warning to match prior LogWarning path
pub fn diag_rain_changed(clock: &SessionClock, raining: bool) -> DiagnosticRecord;
pub fn diag_wiper_motion_changed(clock: &SessionClock, wiping: bool) -> DiagnosticRecord;
pub fn diag_warning(clock: &SessionClock, text: impl Into<String>) -> DiagnosticRecord; // Text
pub fn diag_actuation_failure(clock: &SessionClock, action: &str, err: &str) -> DiagnosticRecord;
pub fn diag_transition_sink_full(clock: &SessionClock) -> DiagnosticRecord;
pub fn diag_transition_sink_closed(clock: &SessionClock) -> DiagnosticRecord;
```

- Remove: `diag_front_headlamp_confirmed`, `diag_state_transition` (ledger owns transitions).
- Drop unused `front_headlamp_log` icon imports from this file.
- `spawn_stdout_diagnostic_observer` keeps printing `{record}` via `Display`.

- [ ] **Step 1: Write failing tests in `sink.rs`**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::fsm::FrontHeadlampIncompleteCause;
    use crate::observation_records::transition::SessionClock;

    #[test]
    fn unconfirmed_helper_sets_kind() {
        let clock = SessionClock::capture();
        let rec = diag_headlamp_actuation_unconfirmed(
            &clock,
            true,
            FrontHeadlampIncompleteCause::TimedOut,
        );
        assert_eq!(rec.level, DiagnosticLevel::Warning);
        assert!(matches!(
            rec.kind,
            DiagnosticKind::HeadlampActuationUnconfirmed { on: true, .. }
        ));
    }

    #[test]
    fn timer_tick_helper_has_no_identity_prose() {
        let clock = SessionClock::capture();
        let rec = diag_timer_tick(&clock);
        assert_eq!(rec.kind, DiagnosticKind::TimerTick);
    }
}
```

- [ ] **Step 2: Run — expect FAIL**

```bash
cargo test -p common --lib observation_records::diagnostic::sink::tests -- --nocapture
```

- [ ] **Step 3: Implement helpers; delete confirmed + state_transition helpers**

- [ ] **Step 4: Run sink tests — PASS**

- [ ] **Step 5: Stop — no commit unless asked**

---

### Task 3: Actor emission wiring

**Files:**
- Modify: `crates/common/src/twin_runtime/controller/virtual_car_actor.rs`

**Interfaces:**
- Consumes: Task 2 helpers.
- Behaviour:
  1. Boot emit → `diag_boot` (no identity string in kind).
  2. TimerTick → `diag_timer_tick(&clock)` (no identity).
  3. **Remove** block that calls `diag_front_headlamp_confirmed`.
  4. **Remove** block that calls `diag_state_transition`.
  5. On committed turn whose `ingress` is `FrontHeadlampActuationIncomplete { direction, cause }`, emit `diag_headlamp_actuation_unconfirmed` with `on: direction == On`. Still allow `LogWarning` `Text` for other warnings, but **skip** emitting `Text` when the warning string is the headlamp `alert_incomplete` line **or** skip `LogWarning`→diagnostic when ingress is incomplete (prefer structured only — avoid double emit). Preferred: if ingress is `FrontHeadlampActuationIncomplete`, emit structured kind and do **not** also emit `diag_warning` for that turn’s headlamp `LogWarning`.
  6. Rain↔wiper: before/after compare on each committed quiescence:
     - If ingress is `RainsStarted` / `RainsStopped`, emit `diag_rain_changed(raining)`.
     - If wiper state crosses into/out of `WiperState::Running`, emit `diag_wiper_motion_changed(wiping)`.

- [ ] **Step 1: Update `actor_contract` headlamp ACK assertion to expect no confirmed diagnostic**

In `crates/common/src/test/actor_contract.rs`, replace the check that looks for `MSG_ACK_ON` in `message` with: after confirmed ON, diagnostics must **not** contain `HeadlampActuationUnconfirmed` for that success path, and must **not** match a confirmed-prose leftover. Add assertion that success does not require a diagnostic at all (filter infos for `HeadlampActuationUnconfirmed` — empty for happy path).

Also update any `msg.message.contains` sites to `matches!(msg.kind, …)` or `Text { text }`.

- [ ] **Step 2: Run actor contract — expect FAIL**

```bash
cargo test -p common --test '*' 2>&1 | head -5; cargo test -p common actor_contract -- --nocapture
```

(Use the actual test target name if integration tests live under `src/test` as lib tests — run `cargo test -p common ack` or the specific fn name.)

- [ ] **Step 3: Wire actor changes as listed above**

Sketch for incomplete (inside `apply_committed_quiescence`, after you know `quiescent` / ingress — thread ingress through if not already on `QuiescentResult`; if only available on hops, read `final_step` / first hop `transition_record.event`):

```rust
// Pseudocode — adapt to actual QuiescentResult fields:
if let FsmEvent::FrontHeadlampActuationIncomplete { direction, cause } = &ingress {
    if let Some(sink) = &runtime_state.diagnostic_sink {
        let _ = sink.try_emit(diag_headlamp_actuation_unconfirmed(
            &runtime_state.session_clock,
            matches!(direction, FrontHeadlampSwitchDirection::On),
            *cause,
        ));
    }
}
```

For `LogWarning` arm: if this turn already emitted `HeadlampActuationUnconfirmed`, skip `diag_warning` for headlamp alert strings; otherwise `diag_warning` → `Text`.

- [ ] **Step 4: Add focused contract or unit test for rain→wiper diagnostic pair**

In an existing wiper/rain contract file (or new test in `crates/common/src/test/`), drive `RainDetected(true)` then assert diagnostics include `RainChanged { raining: true }` and `WiperMotionChanged { wiping: true }` (order: rain then wiper, or both present). Mirror for rain stop.

- [ ] **Step 5: `cargo test -p common` — PASS**

- [ ] **Step 6: Stop — no commit unless asked**

---

### Task 4: Observation schema v2 (`kind` payload)

**Files:**
- Modify: `crates/observation/src/schema/mod.rs` — `CURRENT_SCHEMA_VERSION: u32 = 2`
- Modify: `crates/observation/src/schema/v1.rs` — replace `message: String` with tagged `kind`
- Modify: `crates/observation/tests/support/mod.rs` — `sample_diagnostic` uses `kind`
- Modify: `crates/observation/tests/round_trip.rs`, `schema_compatibility.rs`, `golden_files.rs`, `summary_cli.rs`
- Create: `crates/observation/testdata/golden/v2/...` (copy from v1, bump versions, replace diagnostic payload)
- Remove or leave unused: `testdata/golden/v1` (prefer delete after tests point at v2)

**Interfaces:**
- Produces archival DTO:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DiagnosticKindV1 {
    Text { text: String },
    Boot,
    TimerTick,
    HeadlampActuationUnconfirmed {
        on: bool,
        cause: FrontHeadlampIncompleteCauseV1,
    },
    RainChanged { raining: bool },
    WiperMotionChanged { wiping: bool },
    ActuationFailure { action: String, error: String },
    TransitionSinkFull,
    TransitionSinkClosed,
}

pub struct DiagnosticPayloadV1 {
    pub level: DiagnosticLevelV1,
    pub source: String,
    pub kind: DiagnosticKindV1,
    pub session_started_at: UnixTimestampV1,
}
```

- `diagnostic_envelope` maps every live `DiagnosticKind` arm explicitly (no wildcard).
- Compatibility tests that currently inject `schema_version: 2` as “mismatch” must inject `3` (or `1`) instead once CURRENT is 2.
- Round-trip assertions: `json["schema_version"] == 2`, `json["payload"]["kind"]["type"] == "text"` (for sample warning).

- [ ] **Step 1: Update sample + projection tests first (TDD)**

Change `sample_diagnostic` to `kind: DiagnosticKind::Text { text: "fixed warning".into() }`. Update round_trip expected JSON. Run — FAIL until schema updated.

- [ ] **Step 2: Implement DTO + projection + version bump + golden v2**

- [ ] **Step 3:**

```bash
cargo test -p observation
```

Expected: PASS

- [ ] **Step 4: Stop — no commit unless asked**

---

### Task 5: Dashboard Observer filter/format

**Files:**
- Modify: `crates/tui_dashboard/src/view/driver.rs`
- Modify: `crates/tui_dashboard/src/main.rs` (tests constructing `DiagnosticRecord`, session status if it reads `message`)

**Interfaces:**
- Consumes: `DiagnosticRecord.kind`
- Produces Notice rules:

| Kind | Driver Notice |
|------|----------------|
| `TimerTick` | Treat as “no notice” (keep previous notice, or show `(no notice yet)` only if none ever — **keep last non-tick notice** in `DashboardState` by not overwriting `latest_diagnostic` when kind is `TimerTick`) |
| `Boot` | Optional short `Notice: Info — Twin booting` or ignore after first paint |
| `HeadlampActuationUnconfirmed` | `Notice: Warning — Headlamp ON/OFF not confirmed (timeout\|NACK)` |
| `RainChanged` / `WiperMotionChanged` | Show plain fact lines (Observer may want proof) |
| `Text` | `Notice: {level} — {text}` (no icon strip needed) |
| `ActuationFailure` / sink full/closed | Show |
| Happy-path headlamp | N/A (not emitted) |

**State rule (important):** In the drain path that sets `state.latest_diagnostic`, **ignore** updates when `kind == TimerTick` so Notice does not flicker to heartbeat.

- [ ] **Step 1: Failing view tests**

```rust
#[test]
fn notice_formats_unconfirmed_without_icons() {
    let diag = /* DiagnosticRecord with HeadlampActuationUnconfirmed on=true TimedOut */;
    let pane = driver_pane(Some(&diag), Some(&sample_ledger()), 80);
    let notice = pane.lines[0].trim();
    assert!(notice.contains("not confirmed") || notice.contains("unconfirmed"));
    assert!(!notice.contains('✅'));
    assert!(!notice.contains('✓'));
}

#[test]
fn timer_tick_does_not_replace_notice_in_state_helper() {
    // unit-test the pure predicate or apply_diagnostic helper:
    // assert!(!should_publish_to_notice(&DiagnosticKind::TimerTick));
}
```

- [ ] **Step 2: Implement `format_notice` match on `kind` + `should_update_notice(kind)`**

- [ ] **Step 3:**

```bash
cargo test -p tui_dashboard
```

Expected: PASS

- [ ] **Step 4: Stop — no commit unless asked**

---

### Task 6: Gateway ingress console opt-in

**Files:**
- Modify: `crates/gateway/src/gateway_runtime.rs`
- Modify: `crates/gateway/src/main.rs` (enable console for standalone gateway if desired)
- Modify: `crates/tui_dashboard/src/main.rs` — ensure builder does **not** enable ingress println

**Interfaces:**
- Add `ingress_console_log: bool` to `TwinRuntimeBuilder` (default `false`).
- `gateway` binary main: `.with_ingress_console_log(true)` **or** only when `--trace-actuation-ingress` (prefer: println ACK lines only if `trace_actuation_ingress` OR explicit console flag — simplest per spec: **never println from Dashboard**; standalone gateway may keep println behind `with_ingress_console_log(true)` in `gateway/src/main.rs`).
- When `ingress_console_log` is false, do not spawn the task that `println!`s formatted ACK lines (still submit twin ingress).

- [ ] **Step 1: Unit test builder default is false**

```rust
#[tokio::test]
async fn ingress_console_log_defaults_false() {
    let b = TwinRuntimeBuilder::new();
    assert!(!b.ingress_console_log()); // add getter like auto_power_on
}
```

- [ ] **Step 2: Implement flag + gate the println task**

- [ ] **Step 3: `cargo test -p gateway` and confirm Dashboard builder path unset**

- [ ] **Step 4: Stop — no commit unless asked**

---

### Task 7: Docs touch-up

**Files:**
- Modify: `docs/PHASES.md` — note structured diagnostics under Phase 5 follow-up / emission hygiene
- Modify: `docs/TODO-twin-lifecycle.md` or `docs/TODO-simulation-5.md` only if a gap row clearly applies (keep minimal)
- Spec status already Approved

- [ ] **Step 1: Short PHASES blurb** — diagnostics emit `DiagnosticKind`; Observer filters; happy-path headlamp ACK not on diagnostic stream.

- [ ] **Step 2: `cargo test -p common -p observation -p tui_dashboard -p gateway`**

Expected: PASS

- [ ] **Step 3: Manual smoke (operator):** Dashboard + vcan0 + headlamp actuator — toggle lights: Notice must **not** show ACK; force NACK/timeout: Notice shows unconfirmed; ledger cells must not show stray `✓`.

- [ ] **Step 4: Stop — commit only if user requests**

---

## Spec coverage checklist

| Spec requirement | Task |
|------------------|------|
| `DiagnosticKind` full vocabulary + stream-vs-observer comments | 1 |
| No icons in payloads | 1–2 |
| Remove confirmed headlamp diagnostic | 2–3 |
| `HeadlampActuationUnconfirmed` only on failure | 3 |
| Keep `TimerTick` in stream; hide from Notice | 3, 5 |
| No `StateTransition` diagnostic | 2–3 |
| Rain↔wiper proof kinds | 3 |
| No wiper ACK variants | (non-goal; enforced by enum) |
| Observation archives `kind` | 4 |
| Dashboard filter/format | 5 |
| Gateway no println under Dashboard | 6 |
| Docs | 7 |
| Zone-encased headlamp TODO | comment on enum variant (Task 1) |
| Wiper context publish for `Wipers:` line | **out of scope** (separate TODO) |
| Ratatui icons later | **out of scope** |

## Self-review notes

- No `message` field remains on live `DiagnosticRecord` after Task 1; all call sites must migrate (Tasks 2–5).
- `diag_state_transition` removal may surprise operators who watched stdout diagnostics for transitions — ledger + Dashboard engineer pane cover that.
- Double-emit guard for incomplete headlamp (`LogWarning` + structured) is required in Task 3.
- Schema file stays named `v1.rs` while `CURRENT_SCHEMA_VERSION = 2` — acceptable; do not rename module in this plan (churn).
