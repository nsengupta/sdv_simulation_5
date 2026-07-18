# Phase 4 emulator session runner (Mode 1) design

**Date:** 2026-07-18  
**Status:** Approved  
**Scope:** Emulator-first Phase 4 — shared tick/session architecture, optional `--readings`,
Ctrl+C controlled stop  
**Related:** [Phase 2 finite CAN emulator](2026-07-16-phase-2-finite-can-emulator-design.md),
[Phase 3 observation capture](2026-07-17-phase-3-observation-capture-design.md),
[`docs/PHASES.md`](../../PHASES.md)

## Goal

Refactor the standalone emulator around a reusable tick/emit component and a single session
runner so Mode 1 (live physics) can stop cleanly via optional `--readings N` or Ctrl+C, both
ending in the same controlled trailer. This prepares Mode 2 (file-driven triples), semantic
golden checks, and eventual CI/CD without delivering those yet.

Phase 4 does **not** deliver observation golden regression, `observation-compare`, file/echo
Mode 2, Dashboard/Gateway process split, or CI workflows. Those remain explicit TODOs.

## Why this reshapes the original Phase 4 roadmap

`PHASES.md` originally framed Phase 4 as an E2E observation golden gate on `vcan0`. Design
discussion established:

- Semantic golden only pays off when inputs are file-driven (Mode 2).
- Mode 2 and Twin in/out equivalence deserve a later, narrower design.
- CI/CD is a learning aim, not an immediate deliverable.
- The sound next step is emulator structure + controlled stop (Approach A: emulator-first).

Gap register implications (documentation updates at implementation time):

| Gap | Phase 4 outcome |
|-----|-----------------|
| G4 (CSV/echo) | Remains deferred; Mode 2 file source is a TODO behind `TelemetrySource` |
| G7 (E2E observation golden) | Remains open; `observation-compare` / semantic golden are TODOs |

## Architectural position

Work stays inside the existing `emulator` package. No new crate and no Phase 6
`emulator-core` split.

```text
PhysicalCar / config  →  TelemetrySource (live now; file later TODO)
                ↓
         session runner
           PowerOn → ticks → controlled_stop
                ↓
            FrameSink (SocketCAN | mock)
```

Dependency direction is unchanged: `emulator` → `common` (signals / lifecycle frames). The
observation crate and Dashboard are untouched.

## Mode 1 session semantics

### Start

Every Mode 1 session begins with one lifecycle **PowerOn** frame.

### Ticks

Each tick emits the usual field set from the current physics model, in the existing stable
wire order:

1. `EngineRpm` (`0x102`)
2. `AmbientLux` (`0x103`)
3. `RainDetected` (`0x104`)

The existing 100 ms interval remains between ticks. `EMULATOR_TUNNEL_PROB` and
`EMULATOR_RAIN_PROB` keep their current override behavior.

### Stop triggers (one shared trailer)

`--readings N` is **optional**:

| CLI | Behavior |
|-----|----------|
| `emulator` | Emit ticks until Ctrl+C, then controlled stop |
| `emulator --readings N` | Emit exactly N ticks, then controlled stop |

Both triggers call the same `controlled_stop` path. No second product mode and no extra
shutdown design beyond selecting which condition requests stop.

Early Ctrl+C while `--readings N` is still counting uses that same path (stop sooner; same
trailer). Once stop has started, further Ctrl+C is ignored (idempotent).

### Controlled stop trailer

1. Finish the current tick if a triple is in progress (no torn tick).
2. Send `EngineRpm(0)` (standstill input; Gateway still ignores `0x101` Speed).
3. Send lifecycle `PowerOff`.
4. Flush/close the sink and exit successfully.

The emulator guarantees transmission order, not FSM acceptance. If the twin rejects PowerOff,
Dashboard/observations show the twin-authored outcome; the emulator still exits after sending
the trailer.

### Ctrl+C ownership

- Handler on the **emulator process only**.
- Implemented with a well-known pattern (e.g. `ctrlc` crate) that sets a flag the session loop
  polls, including during inter-tick sleep so shutdown is not stuck for a full tick.
- Dashboard quit / signal handling is out of scope.

## Component boundaries

| Piece | Responsibility |
|-------|----------------|
| Tick / field model | Existing `PhysicalCar` + `PhysicalWorldModelConfig` produce one tick’s usual fields |
| `TelemetrySource` | Abstraction over “next tick fields.” Mode 1: live physics until stop. Mode 2 (TODO): file rows until EOF |
| Session runner | PowerOn → loop source + sink writes → `controlled_stop` on `CtrlC` or `ReadingsLimit` |
| `FrameSink` | Production SocketCAN sink; in-memory/recording sink for tests (Phase 2 seam retained) |
| CLI | Optional `--readings`; no Mode 2 file flags in Phase 4 |

