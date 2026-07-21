# Phase 5 Dashboard Presentation Rework Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rework `tui_dashboard` into a driver/engineer/ledger-tail presentation over existing Twin diagnostic and ledger records, with a testable view-model and a speed bar scaled from `common` constants.

**Architecture:** Pure `view` modules map records → display lines; Ratatui only lays out. `DashboardState` keeps latest diagnostic, latest ledger, and a `VecDeque` of the last 20 ledger rows. No new Twin emissions. Rain/Wipers/ROB gaps show `—`. Process split deferred (phase renumber in docs).

**Tech Stack:** Rust, Ratatui/Crossterm (existing), `common::vehicle_physics` constants, existing observation types via `common::facade`.

## Global Constraints

- Presentation only: no Gateway split, no Twin field additions, no colour/emoji Done criteria.
- Speed replaces RPM as the primary motion metric; expanding/reducing bar required.
- Scale speed bar using `SPEED_EXTREME_OPERATION_THRESHOLD_KPH` (160) as full scale unless a clearer existing constant fits better.
- All pane lines take an explicit `width` and never wrap (`\n` forbidden in a line).
- Ledger tail N = 20 fixed; newest prefixed with `>`.
- Honest subset: Rain, Wipers, Active ROB → `—` / TODO comments in view code.
- Preserve observation capture, boot diagnostic gate, persist-before-display.
- Session bar and keys footer stay observer-only (`q` quit).
- Do not stage or commit unless the user separately requests it.
- Every task implicitly includes this section.

## File structure

| Path | Responsibility |
|------|----------------|
| `crates/common/src/vehicle_physics/display.rs` | Pure `format_speed_bar(speed_kph, bar_width)` (+ unit tests) |
| `crates/common/src/vehicle_physics/mod.rs` | Export display helpers |
| `crates/tui_dashboard/src/view/mod.rs` | View module root + `clip_line` / shared width helpers |
| `crates/tui_dashboard/src/view/driver.rs` | `DriverPane` lines from diagnostic + latest ledger |
| `crates/tui_dashboard/src/view/engineer.rs` | `EngineerPane` lines from latest ledger |
| `crates/tui_dashboard/src/view/ledger_tail.rs` | `LedgerTail` buffer + line formatting |
| `crates/tui_dashboard/src/main.rs` | State, drain, new layout render; reuse session formatter |
| `docs/PHASES.md` | Phase 5 = presentation; renumber 6–10 |
| `docs/ARCHITECTURE-OVERVIEW.md` | Transitional note + G5 deferral |
| `README.md` / `DESIGN.md` §16.2 | Layout blurb |

Binary package keeps `mod view;` from `main.rs` (no forced lib split). View unit tests live in the view modules.

---

### Task 1: Speed bar helper in `common`

**Files:**
- Create: `crates/common/src/vehicle_physics/display.rs`
- Modify: `crates/common/src/vehicle_physics/mod.rs`

**Interfaces:**
- Produces:

```rust
/// Build a filled bar of exactly `bar_width` characters for `speed_kph`.
/// Full scale = `SPEED_EXTREME_OPERATION_THRESHOLD_KPH`. Speeds above full scale clamp to full.
/// Empty `bar_width` yields empty string.
pub fn format_speed_bar(speed_kph: u16, bar_width: usize) -> String;
```

- Uses: `SPEED_EXTREME_OPERATION_THRESHOLD_KPH` from `constants`
- Fill char `'|'`, empty `'.'` (match mockup spirit)

- [ ] **Step 1: Write failing tests in `display.rs`**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::vehicle_physics::SPEED_EXTREME_OPERATION_THRESHOLD_KPH;

    #[test]
    fn zero_speed_is_all_empty() {
        assert_eq!(format_speed_bar(0, 10), "..........");
    }

    #[test]
    fn full_scale_fills_bar() {
        assert_eq!(
            format_speed_bar(SPEED_EXTREME_OPERATION_THRESHOLD_KPH, 10),
            "||||||||||"
        );
    }

    #[test]
    fn above_full_scale_clamps() {
        assert_eq!(
            format_speed_bar(SPEED_EXTREME_OPERATION_THRESHOLD_KPH + 40, 8),
            "||||||||"
        );
    }

    #[test]
    fn mid_speed_fills_proportionally() {
        let bar = format_speed_bar(80, 10); // half of 160
        assert_eq!(bar.chars().filter(|c| *c == '|').count(), 5);
        assert_eq!(bar.len(), 10);
    }

    #[test]
    fn zero_width_is_empty() {
        assert_eq!(format_speed_bar(100, 0), "");
    }
}
```

- [ ] **Step 2: Run and verify failure**

```bash
cargo test -p common format_speed_bar
```

Expected: FAIL (module missing).

- [ ] **Step 3: Implement**

```rust
// display.rs
use super::constants::SPEED_EXTREME_OPERATION_THRESHOLD_KPH;

