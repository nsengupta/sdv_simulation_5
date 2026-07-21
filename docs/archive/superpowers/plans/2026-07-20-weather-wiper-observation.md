# Weather + Wiper Observation (Dashboard Display) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Publish durable rain + full wiper state on the transition ledger (and keep rain/wiper diagnostics), bump observation schema to v3 so JSON mirrors `common` published structs, and render Driver glyphs + blank segment spacers (Engineer text fidelity).

**Architecture:** Add L1 `WeatherContext` on live `VehicleContext`; set `raining` in `zone_turn` on rain edges before hop records are built. Project `weather` + `wiper` into `PublishedVehicleContext` and real `RainsStarted`/`RainsStopped` into `PublishedFsmEvent`. Mirror field-for-field in observation DTOs (`schema_version: 3`). Dashboard reads ledger context for status lines; `SegmentContent::Icon` paints glyphs; blank `LineRole::Spacer` rows between Driver segments.

**Tech Stack:** Rust workspace (`common`, `observation`, `tui_dashboard`), existing observation live/file tee (UDS|Zenoh unchanged), Ratatui segment paint in `tui_dashboard`.

**Spec:** [`docs/superpowers/specs/2026-07-20-weather-wiper-observation-design.md`](../specs/2026-07-20-weather-wiper-observation-design.md)

## Global Constraints

- Emit on **both** diagnostics and ledger; collectors filter (do not remove diagnostic kinds).
- **Common published structs are source of truth** — every JSON field mirrors a published Rust field; no wire-only invention; no unpublished struct fields.
- Ledger wiper: full `Off` / `Ready` / `Running`. Driver collapses motion to glyphs (`Running` → `≋`, else `x`).
- Driver rain glyphs: `☀` dry / `☁` raining (no rain text beside glyph). Visibility: `◼`/`▦`/`◻` + lux number; headlamps **text**.
- Driver **blank** spacers between Notice / Speed / Visibility / Weather (not horizontal rules). Engineer: no new spacers.
- Schema **v3**. File tee + live UDS/Zenoh all carry `schema_version: 3`.
- Out of scope: shutdown/disband, ROB depth, coloured Swatch, Notice colours, glyph animation, push to remote.
- **Do not push.** Commit at task boundaries when the user asks, or leave a clean tree for review; do not push unless asked.
- Every task’s requirements implicitly include this section.

## File structure

| Path | Responsibility |
|------|----------------|
| `crates/common/src/vehicle_state/weather.rs` | `WeatherContext { raining: bool }` |
| `crates/common/src/vehicle_state/mod.rs` | Export weather; add field on `VehicleContext` |
| `crates/common/src/twin_runtime/zone_turn.rs` | Set `weather.raining` on `RainsStarted`/`RainsStopped` |
| `crates/common/src/observation_records/transition/mod.rs` | `PublishedWeather*` / `PublishedWiper*` / rain events; project |
| `crates/common/src/facade.rs` | Re-export new published types |
| `crates/common/src/lib.rs` | Re-export `WeatherContext` if other crates need it |
| `crates/common/src/test/wiper_actuation_contract.rs` | Assert durable raining on ledger (extend) |
| `crates/observation/src/schema/mod.rs` | `CURRENT_SCHEMA_VERSION = 3` |
| `crates/observation/src/schema/v1.rs` | DTO + project/from_envelope for weather/wiper/rain events |
| `crates/observation/tests/support/mod.rs` | Sample ledger includes weather + wiper |
| `crates/observation/testdata/golden/v3/…` | New golden; keep or stop using v2 path |
| `crates/observation/tests/golden_files.rs` | Point at v3 golden |
| `crates/observation/tests/round_trip.rs` | Expect schema 3 |
| `crates/tui_dashboard/src/view/line.rs` | `Icon` payload enum; `LineRole::Spacer`; width |
| `crates/tui_dashboard/src/view/driver.rs` | Glyphs, spacers, weather/visibility lines |
| `crates/tui_dashboard/src/view/engineer.rs` | Weather text + full wiper state |
| `crates/tui_dashboard/src/main.rs` | Paint `Icon` segments |
| Call sites constructing `PublishedVehicleContext` / `VehicleContext` | Add new fields (compile gate) |

