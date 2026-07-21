# Plan — SDV Simulation 5

**Livedoc.** Detailed per-phase checklists live in
[`archive/PHASES-detailed.md`](archive/PHASES-detailed.md). Per-phase agent specs/plans live under
[`archive/superpowers/`](archive/superpowers/).

## Roadmap at a glance

```text
Phase 1   CAN lifecycle + silent ignore while Off     Done
Phase 2   Finite CAN emulator; observer-only Dashboard Done
Phase 3   Observation capture (files)                  Done
Phase 4   Emulator Mode 1 session runner               Done
Phase 5   Dashboard presentation (driver/engineer)     Done
Phase 6   Split Gateway ↔ Dashboard                    Done
Phase 7   Embedded emulator + TUI driver               Cancelled
Phase 8   Standalone replay                            TBD next simulation
Phase 9   Live observation: UDS | Zenoh                Done
Phase 10  Shutdown / disband / polish (TL-6+)          Later
```

**Where we are:** Phases 1–6 and 9 Done. Vehicle bus is **CAN**. Live observation is
**UDS or peer Zenoh** (exclusive CLI). File archive tee always runs with the twin.
**Next:** Phase 10 (shutdown / disband) — see [`TODO-twin-lifecycle.md`](TODO-twin-lifecycle.md).

## What each Done phase delivered (one line)

| Phase | Delivered |
|------:|-----------|
| 1 | `0x100` PowerOn/Off; silent ignore while FSM `Off` |
| 2 | Finite `--readings N` emulator; Dashboard observation-only |
| 3 | Observation library + file streams (manifest / diagnostic / ledger) |
| 4 | Emulator session Mode 1 (Ctrl+C / readings stop) |
| 5 | Driver / Engineer / ledger-tail panes; structured diagnostics |
| 6 | Gateway owns twin; Dashboard is live consumer only |
| 9 | Exclusive `--uds` \| `--zenoh --keyexpr` \| Gateway `--no-live`; schema on the wire |
| — | Weather/wiper on published ledger + Dashboard glyphs (schema **v3**) |

## Important missing TBDs

Listed compactly in the top-level [`README.md`](../README.md) § TBD. Expanded notes below.

### Active ROB turns (Engineer pane)

Not started (line removed; was `—`). Depth lives in `barrier_queue` only; honest live `N`
needs emit-on-queue-change, not a stamp on ledger hops alone. See prior design notes in
[`archive/superpowers/specs/2026-07-20-weather-wiper-observation-design.md`](archive/superpowers/specs/2026-07-20-weather-wiper-observation-design.md).

### Coloured visibility `Swatch`

Unicode lux boxes today (`◼`/`▦`/`◻`). Later: Ratatui colour chips via reserved
`SegmentContent::Swatch` (low/hold/bright).

### Notice colour tokens

Labels are cyan; Notice body is Default. Later: colour by `DiagnosticLevel` / kind.

### Glyph ASCII fallback / animation

Optional terminal fallback and motion — not required for observation correctness.

### Richer assembly-actor detail

Engineer shows Headlamp + Wiper from published context. Deeper twinlet status if Twin publishes it.

### Zone-encased headlamp unconfirmed

Open on `DiagnosticKind::HeadlampActuationUnconfirmed` (may move into zone tell-back).

### Shutdown / disband

Phase 10 / [`TODO-twin-lifecycle.md`](TODO-twin-lifecycle.md) TL-6+.

### Other carry-forwards

Non-blocking actuation, comment pass, headlamp isolation tests, actor fuzz — see
[`TODO-simulation-5.md`](TODO-simulation-5.md) (engineering backlog; not all belong in README).

## Archive map

| Path | Content |
|------|---------|
| [`archive/PHASES-detailed.md`](archive/PHASES-detailed.md) | Full phase gates / acceptance |
| [`archive/DESIGN-iteration-4.md`](archive/DESIGN-iteration-4.md) | Brain / ROB / twin design (Iter 4) |
| [`archive/superpowers/`](archive/superpowers/) | Per-phase specs + implementation plans |
