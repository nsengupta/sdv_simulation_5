# Weather + wiper observation (Dashboard display) design

**Date:** 2026-07-20  
**Status:** Draft — awaiting user review  

**Branch note:** Implement on a feature branch from `main`; do not push unless asked.  
**Related:** [Phase 5 Dashboard presentation](2026-07-18-phase-5-dashboard-presentation-design.md),
[Structured diagnostics](2026-07-18-structured-diagnostics-design.md),
[Phase 9 Zenoh observation](2026-07-20-phase-9-zenoh-observation-design.md),
[`docs/PHASES.md`](../../PHASES.md)

## Goal

Close the Phase 5 honest `—` gaps for **rain** and **wiper** so the Dashboard Driver and
Engineer panes show live weather/wiper status from Twin observation streams. Emit on **both**
diagnostics and the transition ledger; collectors decide what to use and what to ignore.

Ship thin Driver **glyphs** for ambient visibility, rain, and wiper motion in the same change
(Phase 5 reserved `Icon` / `Swatch` slots). Shutdown / disband / ROB polish stay out of scope.

## Why

Today:

- Diagnostics already emit `RainChanged` / `WiperMotionChanged`; Driver Notice formats them.
- Live `VehicleContext` has `wiper` (`Off` / `Ready` / `Running`) but **no durable rain**.
- `PublishedVehicleContext` omits `wiper` and has no weather field → Driver weather line and
  Engineer wiper stay `—`.
- Ledger maps `RainsStarted` / `RainsStopped` to published `TimerTick` (placeholder).

Operators cannot see standing rain/wiper status on status lines after unrelated hops (RPM/lux).

## Principles

1. **Wide streams, selective collectors** — Twin emits rich facts on diagnostics *and* ledger;
   Dashboard (and future subscribers) filter and format. Do not thin the stream to one UI.
2. **Common published structs are source of truth** — Every durable / event field that
   observation JSON carries must exist on the live `common` published record types first.
   Observation schema DTOs are deliberate mirrors of those structs: **what exists in the
   Rust structs must exist in JSON; the schema must conform to the structs** (not invent
   wire-only fields, and not leave published struct fields unprojected).
3. **Presentation is receiver-side** — No icons in Twin payloads; glyphs live in
   `tui_dashboard` view segments.
4. **YAGNI** — No shutdown lifecycle, ROB depth, lux colour swatches, Notice colour tokens,
   or glyph animation in this change.

## Approach (chosen)

**Approach 3 — twin + publish + dashboard glyphs (schema v3).**

| Rejected | Why |
|----------|-----|
| Dashboard-only latch from diagnostics | Weather goes stale after unrelated ledger hops; fights wide-stream principle |
| Data enrichment without glyphs | User prefers glyphs now; `Icon` slot already reserved and wiring is small |
| Collapse wiper to Off/On on the ledger | Ledger is for engineers — keep full `Off` / `Ready` / `Running` |

## Twin model

### `WeatherContext`

New L1 context on `VehicleContext`:

```rust
pub struct WeatherContext {
    pub raining: bool, // default false
}

pub struct VehicleContext {
    pub powertrain: PowertrainContext,
    pub health: VehicleHealthContext,
    pub visibility: VisibilityContext,
    pub weather: WeatherContext, // NEW
    pub headlamp: HeadlampContext,
    pub wiper: WiperContext,
}
```

Chosen over a bare `VehicleContext.raining` or stuffing rain into `VisibilityContext` so
future weather fields stay clean without overloading lux.

### Emission on rain edges

When processing `FsmEvent::RainsStarted` / `RainsStopped`:

1. Set `weather.raining` to `true` / `false` on the vehicle context **before** (or as part of)
   building that hop’s transition record, so ledger `old_ctx` / `current_ctx` already carry
   the durable flag — not only a post-emit side effect.
2. Keep existing diagnostic emits: `diag_rain_changed(…)`.
3. Keep existing wiper motion diagnostic when `WiperState::Running` membership changes.

Wiper context continues to update via the existing zone-turn path; no change to wiper FSM.

Live twin types (`VehicleContext`, `WeatherContext`, `WiperContext`) and published observation
types (`PublishedVehicleContext`, …) are both updated: published structs are projections of
live context, and JSON mirrors published structs.

## Published ledger (`common`)

Update published types so standing status is available on every hop’s `old_ctx` /
`current_ctx`:

```rust
pub struct PublishedWeatherContext {
    pub raining: bool,
}

pub enum PublishedWiperState {
    Off,
    Ready,
    Running,
}

pub struct PublishedWiperContext {
    pub state: PublishedWiperState,
}

pub struct PublishedVehicleContext {
    pub powertrain: PublishedPowertrainContext,
    pub health: PublishedHealthContext,
    pub visibility: PublishedVisibilityContext,
    pub weather: PublishedWeatherContext, // NEW
    pub headlamp: PublishedHeadlampContext,
    pub wiper: PublishedWiperContext,     // NEW
}
```

### Published FSM events

Add real variants; **stop** mapping rain to `TimerTick`:

```rust
// PublishedFsmEvent — add:
RainsStarted,
RainsStopped,
```

`AssemblyZoneReady` may remain a neutral placeholder if still unpublished; rain must not.