---

### Task 1: `WeatherContext` + durable rain in `zone_turn`

**Files:**
- Create: `crates/common/src/vehicle_state/weather.rs`
- Modify: `crates/common/src/vehicle_state/mod.rs`
- Modify: `crates/common/src/twin_runtime/zone_turn.rs` (`RainsStarted` / `RainsStopped` arms)
- Modify: any `VehicleContext { … }` struct literals in `crates/common` that do not use `..Default::default()` (fix helpers)
- Test: add unit test in `weather.rs` and/or extend `crates/common/src/test/wiper_actuation_contract.rs`

**Interfaces:**
- Produces:

```rust
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WeatherContext {
    pub raining: bool, // Default::default() => false
}

// VehicleContext gains:
pub weather: WeatherContext,
```

- `zone_turn` on `RainsStarted`: `next.weather.raining = true;` then existing wiper merge.
- `zone_turn` on `RainsStopped`: `next.weather.raining = false;` then existing wiper merge.

- [ ] **Step 1: Write failing test** in `crates/common/src/vehicle_state/weather.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::fsm::{FsmEvent, FsmState};
    use crate::twin_runtime::zone_replies::ZoneReplies;
    use crate::twin_runtime::zone_turn::zone_turn;
    use crate::vehicle_state::VehicleContext;
    use std::time::Instant;

    #[test]
    fn rains_started_sets_weather_raining_true() {
        let ctx = VehicleContext::default();
        assert!(!ctx.weather.raining);
        let result = zone_turn(
            &ctx,
            &FsmEvent::RainsStarted,
            &FsmState::Idle,
            Instant::now(),
            &ZoneReplies::simulate_locally(),
        );
        assert!(result.ctx.weather.raining);
    }

    #[test]
    fn rains_stopped_clears_weather_raining() {
        let mut ctx = VehicleContext::default();
        ctx.weather.raining = true;
        let result = zone_turn(
            &ctx,
            &FsmEvent::RainsStopped,
            &FsmState::Idle,
            Instant::now(),
            &ZoneReplies::simulate_locally(),
        );
        assert!(!result.ctx.weather.raining);
    }
}
```

Use `crate::twin_runtime::zone_replies::ZoneReplies` (or the path exported by `twin_runtime`). Do **not** invent `ZoneReplies::default()` — use `simulate_locally()`.

- [ ] **Step 2: Run test — expect fail** (no `weather` field / raining not set)

```bash
cargo test -p common rains_started_sets_weather_raining_true -- --nocapture
```

Expected: compile error or assertion failure.

- [ ] **Step 3: Implement `WeatherContext`, wire `VehicleContext`, set raining in `zone_turn`**

```rust
// weather.rs
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WeatherContext {
    pub raining: bool,
}
```

In `mod.rs`: `pub mod weather;` + `pub use weather::WeatherContext;` + add `pub weather: WeatherContext` to `VehicleContext`.

In `zone_turn.rs`:

```rust
FsmEvent::RainsStarted => {
    next.weather.raining = true;
    let zone_reply = merge_wiper_for_message(ctx, WiperMessage::Start, wiper_ingress);
    next.wiper = zone_reply.ctx;
    outcomes.extend(zone_reply.outcomes.into_iter().map(ZoneOutcome::Wiper));
}
FsmEvent::RainsStopped => {
    next.weather.raining = false;
    let zone_reply = merge_wiper_for_message(ctx, WiperMessage::Stop, wiper_ingress);
    next.wiper = zone_reply.ctx;
    outcomes.extend(zone_reply.outcomes.into_iter().map(ZoneOutcome::Wiper));
}
```

Fix all `VehicleContext { … }` literals in the workspace that break compile (prefer `weather: WeatherContext::default()` or `..Default::default()`).

- [ ] **Step 4: Run tests**

```bash
cargo test -p common rains_started_sets_weather_raining_true rains_stopped_clears_weather_raining
cargo test -p common --lib
```

Expected: PASS.

- [ ] **Step 5: Commit** (when user asks, or leave staged note)

```bash
git add crates/common/src/vehicle_state/weather.rs crates/common/src/vehicle_state/mod.rs \
  crates/common/src/twin_runtime/zone_turn.rs
# plus any VehicleContext call-site fixes
git commit -m "feat(common): add WeatherContext and durable raining on rain edges"
```