pub fn format_speed_bar(speed_kph: u16, bar_width: usize) -> String {
    if bar_width == 0 {
        return String::new();
    }
    let full = SPEED_EXTREME_OPERATION_THRESHOLD_KPH.max(1) as usize;
    let filled = ((speed_kph as usize).min(full) * bar_width) / full;
    let mut out = String::with_capacity(bar_width);
    for i in 0..bar_width {
        out.push(if i < filled { '|' } else { '.' });
    }
    out
}
```

Export from `mod.rs`: `pub mod display; pub use display::format_speed_bar;`

- [ ] **Step 4: Pass**

```bash
cargo test -p common format_speed_bar
```

Expected: PASS.

- [ ] **Step 5: Commit only if user requested**

```bash
git add crates/common/src/vehicle_physics/display.rs crates/common/src/vehicle_physics/mod.rs
git commit -m "feat(common): add format_speed_bar for dashboard telemetry"
```

---

### Task 2: View clip helper + DriverPane

**Files:**
- Create: `crates/tui_dashboard/src/view/mod.rs`
- Create: `crates/tui_dashboard/src/view/driver.rs`
- Modify: `crates/tui_dashboard/src/main.rs` — add `mod view;` only (no layout swap yet)

**Interfaces:**
- Produces:

```rust
pub const MISSING: &str = "—";

pub fn clip_line(s: &str, width: usize) -> String; // ellipsis if truncated; never contains '\n'

pub struct DriverPane {
    pub lines: Vec<String>,
}

pub fn driver_pane(
    diagnostic: Option<&DiagnosticRecord>,
    ledger: Option<&PublishedTransitionRecord>,
    width: usize,
) -> DriverPane;
```

Line order (readable labels):

1. `Notice: {Level} — {message}` or `Notice: (no notice yet)`
2. `Speed: [{bar}] {kph} km/h` — bar width derived from remaining space after label/value; if no ledger, `Speed: —`
3. `Light: {lux} lux` or `—`
4. `Headlamps: {On|Off|…}` (+ `; waiting for reply` if `ack_pending_since.is_some()`)
5. `Rain: —` (TODO)
6. `Wipers: —` (TODO)

Pre-PowerOn (`ledger.is_none()`): single standby line (reuse today’s wording), clipped.

Headlamp labels: plain English (`On`, `Off`, `On requested`, `Off requested`, `Ready`).

- [ ] **Step 1: Failing tests in `driver.rs`**

Reuse a local `sample_ledger_with_speed(speed, lux, headlamp)` helper (copy field pattern from `main.rs` `sample_ledger_row` / `empty_published_ctx`).

```rust
#[test]
fn driver_shows_notice_and_speed_bar() { /* width 48; speed 80 → bar has some '|' */ }

#[test]
fn driver_rain_and_wipers_are_placeholders() {
    let pane = driver_pane(Some(&diag), Some(&ledger), 40);
    assert!(pane.lines.iter().any(|l| l.starts_with("Rain: —")));
    assert!(pane.lines.iter().any(|l| l.starts_with("Wipers: —")));
}

#[test]
fn driver_lines_never_exceed_width_or_wrap() {
    let pane = driver_pane(Some(&long_diag), Some(&ledger), 32);
    for line in &pane.lines {
        assert!(line.chars().count() <= 32);
        assert!(!line.contains('\n'));
    }
}

#[test]
fn higher_speed_fills_more_bar_cells() {
    // same width; count '|' in Speed line for speed 40 vs 120
}
```

- [ ] **Step 2: Run fail**

```bash
cargo test -p tui_dashboard --lib 2>&1 || cargo test -p tui_dashboard driver
```

Note: binary crate tests are `cargo test -p tui_dashboard` filtering `driver`.

Expected: FAIL.

- [ ] **Step 3: Implement `clip_line` + `driver_pane`**

Use `common::vehicle_physics::format_speed_bar`. Move/adapt truncation from `main.rs` `truncate_to` into `view::clip_line` (keep `main` calling view or leave thin wrappers temporarily).

- [ ] **Step 4: Pass**

```bash
cargo test -p tui_dashboard driver
```

- [ ] **Step 5: Commit only if user requested**

---

### Task 3: EngineerPane + LedgerTail

**Files:**
- Create: `crates/tui_dashboard/src/view/engineer.rs`
- Create: `crates/tui_dashboard/src/view/ledger_tail.rs`
- Modify: `crates/tui_dashboard/src/view/mod.rs` — export

**Interfaces:**

```rust
pub struct EngineerPane { pub lines: Vec<String> }