The reusable internal component is the tick production + session loop boundary so that later:

- Mode 2 can feed fixed triples through the same runner,
- a generator can record N live ticks to a file for Mode 2,
- Dashboard presentation tests need not invent Twin-correct data.

## CLI

```text
emulator
emulator --readings <positive integer>
```

- Missing `--readings`: open-ended until Ctrl+C.
- `--readings N` with `N >= 1`: stop after N ticks via shared trailer.
- `--readings 0`, non-integer, unknown flags, duplicate flags: usage errors (same strictness
  style as Phase 2 parsing, adjusted for optionality).

Environment variables unchanged: `EMULATOR_TUNNEL_PROB`, `EMULATOR_RAIN_PROB`.

## Migration from Phase 2

Phase 2 required `--readings N` and always auto-trailed after N. Phase 4 keeps the same
trailer and wire contract but makes `--readings` optional so omit means “wait for Ctrl+C.”

When `--readings N` is present, behavior matches the familiar finite path: N triples then
Rpm(0)+PowerOff. Documentation (README How-to-Run, architecture notes) must show both forms.

The Phase 2 design doc remains historical for the finite-required CLI. This spec supersedes
emulator CLI and stop semantics going forward.

## Out of scope

- Mode 2 file/echo CLI and CSV/JSONL reader
- Generator that writes N ticks to a file
- Semantic golden / Twin in/out equivalence rules
- `observation-compare` binary
- SocketCAN-capable CI runner / GitHub Actions (or equivalent) workflows
- Dashboard Ctrl+C or lifecycle changes
- Gateway/Dashboard process split (Phase 5)
- Zenoh / uProtocol
- Splitting a separate `emulator-core` crate (defer to Phase 6 unless forced)

## Explicit TODOs (CI/CD readiness, not delivery)

Record these in `docs/PHASES.md` / gap notes when implementing:

1. **Mode 2 `TelemetrySource`** — readable CSV or JSONL; one row = one triple (RPM, lux, rain
   bool); no timestamps in file; session still frames live PowerOn → rows → controlled stop.
2. **Tick file generator** — reuse live tick component to create N entries for Mode 2.
3. **Semantic golden / `observation-compare`** — SUT is the Digital Twin; equivalence between
   pumped-in ticks and diagnostic/ledger outcomes against Twin rules (revisit as its own design).
4. **Always-on vs SocketCAN CI** — unit/session tests without `vcan0` first; optional later job
   on a SocketCAN-capable runner for full multi-process E2E.
5. **Optional scripted orchestration** — documented multi-process smoke remains manual until a
   CI runner exists.

## Mandatory tests

1. With `--readings N` and a mock sink: PowerOn, exactly N triples in wire order, then one
   `EngineRpm(0)`, then one PowerOff; no duplicate trailer.
2. Stop flag asserted mid-session (simulating Ctrl+C): same trailer; idempotent if flagged again.
3. Live `TelemetrySource` produces the usual field set for a tick; session runner writes them
   in stable order.
4. CLI: no args accepted; `--readings 30` accepted; `--readings 0` / invalid / unknown rejected.
5. Existing emulator unit coverage updated for optional readings; `cargo test --workspace` green.

Manual acceptance (not automated CI):

- `vcan0` up; actuators; Dashboard; `emulator` or `emulator --readings N`; stop via Ctrl+C or N;
  trailer transmitted; Dashboard shows Twin accept or reject of PowerOff.

## Acceptance criteria

- Session runner + `TelemetrySource` seam exist; Mode 1 live source wired to SocketCAN.
- Optional `--readings` and Ctrl+C both end in one shared controlled stop.
- Focused emulator tests and workspace tests pass.
- README and phase/gap docs reflect the new CLI and deferred G4/G7 work.
- Mode 2, generator, observation golden, compare tool, and CI workflows are documented TODOs
  only — not required to mark Phase 4 Done under this design.

## Expected touch points

- `crates/emulator/src/lib.rs` — module layout for source/session if split from `runner`
- `crates/emulator/src/runner.rs` — session loop + `controlled_stop` (replaces finite-only API)
- `crates/emulator/src/cli.rs` — optional `--readings`
- `crates/emulator/src/main.rs` — Ctrl+C flag + composition root
- `crates/emulator/src/sink.rs` — unchanged contract; tests keep mock sink
- `crates/emulator/Cargo.toml` — Ctrl+C dependency as needed
- Emulator unit tests under `crates/emulator/`
- `README.md`, `docs/PHASES.md`, `docs/ARCHITECTURE-OVERVIEW.md` — CLI, Phase 4 status, TODOs
