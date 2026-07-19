# Phase 5 Dashboard presentation rework design

**Date:** 2026-07-18  
**Status:** Approved  
**Reference layout:** [`assets/Dashboard-format.txt`](../../../assets/Dashboard-format.txt)  
**Related:** [`docs/PHASES.md`](../../PHASES.md), [`DESIGN.md`](../../../DESIGN.md) §16.2,
Phase 3 observation capture (streams unchanged)

## Goal

Rework `tui_dashboard` into a clearer **driver + engineer** presentation of Twin emissions,
using existing diagnostic and ledger records only. Prove in live use whether Twin data is
sufficient before any Gateway/Dashboard process split.

Phase 5 does **not** add Twin emission fields, process split, colour/emoji polish, replay, or
embedded emulator. Missing values render as `—` (or omit the line); enrichments are follow-ups
after operators see the new UI in action.

## Why Phase 5 changes

The previous Phase 5 (Gateway ↔ Dashboard process split) is **deferred**. Splitting processes
before the Dashboard can show that Twin output is sufficient, correct, and timely would hide
presentation gaps behind IPC. New ordering:

| New # | Former | Topic |
|------:|--------|--------|
| **5** | *(new)* | Dashboard presentation rework |
| **6** | 5 | Split Gateway ↔ Dashboard |
| **7** | 6 | Dashboard embeds emulator + TUI driver |
| **8** | 7 | Standalone replay |
| **9** | 8 | Transport CAN vs Zenoh |
| **10** | 9 | Shutdown, disband, polish (TL-6/7/8) |

Roadmap and gap register updates land with implementation docs (`PHASES.md`,
`ARCHITECTURE-OVERVIEW.md`). Twin remains in-process inside `tui_dashboard` until **Phase 6**.

## Approach

**View-model module** inside `tui_dashboard` (not a render-only dump in `main.rs`):

- Pure functions map records → `DriverPane` / `EngineerPane` / `LedgerTail` display lines.
- Ratatui only lays out and paints those lines.
- Capture, boot diagnostic, and persist-before-display stay unchanged.

## Layout

```text
┌─ Session (unchanged from today) ──────────────────────────────┐
├─ Diagnostic/Telemetry (driver) ─┬─ State Transitions (eng.) ──┤
├─ Deterministic Transition Ledger (tail −20, `>` on newest) ───┤
└─ Keys ────────────────────────────────────────────────────────┘
```

Match the structure in `assets/Dashboard-format.txt`. Session bar content and meaning stay as
today. Keys footer stays observer-only (`q` quit; no lifecycle injection).

## View-model boundaries

| Piece | Responsibility |
|-------|----------------|
| `view` (new module(s)) | Pure presentation mapping; no I/O, no Twin, no Ratatui |
| `DashboardState` | Latest diagnostic, latest ledger, `VecDeque` of last **20** ledger rows |
| `render` | Layout constraints + widgets; consumes view output; measures pane width |
| `main` | Install, capture, drain loop, keys — ownership unchanged |

**Readability:** pane titles and field labels use plain language (`DriverPane` /
`EngineerPane` naming in code; UI titles as in the mockup). Ledger lines are supplementary;
coloured keywords are explicitly later.

## DriverPane (left) — regular driver

Primary signal: **latest diagnostic** (level + message), kept readable.

Secondary: honest fields from **latest ledger** `current_ctx` when present:

| Line | Source | Notes |
|------|--------|--------|
| Latest notice | `DiagnosticRecord` | Level + message; “(no notice yet)” if none |
| Speed + bar | `current_ctx.powertrain.speed_kph` | **Speed replaces RPM** this phase |
| Light / lux | `current_ctx.visibility.ambient_lux` | Number; optional low/ok only via `common` thresholds if already available |
| Headlamps | `current_ctx.headlamp.state` | Include “waiting for reply” when `ack_pending_since` is set |
| Rain | — | **TODO** (not published today) |
| Wipers | — | **TODO** (not a durable published context field) |

### Speed bar

Expanding/reducing bar is **in scope** and important. Scale using `common` physics/safety
constants where applicable (e.g. extreme-operation speed threshold as a natural full-scale
reference). Bar and label must fit the **inner pane width**; no wrapping.

Prefer a small pure helper in `common` if none exists (e.g. `format_speed_bar(speed, width)`)
so presentation stays testable and reusable; otherwise a `tui_dashboard` helper that only
imports constants from `common`.

## EngineerPane (right) — engineers

| Line | Source | If missing |
|------|--------|------------|
| Current state | Latest ledger `next_state` | `—` |
| Last event | Latest ledger `event` | `—` |
| Active ROB turns | Not published | `—` |
| Sub-assemblies | Headlamp may mirror headlamp context; Wiper actor status not published | Honest `—` / partial |

No invented ROB depth or assembly machine state.

## LedgerTail (bottom)

- Behavior like Linux `tail -N` with **N = 20** (fixed this phase; CLI override not required).
- On each new ledger record: push back, pop front if over capacity.
- One line per row, clipped to full-width ledger pane (no wrap).
- Newest line prefixed with `>`.
- Suggested plain format: `[seq] event  old → next` (exact wording chosen for clarity in
  implementation; keep monospace-friendly).
- Coloured keywords: **later**, not Phase 5 Done criteria.

## Width and clipping