pub fn engineer_pane(ledger: Option<&PublishedTransitionRecord>, width: usize) -> EngineerPane;

pub const LEDGER_TAIL_N: usize = 20;

pub struct LedgerTail {
    rows: VecDeque<PublishedTransitionRecord>,
}

impl LedgerTail {
    pub fn new() -> Self;
    pub fn push(&mut self, row: PublishedTransitionRecord);
    pub fn lines(&self, width: usize) -> Vec<String>; // newest last; prefix '>' on last
}

pub fn format_ledger_line(row: &PublishedTransitionRecord, width: usize, newest: bool) -> String;
```

Engineer lines:

1. `Current state: {…}` or `—`
2. `Last event: {…}` or `—`
3. `Active ROB turns: —`
4. `Sub-assemblies:` then `  Headlamp: {…}` from context; `  Wiper: —`

Ledger line format (plain):  
`[seq] {event}  {old} → {next}` with optional leading `> ` for newest. Clip to width.

- [ ] **Step 1: Failing tests**

```rust
#[test]
fn engineer_fills_state_and_event_rob_placeholder() { … }

#[test]
fn ledger_tail_keeps_last_20_of_25() {
    let mut tail = LedgerTail::new();
    for seq in 1..=25 {
        tail.push(sample_with_seq(seq));
    }
    let lines = tail.lines(80);
    assert_eq!(lines.len(), 20);
    assert!(lines[0].contains("[6]")); // first kept
    assert!(lines[19].starts_with("> "));
    assert!(lines[19].contains("[25]"));
}

#[test]
fn ledger_lines_respect_width() { … }
```

- [ ] **Step 2: Run fail → Step 3: Implement → Step 4: Pass**

```bash
cargo test -p tui_dashboard engineer
cargo test -p tui_dashboard ledger
```

- [ ] **Step 5: Commit only if user requested**

---

### Task 4: Wire state + Ratatui layout

**Files:**
- Modify: `crates/tui_dashboard/src/main.rs`

**Interfaces / behavior:**

```rust
struct DashboardState {
    latest_diagnostic: Option<DiagnosticRecord>,
    latest_transition: Option<PublishedTransitionRecord>,
    ledger_tail: view::LedgerTail,
}