Domain actions `RequestWiperStart` / `RequestWiperStop` already exist — unchanged.

### Diagnostics

No new `DiagnosticKind` variants required. Keep:

- `RainChanged { raining }`
- `WiperMotionChanged { wiping }`

## Observation schema (v3)

Bump `CURRENT_SCHEMA_VERSION` to **3**.

Archival / live-wire DTOs in `observation` must mirror the published `common` shapes
field-for-field (same names/semantics after existing snake_case JSON conventions):

- `VehicleContextV1`: add `weather`, `wiper`
- `FsmEventV1`: add `rains_started`, `rains_stopped`
- Exhaustive project / `from_envelope` arms — compile-fail on new live variants

**Conformity rule:** if a field is on `PublishedVehicleContext` / `PublishedFsmEvent` /
`DiagnosticKind`, it has a JSON counterpart in schema v3. Golden and round-trip tests assert
this. Do not leave published struct fields out of JSON “because the Dashboard ignores them.”

File tee and live UDS/Zenoh envelopes all carry `schema_version: 3`.

## Dashboard presentation

### Driver

| Line | Source | Presentation |
|------|--------|----------------|
| Notice | Latest diagnostic (filtered) | Unchanged |
| Speed | Ledger `powertrain.speed_kph` | Existing zoned bar |
| Visibility | Ledger `visibility.ambient_lux` + headlamp | Glyph band + lux number + headlamp **text** |
| Weather | Ledger `weather.raining` + wiper | Glyphs only for values |

**Vertical rhythm:** insert one blank pane line between logical segments (Notice / Speed /
Visibility / Weather) so the Diagnostic pane is easier to scan. Blank lines are presentation
only (`LineRole` spacer or equivalent empty `PaneLine`); they carry no Twin data. Standby
(pre-PowerOn) copy may keep its current tight stacking unless it looks cramped in smoke.

Example stack (content abbreviated):

```text
Notice: …
<blank>
Speed: […] N/160 km/h
<blank>
Visibility: ◼ (120 lux)  Headlamps: On
<blank>
Weather: ☁  Wipers: ≋
```

**Visibility ambient bands** (from `common` `LUX_ON_THRESHOLD` / `LUX_OFF_THRESHOLD`):

| Band | Condition | Glyph |
|------|-----------|--------|
| Dark | `lux <= LUX_ON` (840) | `◼` |
| Hold | `(LUX_ON, LUX_OFF)` | `▦` |
| Bright | `lux >= LUX_OFF` (860) | `◻` |

Example: `Visibility: ◼ (120 lux)  Headlamps: On`

**Weather / wiper glyphs** (distinct families — no shared metaphor with visibility):

| Fact | Glyph |
|------|--------|
| Dry | `☀` |
| Raining | `☁` |
| Wipers Off | `x` (ASCII cross) — wiper `Off` or `Ready` |
| Wipers On | `≋` — wiper `Running` only |

Example: `Weather: ☁  Wipers: ≋`

Implement via `SegmentContent::Icon` (give it a small payload enum); count width with
`unicode_width`; paint in `main` like `SpeedBar`. Glyph **replaces** rain/wiper words on
Driver (avoid glyph+text overload). Lux **keeps** the numeric.

### Engineer (text only, full fidelity)

- Weather: `Raining` / `Dry` from `current_ctx.weather.raining`
- Sub-assembly Wiper: `Off` / `Ready` / `Running` (replace `—`)
- Last event: real `RainsStarted` / `RainsStopped` once published

### Glyph families (summary)

| Domain | Glyphs |
|--------|--------|
| Ambient lux | `◼` `▦` `◻` |
| Rain | `☀` `☁` |
| Wipers | `x` `≋` |

## Error handling / compatibility

- Schema bump is intentional: v2 readers are not required to accept v3.
- Projection failures (session/vehicle mismatch) unchanged.
- Terminals that mangle BMP symbols: keep mapping in one Dashboard helper so ASCII fallback
  can be added later without Twin changes.

## Tests

- `WeatherContext` default + rain edge updates durable `raining`
- Published projection includes `weather` + `wiper`; rain events are not `TimerTick`
- Observation schema v3 golden / round-trip / compatibility tests updated
- Existing rain↔wiper diagnostic contracts remain green
- Driver: lux band glyphs, rain glyphs, wiper glyphs; weather line no longer `—`;
  blank spacer lines between Notice / Speed / Visibility / Weather
- Engineer: wiper full state; weather Raining/Dry text

## Out of scope

- Shutdown / disband / Twin lifecycle polish (Phase 10)
- ROB depth on Engineer
- Coloured visibility `Swatch` (brown/yellow) — unicode boxes only this pass
- Notice colour tokens; glyph animation
- Push to remote unless explicitly asked

## Success criteria

1. After rain on CAN, Driver shows `☁` (and wiper `≋` when Running); after rain clear, `☀` / `x`.
2. Visibility line shows `◼`/`▦`/`◻` consistent with lux vs thresholds, plus lux number.
3. Engineer shows durable weather text and full wiper assembly state from ledger context.
4. Observation JSON for ledger `current_ctx` contains `weather` and `wiper` matching
   `PublishedVehicleContext`; rain events appear as named variants in JSON.
5. Diagnostics still carry rain/wiper edge facts for collectors that prefer them.
)