Every frame, measure each pane’s inner width. Formatters take `width` and produce a single
line of at most that many characters (truncate with ellipsis or hard clip — pick one and test
it). Never insert newlines inside a field line.

Reuse existing dashboard truncation patterns where they already exist; centralize in `view`.

## Runtime behavior

1. Boot diagnostic + observation capture unchanged (Phase 3).
2. Pre-PowerOn: standby copy in driver/engineer panes (no fake gauges).
3. Diagnostic arrival → update Driver notice; refresh view.
4. Ledger arrival → update latest ledger, append to tail (cap 20), refresh all three views.
5. Twin remains authoritative; Dashboard never injects lifecycle.

## Out of scope (original Done gate)

- Twin emission enrichments (rain, wiper status, ROB depth, assembly actors) — open TODOs after live use
- Gateway/Dashboard process split (Phase 6) — **phase numbers 6+ stay as listed above**
- Embedded emulator / driver controls (Phase 7)
- Replay (Phase 8)
- Zenoh / shutdown polish (Phases 9–10)

Structured pane lines and zoned speed colour are **Phase 5 follow-up** (below), not a new
roadmap phase and not folded into Phase 6.

## Mandatory tests

1. **Driver view:** fixture diagnostic + ledger → expected notice, speed value, bar length
   monotonic with speed; Rain/Wipers absent or `—`.
2. **Engineer view:** state/event filled from ledger; ROB `—`.
3. **LedgerTail:** 25 rows in → 20 retained; newest marked `>`; order preserved.
4. **Width:** long strings truncated to `width`; no `\n` in a formatted line.
5. Existing capture/boot/CLI tests remain green; `cargo test --workspace` passes.

## Acceptance

- Layout matches `Dashboard-format.txt` structure (Session / driver / engineer / ledger / keys).
- Speed bar expands/reduces; RPM not shown as the primary motion metric this phase.
- Honest gaps for Rain, Wipers, ROB, missing assemblies.
- Live `vcan0` smoke: operator can read driver vs engineer intent; ledger tails correctly.
- Docs: Phase renumber applied; process split deferred with rationale.
- Mark Phase 5 Done only after that smoke (same gate style as Phase 4).

## Expected touch points

- `crates/tui_dashboard/src/view/*.rs` (or `view.rs`) — pure view-model
- `crates/tui_dashboard/src/main.rs` — state, drain, render wiring
- `crates/common/src/…` — optional shared bar/format helper + existing constants
- `crates/tui_dashboard` unit tests for view-model
- `docs/PHASES.md`, `docs/ARCHITECTURE-OVERVIEW.md`, `README.md` (Dashboard layout blurb)
- `DESIGN.md` §16.2 layout sketch (align with new panes)

## Follow-up — structured lines and zoned speed (still Phase 5)

Land **before Phase 6**. Does not renumber phases. Original Phase 5 Done (layout + smoke)
remains; this is presentation hardening on the same gate.

### Decisions (locked)

| Topic | Choice |
|-------|--------|
| Bar colouring | **B — zoned segments:** each cell’s colour follows its place on the 0…160 scale, not a single colour for the whole fill |
| Empty cells | Glyph `.` (not a filled/dim block) |
| `Speed:` label | Default (unstyled) |
| Numeric `N/160 km/h` | Colour of **current** speed band |
| Ledger newest `>` | Default for now |
| Constants | Band limits and full scale from **`common`** only (one truth app-wide) |
| Bands | green `0..=100`, yellow `101..=150`, red `>=151`; full scale = `SPEED_EXTREME_OPERATION_THRESHOLD_KPH` |

### View model

Replace `Vec<String>` pane lines with structured lines so every row carries properties and
future widgets do not require another reshape:

```text
PaneLine { role, segments[] }

Segment
  style: SegmentStyle          // semantic token (ZoneGreen, Mute, Default, …) — not Ratatui Color
  content:
    Text("…")
    | SpeedBar { cells[] }     // each cell: '.' or '|' + zone token
    | Swatch { … }             // future: visibility low/high boxes
    | Icon { … }               // future: rain / clear-day glyphs
```

- **View** stays free of Ratatui; **render** maps tokens → `Style` / widgets.
- All driver, engineer, and ledger lines use `PaneLine` (simple rows = one `Text` segment).
- Heads-up: Diagnostic and Ledger will later embed **inline widgets** on some lines
  (e.g. low visibility = brown box, high = bright yellow; rain = cloud/rain symbol,
  clear = sunlit-day symbol). Those use `Swatch` / `Icon` on the same model; blocked on
  Twin fields where noted.

### Follow-up acceptance (when implemented)

- Every pane line is a `PaneLine` with role + segments.
- Speed bar shows zone colours; empty `.`; label Default; numeric suffix band-coloured.
- Band/full-scale constants imported from `common` only.
- `cargo test -p tui_dashboard` / `common` display helpers green; optional `vcan0` colour smoke.

## Explicit follow-ups (data / later polish)

1. Rain line — Twin field or diagnostic convention (+ weather `Icon` when ready)  
2. Wiper status line  
3. Active ROB turns  
4. Assembly actor statuses  
5. Notice-level colour tokens / emoji on filtered Notice lines  
6. Visibility `Swatch` using `common` lux thresholds when product wants the boxes  
7. Phase 6 process split (only once emissions prove sufficient; **not** this follow-up)