fn handle_ledger(...) {
    capture.record_ledger(&record)?;
    state.ledger_tail.push(record.clone()); // or push then move — clone if needed for latest
    state.latest_transition = Some(record);
    Ok(())
}
```

Prefer: `state.latest_transition = Some(record.clone()); state.ledger_tail.push(record);` if clone is cheap enough, or push clone into tail.

`render_frame`:

```text
Vertical: Session (len 3) | Middle (min) | Keys (len 3)
Middle vertical: TopSplit (min ~40%) | Ledger (min ~60%)
TopSplit horizontal: Driver 50% | Engineer 50%
```

Titles: ` Diagnostic/Telemetry `, ` State Transitions `, ` Deterministic Transition Ledger (live) `.

Inner width = area width − border/padding (same subtract-4 pattern as today). Call `driver_pane` / `engineer_pane` / `ledger_tail.lines(width)`.

Session: keep `format_status_line`. Keys: keep `KEYS_FOOTER`.

Update `run_ui_loop` draw closure to pass full `&DashboardState`.

Migrate existing truncation helpers used only by old panels; keep any still needed for session line.

Update unit tests in `main.rs` that assume old two-panel render only if they break; capture/boot tests must stay green.

- [ ] **Step 1: Adjust/add a focused test that `handle_ledger` grows the tail**

```rust
#[test]
fn handle_ledger_appends_to_tail() {
    // mock capture; push 2 rows; assert ledger_tail.lines(80).len() == 2
}
```

- [ ] **Step 2: Implement layout wiring**

- [ ] **Step 3: Verify**

```bash
cargo test -p tui_dashboard
cargo build -p tui_dashboard
```

Expected: PASS / SUCCESS.

- [ ] **Step 4: Manual smoke note** (operator): `vcan0` + actuators + dashboard + emulator — confirm Speed bar moves, ledger tails with `>`, Rain/Wipers/ROB show `—`.

- [ ] **Step 5: Commit only if user requested**

---

### Task 5: Phase renumber and docs

**Files:**
- Modify: `docs/PHASES.md` — roadmap + replace Phase 5 section; renumber former 5→6 … 9→10; fix “twin until Phase 5” → Phase 6
- Modify: `docs/ARCHITECTURE-OVERVIEW.md` — G5 text (split deferred); emulator/dashboard rows; observation ownership still “Phase 6”
- Modify: `README.md` — Dashboard layout description
- Modify: `DESIGN.md` §16.2 layout sketch to new panes
- Cross-links that say “Phase 5 process split” → Phase 6 (grep and fix)

Phase 5 section status: `In progress` until smoke; checkboxes per design acceptance.

Follow-up TODOs listed under Phase 5 (structured lines / zoned speed = Task 6; Rain/Wipers/ROB
data; then Phase 6 split). **Do not renumber phases 6+.**

- [ ] **Step 1: Apply doc edits**

- [ ] **Step 2: Grep stale references**

```bash
rg -n "Phase 5.*split|until \*\*Phase 5\*\*|Gateway ↔ Dashboard" docs README.md DESIGN.md
```

- [ ] **Step 3: Workspace tests**

```bash
cargo test -p tui_dashboard
cargo test -p common format_speed_bar
cargo test --workspace
```

Expected: PASS.

- [ ] **Step 4: Commit only if user requested**

---

### Task 6: Phase 5 follow-up — structured `PaneLine` + zoned speed bar

> Run **after** original Phase 5 Done; **before** Phase 6. Phases 6–10 stay numbered as today.

**Spec:** design § Follow-up — structured lines and zoned speed.

**Files (expected):**
- Modify: `crates/common/src/vehicle_physics/constants.rs` — display band constants
  (`SPEED_BAND_GREEN_MAX_KPH` = 100, `SPEED_BAND_YELLOW_MAX_KPH` = 150; full scale remains
  `SPEED_EXTREME_OPERATION_THRESHOLD_KPH`)
- Modify: `crates/common/src/vehicle_physics/display.rs` — zone/band helpers (pure; no Ratatui)
- Modify: `crates/tui_dashboard/src/view/mod.rs` — `PaneLine`, `LineRole`, `Segment`,
  `SegmentStyle`, `SegmentContent` (`Text` | `SpeedBar` | reserved `Swatch` | `Icon`)
- Modify: `crates/tui_dashboard/src/view/{driver,engineer,ledger_tail}.rs` — emit `Vec<PaneLine>`
- Modify: `crates/tui_dashboard/src/main.rs` — `PaneLine` → ratatui `Line` style map
- Modify: `docs/PHASES.md` — check follow-up boxes when done

**Locked UI rules:**
- Zoned bar **B**; empty cell `.`; `Speed:` Default; numeric suffix = current band colour.
- Ledger `>` Default.
- All constants from `common` only.
- Heads-up only: visibility swatch / weather icon segments when Twin data exists — not Done
  for this task.

- [x] **Step 1: `common` band constants + pure zone/cell helper tests**

- [x] **Step 2: Introduce `PaneLine` model; migrate all panes to single-segment `Text` (look unchanged)**

- [x] **Step 3: Wire renderer style map; keep Default look**

- [x] **Step 4: Multi-segment zoned `SpeedBar` + band-coloured numeric suffix**

- [x] **Step 5: Update view tests; `cargo test -p common` display + `cargo test -p tui_dashboard`**

- [ ] **Step 6: Optional `vcan0` colour smoke; tick PHASES follow-up checkboxes**
  (structure + zoned speed checkboxes ticked in PHASES; heads-up widgets still open)

- [ ] **Step 7: Commit only if user requested**

---

## Spec coverage checklist

| Spec item | Task |
|-----------|------|
| `format_speed_bar` / Speed not RPM | Task 1–2 |
| DriverPane honest subset + Rain/Wipers `—` | Task 2 |
| EngineerPane + ROB `—` | Task 3 |
| LedgerTail N=20 + `>` | Task 3–4 |
| Width clipping | Task 2–3 |
| Layout Session / driver / eng / ledger / keys | Task 4 |
| Capture unchanged | Task 4 (no capture API change) |
| Phase renumber 6–10 | Task 5 |
| Colour/emoji / Twin enrichments / process split out of scope (original Done) | Tasks 1–5 |
| `PaneLine` + zoned speed + `common` bands (follow-up) | Task 6 |

## Plan self-review

- No Twin schema changes sneaked in.
- Bar full-scale pinned to existing `SPEED_EXTREME_OPERATION_THRESHOLD_KPH`.
- Binary crate tests use `cargo test -p tui_dashboard <filter>` (no false `--lib` requirement once modules exist under `main`).
- Commits gated on user request (design+plan already authorized as a docs commit separately).
- Task 6 does **not** invent a Phase 6 presentation slice or renumber later phases.