---

### Task 2: Published ledger types — weather, wiper, rain events

**Files:**
- Modify: `crates/common/src/observation_records/transition/mod.rs`
- Modify: `crates/common/src/facade.rs` (re-exports)
- Modify: `crates/common/src/lib.rs` if it re-exports published types
- Fix all `PublishedVehicleContext { … }` literals across the workspace
- Test: unit tests in `transition/mod.rs` or extend `wiper_actuation_contract.rs`

**Interfaces:**
- Produces:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublishedWeatherContext {
    pub raining: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PublishedWiperState {
    Off,
    Ready,
    Running,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublishedWiperContext {
    pub state: PublishedWiperState,
}

// PublishedVehicleContext gains weather + wiper
// PublishedFsmEvent gains RainsStarted, RainsStopped
// From<&FsmEvent>: map RainsStarted/RainsStopped to those variants (NOT TimerTick)
// AssemblyZoneReady may remain TimerTick placeholder
```

- `PublishedVehicleContext::project` must include:

```rust
weather: PublishedWeatherContext { raining: ctx.weather.raining },
wiper: PublishedWiperContext { state: (&ctx.wiper.state).into() },
```

- [ ] **Step 1: Write failing projection test** in `transition/mod.rs` `#[cfg(test)]`:

```rust
#[test]
fn published_fsm_event_keeps_rain_variants() {
    assert_eq!(
        PublishedFsmEvent::from(&FsmEvent::RainsStarted),
        PublishedFsmEvent::RainsStarted
    );
    assert_eq!(
        PublishedFsmEvent::from(&FsmEvent::RainsStopped),
        PublishedFsmEvent::RainsStopped
    );
}

#[test]
fn published_vehicle_context_includes_weather_and_wiper() {
    let clock = SessionClock::capture();
    let mut ctx = VehicleContext::default();
    ctx.weather.raining = true;
    ctx.wiper.state = WiperState::Running;
    let pub_ctx = PublishedVehicleContext::project(&ctx, &clock);
    assert!(pub_ctx.weather.raining);
    assert_eq!(pub_ctx.wiper.state, PublishedWiperState::Running);
}
```

(Make `project` visible to the test module — `pub(crate)` or test via `PublishedTransitionRecord::project` if `project` stays private.)

- [ ] **Step 2: Run — expect fail**

```bash
cargo test -p common published_fsm_event_keeps_rain_variants -- --nocapture
```

- [ ] **Step 3: Implement published types + projection + fix call sites**

Update `From<&FsmEvent>`:

```rust
FsmEvent::RainsStarted => Self::RainsStarted,
FsmEvent::RainsStopped => Self::RainsStopped,
FsmEvent::AssemblyZoneReady(_) => Self::TimerTick, // still unpublished
```

Remove rain from the old combined `TimerTick` arm.

Export new types from `facade.rs`:

```rust
PublishedWeatherContext, PublishedWiperContext, PublishedWiperState,
```

Fix every `PublishedVehicleContext { … }` in gateway/dashboard/observation/tests to include:

```rust
weather: PublishedWeatherContext { raining: false },
wiper: PublishedWiperContext { state: PublishedWiperState::Off },
```

- [ ] **Step 4: Run**

```bash
cargo test -p common published_fsm_event_keeps_rain_variants published_vehicle_context_includes_weather_and_wiper
cargo test -p common --lib
cargo check -p gateway -p tui_dashboard -p observation
```

Expected: PASS / compile clean.

- [ ] **Step 5: Commit**

```bash
git commit -m "feat(common): publish weather, wiper context, and rain ledger events"
```

---

### Task 3: Observation schema v3 mirrors published structs

**Files:**
- Modify: `crates/observation/src/schema/mod.rs` → `CURRENT_SCHEMA_VERSION = 3`
- Modify: `crates/observation/src/schema/v1.rs` — DTOs + project/from_envelope
- Modify: `crates/observation/tests/support/mod.rs`
- Modify: `crates/observation/tests/round_trip.rs`, `schema_compatibility.rs`, `summary.rs` as needed
- Create: `crates/observation/testdata/golden/v3/<RUN_ID>/…`
- Modify: `crates/observation/tests/golden_files.rs` to use `testdata/golden/v3`

**Interfaces:**
- Produces JSON shapes matching published structs:

```rust
pub struct WeatherContextV1 { pub raining: bool }
pub enum WiperStateV1 { Off, Ready, Running } // serde snake_case
pub struct WiperContextV1 { pub state: WiperStateV1 }
// VehicleContextV1 { …, weather, wiper }
// FsmEventV1::{ RainsStarted, RainsStopped }
```

- Conformity: no published field left out of JSON; no JSON-only fields.

- [ ] **Step 1: Update support sample + failing round-trip expectation**

In `support/mod.rs` add weather/wiper to sample context. In `round_trip.rs` change `assert_eq!(json["schema_version"], 2)` → `3`.

- [ ] **Step 2: Run golden/round_trip — expect fail**

```bash
cargo test -p observation --test round_trip --test golden_files
```

- [ ] **Step 3: Implement schema v3 DTOs and projections**

Bump version constant. Extend `project_vehicle_context` / `live_vehicle_context` / `project_fsm_event` / `live_fsm_event` with exhaustive matches for weather, wiper, rain events.

Regenerate golden:

```bash
UPDATE_OBSERVATION_GOLDEN=1 cargo test -p observation --test golden_files
```

Move/copy under `testdata/golden/v3/…` and point `golden_files.rs` at v3. Leave v2 testdata in tree only if still referenced; otherwise delete to avoid confusion.

- [ ] **Step 4: Run full observation tests**

```bash
cargo test -p observation
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git commit -m "feat(observation): schema v3 weather/wiper and rain events"
```

---

### Task 4: Dashboard Driver glyphs, spacers, Engineer text

**Files:**
- Modify: `crates/tui_dashboard/src/view/line.rs`
- Modify: `crates/tui_dashboard/src/view/driver.rs`
- Modify: `crates/tui_dashboard/src/view/engineer.rs`
- Modify: `crates/tui_dashboard/src/main.rs` (paint `Icon`)
- Modify: test helpers’ `PublishedVehicleContext` in view tests (already done in Task 2 if shared)

**Interfaces:**
- Produces:

```rust
pub enum LineRole { /* existing */, Spacer }

pub enum DriverIcon {
    LuxDark,   // ◼
    LuxHold,   // ▦
    LuxBright, // ◻
    Dry,       // ☀
    Raining,   // ☁
    WiperOff,  // x
    WiperOn,   // ≋
}

pub enum SegmentContent {
    Text(String),
    SpeedBar { cells: Vec<SpeedBarCell> },
    Swatch, // still unused
    Icon(DriverIcon),
}
```

- `DriverIcon::as_str(&self) -> &'static str` for paint + width.
- Lux band helper using `LUX_ON_THRESHOLD` / `LUX_OFF_THRESHOLD` from `common::vehicle_physics`.
- `driver_pane`: Notice, blank Spacer, Speed, blank, Visibility, blank, Weather.
- Wiper glyph: `Running` → `WiperOn`, else `WiperOff`.
- Engineer: `Weather: Raining|Dry`, `Wiper: Off|Ready|Running`.

- [ ] **Step 1: Write failing Driver/Engineer tests**

Replace `driver_rain_and_wipers_share_a_placeholder_line` with assertions on glyphs and spacers:

```rust
#[test]
fn driver_weather_line_uses_glyphs_from_ledger() {
    let mut ledger = sample_ledger(10, 100, PublishedHeadlampState::Off);
    ledger.current_ctx.weather.raining = true;
    ledger.current_ctx.wiper.state = PublishedWiperState::Running;
    let pane = driver_pane(None, Some(&ledger), 64);
    let weather = pane.lines.iter().find(|l| l.role == LineRole::Weather).unwrap();
    assert!(weather.text().contains('☁'));
    assert!(weather.text().contains('≋'));
    assert!(!weather.text().contains('—'));
}

#[test]
fn driver_inserts_blank_spacers_between_segments() {
    let ledger = sample_ledger(10, 100, PublishedHeadlampState::Off);
    let pane = driver_pane(None, Some(&ledger), 64);
    let roles: Vec<_> = pane.lines.iter().map(|l| l.role).collect();
    assert!(roles.windows(2).any(|w| w[0] == LineRole::Notice && w[1] == LineRole::Spacer));
    assert!(roles.windows(2).any(|w| w[0] == LineRole::Speed && w[1] == LineRole::Spacer));
    assert!(roles.windows(2).any(|w| w[0] == LineRole::Visibility && w[1] == LineRole::Spacer));
}

#[test]
fn driver_visibility_lux_band_glyph() {
    let ledger = sample_ledger(10, 100, PublishedHeadlampState::On); // dark band
    let pane = driver_pane(None, Some(&ledger), 64);
    let vis = pane.lines.iter().find(|l| l.role == LineRole::Visibility).unwrap();
    assert!(vis.text().contains('◼'));
    assert!(vis.text().contains("100 lux"));
}

#[test]
fn engineer_shows_weather_and_full_wiper() {
    let mut row = sample_ledger();
    row.current_ctx.weather.raining = true;
    row.current_ctx.wiper.state = PublishedWiperState::Ready;
    let pane = engineer_pane(Some(&row), 48);
    assert!(pane.lines.iter().any(|l| l.text().contains("Raining")));
    assert!(pane.lines.iter().any(|l| l.text().contains("Wiper: Ready")));
}
```

(Adapt `sample_ledger` helpers to include weather/wiper fields.)

- [ ] **Step 2: Run — expect fail**

```bash
cargo test -p tui_dashboard driver_weather_line_uses_glyphs_from_ledger driver_inserts_blank_spacers_between_segments
```

- [ ] **Step 3: Implement view + paint**

`line.rs`: extend `Icon(DriverIcon)`; `text()` / `display_width()` include `icon.as_str()`; add `LineRole::Spacer`; helper `PaneLine::spacer(width)` pads spaces to width.

`driver.rs`: build visibility/weather with `Segment` lists mixing `Text` + `Icon`; insert spacers.

`engineer.rs`: replace wiper `—`; add weather line (or fold into assembly block — prefer one clear `Weather: Raining|Dry` line plus `Wiper: …`).

`main.rs` paint arm:

```rust
SegmentContent::Icon(icon) => {
    span = Span::styled(icon.as_str(), style_for(seg.style));
}
```

- [ ] **Step 4: Run**

```bash
cargo test -p tui_dashboard
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git commit -m "feat(dashboard): weather/wiper/visibility glyphs and Driver spacers"
```

---

### Task 5: Docs touch + workspace verify

**Files:**
- Modify: `docs/PHASES.md` — mark deferred rain/wiper Dashboard fields as addressed (or add a short “Weather/wiper observation” note under Phase 5 follow-up / post–Phase 9)
- Optional one-liner in `docs/ARCHITECTURE-OVERVIEW.md` if observation schema version is documented as 2

- [ ] **Step 1: Update PHASES deferred TODO** that says Twin rain/wiper on Dashboard lines — mark done / point at this design.

- [ ] **Step 2: Full verify**

```bash
cargo test -p common -p observation -p tui_dashboard -p gateway
```

Expected: PASS.

- [ ] **Step 3: Commit docs**

```bash
git commit -m "docs: note weather/wiper observation on Dashboard streams"
```

---

## Spec coverage (self-review)

| Spec requirement | Task |
|------------------|------|
| `WeatherContext` on live `VehicleContext` | 1 |
| Set raining before/in hop context (`zone_turn`) | 1 |
| Keep `RainChanged` / `WiperMotionChanged` diagnostics | (already present; no removal) |
| Publish weather + full wiper on ledger ctx | 2 |
| Real `RainsStarted`/`RainsStopped` published events | 2 |
| Schema v3 JSON mirrors published structs | 3 |
| Driver glyphs lux/rain/wiper | 4 |
| Blank spacers Driver-only | 4 |
| Engineer text weather + full wiper | 4 |
| Out of scope items not implemented | all tasks |
| Common structs ↔ JSON conformity | 2 + 3 |

## Placeholder / type consistency check

- `PublishedWiperState` / `WiperStateV1` / live `WiperState` naming aligned across tasks.
- `DriverIcon` is Dashboard-only; not on the wire.
- Golden path is **v3**, not v2.
)
