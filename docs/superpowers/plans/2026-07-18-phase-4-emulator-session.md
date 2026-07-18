# Phase 4 Emulator Session (Mode 1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Refactor the emulator around `TelemetrySource` + a session runner so Mode 1 stops via optional `--readings N` or Ctrl+C through one shared `controlled_stop` trailer.

**Architecture:** Keep work inside `crates/emulator`. Extract tick fields and a `TelemetrySource` trait; `LivePhysicsSource` wraps `PhysicalCar`. `run_session` sends PowerOn, loops ticks (wire order unchanged), and on stop sends `EngineRpm(0)` then `PowerOff`. The binary installs a Ctrl+C flag the session loop polls (including during inter-tick sleep). Mode 2 / golden / CI remain documentation TODOs only.

**Tech Stack:** Rust stable, existing `anyhow` + `socketcan`, `ctrlc` for SIGINT, `std::sync::atomic` stop flag, mock `FrameSink` in tests (no `vcan0`).

## Global Constraints

- Stay inside `crates/emulator`; do not create `emulator-core`.
- Do not implement Mode 2 file reading, tick-file generator, `observation-compare`, or CI workflows.
- Preserve wire order: PowerOn; per tick `EngineRpm`, `AmbientLux`, `RainDetected`; trailer `EngineRpm(0)` then PowerOff.
- Preserve `EMULATOR_TUNNEL_PROB` / `EMULATOR_RAIN_PROB` behavior.
- `--readings` is optional: omit → run until stop flag; present with `N >= 1` → stop after N ticks; both use the same trailer.
- Emulator guarantees transmission order, not Twin PowerOff acceptance.
- Ctrl+C handling is emulator-only; do not change Dashboard signal handling.
- Session sleep must be interruptible so Ctrl+C is not stuck for a full 100 ms tick.
- Do not stage or commit changes unless the user separately requests it.
- Every task’s requirements implicitly include this section.

## File structure

| Path | Responsibility |
|------|----------------|
| `crates/emulator/src/tick.rs` | `TickFields` + conversion to the three `VssSignal`s |
| `crates/emulator/src/source.rs` | `TelemetrySource` trait + `LivePhysicsSource` |
| `crates/emulator/src/runner.rs` | `SessionConfig`, `run_session`, `controlled_stop`, interruptible sleep helper; remove finite-only API |
| `crates/emulator/src/cli.rs` | Optional `--readings` → `Option<NonZeroUsize>` |
| `crates/emulator/src/main.rs` | Ctrl+C flag + compose live source + `run_session` |
| `crates/emulator/src/lib.rs` | Export new modules |
| `crates/emulator/Cargo.toml` | Add `ctrlc` |
| `crates/emulator/tests/finite_run_contract.rs` | Rename/update to session contracts (readings + stop flag) |
| `README.md`, `docs/PHASES.md`, `docs/ARCHITECTURE-OVERVIEW.md` | CLI, Phase 4 scope, G4/G7 TODOs |
| `docs/superpowers/specs/2026-07-18-phase-4-emulator-session-design.md` | Mark Status: Approved |

---

### Task 1: Optional `--readings` CLI

**Files:**
- Modify: `crates/emulator/src/cli.rs`
- Modify: `crates/emulator/src/main.rs` (only if needed to compile after arg type change — prefer deferring session wiring to Task 4; keep `main` compiling by adapting the temporary `run_finite` call or leave a short bridge)

**Interfaces:**
- Produces: `EmulatorArgs { readings: Option<NonZeroUsize> }`
- Produces: `parse_args(...) -> Result<EmulatorArgs>`
- Consumes: none from later tasks

- [ ] **Step 1: Rewrite the CLI unit test for optionality**

Replace `readings_argument_is_required_and_positive` with:

```rust
#[test]
fn readings_argument_is_optional_and_positive_when_present() {
    let overflowing_readings = format!("{}0", usize::MAX);

    assert_eq!(parse_args(std::iter::empty::<&str>()).unwrap().readings, None);
    assert_eq!(
        parse_args(["--readings", "30"]).unwrap().readings,
        NonZeroUsize::new(30)
    );
    assert!(parse_args(["--readings", "0"]).is_err());
    assert!(parse_args(["--readings", "abc"]).is_err());
    assert!(parse_args(["--readings", overflowing_readings.as_str()]).is_err());
    assert!(parse_args(["--readings", "30", "extra"]).is_err());
    assert!(parse_args(["--readings"]).is_err());
    assert!(parse_args(["--unknown"]).is_err());
}
```

- [ ] **Step 2: Run CLI tests and verify failure**

Run:

```bash
cargo test -p emulator --lib cli::tests
```

Expected: FAIL because empty args still error and `readings` is not `Option`.

- [ ] **Step 3: Implement optional parsing**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmulatorArgs {
    pub readings: Option<NonZeroUsize>,
}

pub fn parse_args<I, S>(args: I) -> Result<EmulatorArgs>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let values: Vec<String> = args
        .into_iter()
        .map(|value| value.as_ref().to_string())
        .collect();
    if values.is_empty() {
        return Ok(EmulatorArgs { readings: None });
    }
    if values.len() != 2 || values[0] != "--readings" {
        bail!("usage: emulator [--readings <positive integer>]");
    }
    let parsed = values[1]
        .parse::<usize>()
        .with_context(|| format!("invalid --readings value {:?}", values[1]))?;
    let readings = NonZeroUsize::new(parsed).context("--readings must be greater than zero")?;
    Ok(EmulatorArgs {
        readings: Some(readings),
    })
}
```

Temporarily keep `main` compiling: if `args.readings` is `None`, map to a placeholder only if `run_finite` still requires `NonZeroUsize` — **do not** invent a huge default. Prefer updating `main` in this task to:

```rust
// Bridge until Task 3/4: require readings for the old runner path is unacceptable.
// Instead, change main to match Option after Task 3. For Task 1 only, update
// run_finite call site to:
let readings = args.readings.context(
    "internal: session runner not wired; pass --readings until Task 4 completes",
)?;
run_finite(&mut sink, &mut car, readings, thread::sleep)
```

That bridge is only acceptable inside an unfinished workspace for minutes. **Preferred:** complete Task 1 CLI + tests, then immediately continue Task 2–3 in the same implementation session so `main` never ships the bridge. If committing per task, Task 1 commit must leave `cargo test -p emulator` green — so either (a) fold a minimal `run_session` stub into Task 1, or (b) keep Task 1 as CLI-only with the temporary `context` bridge and Task 3 removing it the same day.

**Chosen approach for this plan:** Task 1 uses the temporary `anyhow::Context` bridge in `main` so lib tests pass and binary still runs with `--readings`. Task 4 removes the bridge.

- [ ] **Step 4: Run tests**

```bash
cargo test -p emulator --lib cli::tests
cargo test -p emulator
```

Expected: PASS (integration tests still use `run_finite` with explicit N).

- [ ] **Step 5: Commit (only if user requested commits)**

```bash
git add crates/emulator/src/cli.rs crates/emulator/src/main.rs
git commit -m "$(cat <<'EOF'
feat(emulator): make --readings optional for Mode 1 sessions

EOF
)"
```

---

### Task 2: `TickFields` and `LivePhysicsSource`

**Files:**
- Create: `crates/emulator/src/tick.rs`
- Create: `crates/emulator/src/source.rs`
- Modify: `crates/emulator/src/lib.rs`
- Modify: `crates/emulator/src/car_physics.rs` (optional thin helper; prefer calling existing `update_and_read` from the source)

**Interfaces:**
- Produces:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TickFields {
    pub rpm: u16,
    pub ambient_lux: u16,
    pub rain_detected: bool,
}

impl TickFields {
    pub fn to_signals(self) -> [common::VssSignal; 3];
}

pub trait TelemetrySource {
    fn next_tick(&mut self) -> anyhow::Result<Option<TickFields>>;
}

pub struct LivePhysicsSource {
    car: PhysicalCar,
}

impl LivePhysicsSource {
    pub fn new(car: PhysicalCar) -> Self;
    pub fn from_config(cfg: PhysicalWorldModelConfig) -> Self;
}

impl TelemetrySource for LivePhysicsSource {
    fn next_tick(&mut self) -> anyhow::Result<Option<TickFields>>;
}
```

- Consumes: `PhysicalCar::update_and_read`

- [ ] **Step 1: Write failing unit tests in `source.rs`**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::PhysicalWorldModelConfig;

    #[test]
    fn live_source_emits_usual_fields_each_tick() {
        let mut source = LivePhysicsSource::from_config(
            PhysicalWorldModelConfig::daytime_tunnel_profile(),
        );
        let tick = source.next_tick().unwrap().expect("live source is open-ended");
        let signals = tick.to_signals();
        assert!(matches!(signals[0], common::VssSignal::EngineRpm(_)));
        assert!(matches!(signals[1], common::VssSignal::AmbientLux(_)));
        assert!(matches!(signals[2], common::VssSignal::RainDetected(_)));
    }

    #[test]
    fn tick_fields_signal_order_is_rpm_lux_rain() {
        let tick = TickFields {
            rpm: 1500,
            ambient_lux: 800,
            rain_detected: true,
        };
        assert_eq!(
            tick.to_signals(),
            [
                common::VssSignal::EngineRpm(1500),
                common::VssSignal::AmbientLux(800),
                common::VssSignal::RainDetected(true),
            ]
        );
    }
}
```

- [ ] **Step 2: Run tests and verify failure**

```bash
cargo test -p emulator --lib source::tests
```

Expected: FAIL (modules missing).

- [ ] **Step 3: Implement `tick.rs` and `source.rs`**

```rust
// tick.rs
use common::VssSignal;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TickFields {
    pub rpm: u16,
    pub ambient_lux: u16,
    pub rain_detected: bool,
}

impl TickFields {
    pub fn to_signals(self) -> [VssSignal; 3] {
        [
            VssSignal::EngineRpm(self.rpm),
            VssSignal::AmbientLux(self.ambient_lux),
            VssSignal::RainDetected(self.rain_detected),
        ]
    }
}
```

```rust
// source.rs
use crate::car_physics::PhysicalCar;
use crate::models::PhysicalWorldModelConfig;
use crate::tick::TickFields;
use anyhow::Result;
use common::VssSignal;

pub trait TelemetrySource {
    fn next_tick(&mut self) -> Result<Option<TickFields>>;
}

pub struct LivePhysicsSource {
    car: PhysicalCar,
}

impl LivePhysicsSource {
    pub fn new(car: PhysicalCar) -> Self {
        Self { car }
    }

    pub fn from_config(cfg: PhysicalWorldModelConfig) -> Self {
        Self::new(PhysicalCar::new_with_config(cfg))
    }
}

impl TelemetrySource for LivePhysicsSource {
    fn next_tick(&mut self) -> Result<Option<TickFields>> {
        let [rpm, lux, rain] = self.car.update_and_read();
        let (VssSignal::EngineRpm(rpm), VssSignal::AmbientLux(lux), VssSignal::RainDetected(rain)) =
            (rpm, lux, rain)
        else {
            unreachable!("PhysicalCar::update_and_read always returns rpm/lux/rain");
        };
        Ok(Some(TickFields {
            rpm,
            ambient_lux: lux,
            rain_detected: rain,
        }))
    }
}
```

Export from `lib.rs`:

```rust
pub mod car_physics;
pub mod cli;
pub mod models;
pub mod runner;
pub mod sink;
pub mod source;
pub mod tick;
```

- [ ] **Step 4: Run tests**

```bash
cargo test -p emulator --lib source::tests
cargo test -p emulator
```

Expected: PASS.

- [ ] **Step 5: Commit (only if user requested commits)**

```bash
git add crates/emulator/src/tick.rs crates/emulator/src/source.rs crates/emulator/src/lib.rs
git commit -m "$(cat <<'EOF'
feat(emulator): add TickFields and LivePhysicsSource

EOF
)"
```

---

### Task 3: Session runner and shared `controlled_stop`

**Files:**
- Modify: `crates/emulator/src/runner.rs`
- Modify: `crates/emulator/tests/finite_run_contract.rs` (update to `run_session`; keep filename or rename to `session_run_contract.rs` — if renaming, delete the old file in the same change)
- Modify: `crates/emulator/src/main.rs` (switch off `run_finite` bridge to `run_session` with `|| false` stop flag until Task 4)

**Interfaces:**
- Produces:

```rust
pub const TICK: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionConfig {
    pub max_readings: Option<NonZeroUsize>,
}

pub fn controlled_stop<S: FrameSink>(sink: &mut S) -> Result<()>;

pub fn run_session<S, Src, Sleep, Stopped>(
    sink: &mut S,
    source: &mut Src,
    config: SessionConfig,
    mut sleep: Sleep,
    mut stop_requested: Stopped,
) -> Result<()>
where
    S: FrameSink,
    Src: TelemetrySource,
    Sleep: FnMut(Duration),
    Stopped: FnMut() -> bool;
```

- Consumes: `TelemetrySource`, `TickFields::to_signals`, `FrameSink`
- Removes: `run_finite` (replace all call sites)

**Loop rules (implement exactly):**

1. Write PowerOn.
2. Loop:
   - If `stop_requested()` → break (no new tick).
   - If `max_readings` is `Some(n)` and `emitted >= n.get()` → break.
   - `next_tick()`: `None` → break (Mode 2 EOF seam; live source never returns `None`).
   - Write the three signals from the tick.
   - `emitted += 1`.
   - If stop is already requested or readings limit reached → do not sleep; break.
   - Otherwise `interruptible_sleep(&mut sleep, TICK, &mut stop_requested)`.
3. Call `controlled_stop(sink)` once.
4. `controlled_stop`: write `EngineRpm(0)`, then PowerOff. No second call from elsewhere.

`interruptible_sleep`: sleep in ≤10 ms slices (or poll each slice) until `TICK` elapses or `stop_requested()` is true.

- [ ] **Step 1: Write failing session contract tests**

Replace `finite_run_contract.rs` contents with tests against `run_session` (keep `RecordingSink`):

```rust
use anyhow::{Result, anyhow};
use common::{LifecycleCommand, VssSignal};
use emulator::runner::{SessionConfig, TICK, run_session};
use emulator::sink::FrameSink;
use emulator::source::{LivePhysicsSource, TelemetrySource};
use emulator::tick::TickFields;
use socketcan::CanFrame;
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

#[derive(Default)]
struct RecordingSink {
    frames: Vec<CanFrame>,
    fail_after: Option<usize>,
}

impl FrameSink for RecordingSink {
    fn write_frame(&mut self, frame: CanFrame) -> Result<()> {
        if self.fail_after == Some(self.frames.len()) {
            return Err(anyhow!("injected sink failure"));
        }
        self.frames.push(frame);
        Ok(())
    }
}

struct FixedSource {
    ticks: Vec<TickFields>,
    index: usize,
}

impl TelemetrySource for FixedSource {
    fn next_tick(&mut self) -> Result<Option<TickFields>> {
        if self.index >= self.ticks.len() {
            return Ok(None);
        }
        let tick = self.ticks[self.index];
        self.index += 1;
        Ok(Some(tick))
    }
}

#[test]
fn readings_limit_writes_exact_order_and_count() {
    let mut sink = RecordingSink::default();
    let mut source = LivePhysicsSource::new(emulator::car_physics::PhysicalCar::new());
    let mut sleeps = Vec::new();

    run_session(
        &mut sink,
        &mut source,
        SessionConfig {
            max_readings: Some(NonZeroUsize::new(2).unwrap()),
        },
        |duration| sleeps.push(duration),
        || false,
    )
    .unwrap();

    assert_eq!(sink.frames.len(), 9); // 3N + 3
    assert_eq!(
        LifecycleCommand::from_can_frame(&sink.frames[0]),
        Some(LifecycleCommand::PowerOn)
    );
    assert!(matches!(
        VssSignal::from_can_frame(&sink.frames[1]),
        Some(VssSignal::EngineRpm(_))
    ));
    assert!(matches!(
        VssSignal::from_can_frame(&sink.frames[2]),
        Some(VssSignal::AmbientLux(_))
    ));
    assert!(matches!(
        VssSignal::from_can_frame(&sink.frames[3]),
        Some(VssSignal::RainDetected(_))
    ));
    assert_eq!(
        VssSignal::from_can_frame(&sink.frames[7]),
        Some(VssSignal::EngineRpm(0))
    );
    assert_eq!(
        LifecycleCommand::from_can_frame(&sink.frames[8]),
        Some(LifecycleCommand::PowerOff)
    );
    // interruptible_sleep slices TICK into 10 ms pieces
    assert_eq!(sleeps.len(), 10);
    assert!(sleeps.iter().all(|d| *d == Duration::from_millis(10)));
}

#[test]
fn stop_flag_after_first_tick_writes_trailer_once() {
    let mut sink = RecordingSink::default();
    let mut source = FixedSource {
        ticks: (0..8)
            .map(|i| TickFields {
                rpm: 1000 + i as u16,
                ambient_lux: 800,
                rain_detected: false,
            })
            .collect(),
        index: 0,
    };
    let stop = AtomicBool::new(false);
    let mut emitted = 0usize;

    run_session(
        &mut sink,
        &mut source,
        SessionConfig { max_readings: None },
        |_| {
            // After the runner sleeps post-tick-1, request stop.
            emitted += 1;
            if emitted >= 1 {
                stop.store(true, Ordering::SeqCst);
            }
        },
        || stop.load(Ordering::SeqCst),
    )
    .unwrap();

    // PowerOn + 1 triple + Rpm0 + PowerOff = 6 frames
    assert_eq!(sink.frames.len(), 6);
    assert_eq!(
        VssSignal::from_can_frame(&sink.frames[4]),
        Some(VssSignal::EngineRpm(0))
    );
    assert_eq!(
        LifecycleCommand::from_can_frame(&sink.frames[5]),
        Some(LifecycleCommand::PowerOff)
    );
    let power_off_count = sink
        .frames
        .iter()
        .filter(|frame| {
            LifecycleCommand::from_can_frame(frame) == Some(LifecycleCommand::PowerOff)
        })
        .count();
    assert_eq!(power_off_count, 1);
}

#[test]
fn sink_error_stops_the_session() {
    let mut sink = RecordingSink {
        frames: Vec::new(),
        fail_after: Some(1),
    };
    let mut source = LivePhysicsSource::new(emulator::car_physics::PhysicalCar::new());

    let error = run_session(
        &mut sink,
        &mut source,
        SessionConfig {
            max_readings: Some(NonZeroUsize::new(1).unwrap()),
        },
        |_| {},
        || false,
    )
    .unwrap_err();

    assert!(error.to_string().contains("injected sink failure"));
    assert_eq!(sink.frames.len(), 1);
}
```

Note: the stop-flag test relies on sleep callback running once after the first tick. Implement the runner so sleep runs only between ticks when another tick may follow; the first sleep is when the stop flag is set for the next loop-head check.

- [ ] **Step 2: Run contract tests and verify failure**

```bash
cargo test -p emulator --test finite_run_contract
```

Expected: FAIL (`run_session` / `SessionConfig` missing).

- [ ] **Step 3: Implement `runner.rs`**

```rust
use crate::sink::FrameSink;
use crate::source::TelemetrySource;
use crate::tick::TickFields;
use anyhow::Result;
use common::{LifecycleCommand, VssSignal};
use std::num::NonZeroUsize;
use std::time::Duration;

pub const TICK: Duration = Duration::from_millis(100);
const SLEEP_SLICE: Duration = Duration::from_millis(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionConfig {
    pub max_readings: Option<NonZeroUsize>,
}

pub fn controlled_stop<S: FrameSink>(sink: &mut S) -> Result<()> {
    sink.write_frame(VssSignal::EngineRpm(0).to_can_frame()?)?;
    sink.write_frame(LifecycleCommand::PowerOff.to_can_frame()?)?;
    Ok(())
}

pub fn run_session<S, Src, Sleep, Stopped>(
    sink: &mut S,
    source: &mut Src,
    config: SessionConfig,
    mut sleep: Sleep,
    mut stop_requested: Stopped,
) -> Result<()>
where
    S: FrameSink,
    Src: TelemetrySource,
    Sleep: FnMut(Duration),
    Stopped: FnMut() -> bool,
{
    sink.write_frame(LifecycleCommand::PowerOn.to_can_frame()?)?;

    let mut emitted = 0usize;
    loop {
        if stop_requested() {
            break;
        }
        if let Some(max) = config.max_readings {
            if emitted >= max.get() {
                break;
            }
        }

        let Some(tick) = source.next_tick()? else {
            break;
        };
        write_tick(sink, tick)?;
        emitted += 1;

        let limit_reached = config
            .max_readings
            .is_some_and(|max| emitted >= max.get());
        if stop_requested() || limit_reached {
            break;
        }

        interruptible_sleep(&mut sleep, TICK, &mut stop_requested);
    }

    controlled_stop(sink)
}

fn write_tick<S: FrameSink>(sink: &mut S, tick: TickFields) -> Result<()> {
    for signal in tick.to_signals() {
        sink.write_frame(signal.to_can_frame()?)?;
    }
    Ok(())
}

fn interruptible_sleep<Sleep, Stopped>(
    sleep: &mut Sleep,
    total: Duration,
    stop_requested: &mut Stopped,
) where
    Sleep: FnMut(Duration),
    Stopped: FnMut() -> bool,
{
    let mut remaining = total;
    while remaining > Duration::ZERO {
        if stop_requested() {
            return;
        }
        let slice = remaining.min(SLEEP_SLICE);
        sleep(slice);
        remaining = remaining.saturating_sub(slice);
    }
}
```

Update `main.rs` bridge to:

```rust
use emulator::runner::{SessionConfig, run_session};
use emulator::source::LivePhysicsSource;
// ...
let mut source = LivePhysicsSource::new(car);
run_session(
    &mut sink,
    &mut source,
    SessionConfig {
        max_readings: args.readings,
    },
    thread::sleep,
    || false, // Task 4 wires Ctrl+C
)?;
```

- [ ] **Step 4: Run tests**

```bash
cargo test -p emulator --test finite_run_contract
cargo test -p emulator
```

Expected: PASS. If the stop-flag test flakes on sleep-slice counting, assert frame shape only (PowerOn + one triple + trailer) and that `source.index == 1` after the run.

- [ ] **Step 5: Commit (only if user requested commits)**

```bash
git add crates/emulator/src/runner.rs crates/emulator/src/main.rs crates/emulator/tests/
git commit -m "$(cat <<'EOF'
feat(emulator): add session runner with shared controlled stop

EOF
)"
```

---

### Task 4: Wire Ctrl+C in the binary

**Files:**
- Modify: `crates/emulator/Cargo.toml`
- Modify: `crates/emulator/src/main.rs`

**Interfaces:**
- Consumes: `run_session`, `SessionConfig`, `LivePhysicsSource`, `parse_args`
- Produces: process-global `AtomicBool` set by `ctrlc::set_handler`

- [ ] **Step 1: Add dependency**

In `crates/emulator/Cargo.toml`:

```toml
ctrlc = "3.4"
```

- [ ] **Step 2: Wire the stop flag in `main`**

```rust
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

fn main() -> Result<()> {
    let args = parse_args(env::args().skip(1))?;
    let mut cfg = PhysicalWorldModelConfig::daytime_tunnel_profile();
    // ... existing env probability overrides unchanged ...

    let stop = Arc::new(AtomicBool::new(false));
    let stop_for_handler = Arc::clone(&stop);
    ctrlc::set_handler(move || {
        stop_for_handler.store(true, Ordering::SeqCst);
    })?;

    let mut sink = SocketCanSink::open("vcan0")?;
    let mut source = LivePhysicsSource::from_config(cfg);
    run_session(
        &mut sink,
        &mut source,
        SessionConfig {
            max_readings: args.readings,
        },
        thread::sleep,
        || stop.load(Ordering::SeqCst),
    )
}
```

Remove the Task 1 `context` bridge and any remaining `run_finite` / direct `PhysicalCar` usage in `main`.

- [ ] **Step 3: Compile and run unit/integration tests**

```bash
cargo test -p emulator
cargo build -p emulator
```

Expected: PASS / SUCCESS.

- [ ] **Step 4: Manual smoke note (operator)**

Document in the commit message / README (Task 5) that manual verification is:

```bash
# with vcan0 + actuators + tui_dashboard already running
cargo run -p emulator -- --readings 5
# or:
cargo run -p emulator
# Ctrl+C → expect Rpm0 + PowerOff on the bus / Twin reaction in Dashboard
```

Do not automate SocketCAN in this task.

- [ ] **Step 5: Commit (only if user requested commits)**

```bash
git add crates/emulator/Cargo.toml crates/emulator/src/main.rs Cargo.lock
git commit -m "$(cat <<'EOF'
feat(emulator): stop Mode 1 sessions on Ctrl+C

EOF
)"
```

---

### Task 5: Documentation and phase status

**Files:**
- Modify: `README.md` (How to Run emulator section)
- Modify: `docs/PHASES.md` (Phase 4 scope/status/checkboxes)
- Modify: `docs/ARCHITECTURE-OVERVIEW.md` (emulator row + G4/G7 notes)
- Modify: `docs/superpowers/specs/2026-07-18-phase-4-emulator-session-design.md` (Status → Approved)

**Interfaces:** none (docs only)

- [ ] **Step 1: Update README emulator commands**

Replace “`--readings N` is required…” with:

```markdown
# Terminal 4 — Mode 1 emulator (PowerOn, live ticks, controlled stop)
cargo run -p emulator -- --readings 30
# or open-ended until Ctrl+C (emulator process only):
cargo run -p emulator
```

State clearly:

- optional `--readings N` stops after N ticks via Rpm(0)+PowerOff;
- omitting it runs until Ctrl+C, then the same trailer;
- Ctrl+C is handled by the emulator only.

- [ ] **Step 2: Rewrite Phase 4 section in `docs/PHASES.md`**

Set Phase 4 status to match implementation progress (`In progress` while coding, `Done` only after manual smoke). Scope bullets must match the design:

1. Emulator `TelemetrySource` + session runner
2. Optional `--readings` + Ctrl+C shared controlled stop
3. Unit/session tests without `vcan0`
4. Explicit TODOs: Mode 2 file source, tick-file generator, semantic golden / `observation-compare`, SocketCAN CI job

Mark original “observation-compare golden gate” items as deferred TODOs, not Phase 4 Done requirements.

- [ ] **Step 3: Update architecture gap notes**

In `docs/ARCHITECTURE-OVERVIEW.md`:

- Emulator row: finite required-N → Mode 1 session (optional N / Ctrl+C); file Mode 2 deferred
- G4: still deferred (Mode 2 TODO behind `TelemetrySource`)
- G7: still open (observation golden TODO)

- [ ] **Step 4: Mark design spec approved**

In the Phase 4 design doc header: `**Status:** Approved`

- [ ] **Step 5: Verification commands**

```bash
cargo test -p emulator
cargo test --workspace
```

Expected: PASS.

- [ ] **Step 6: Commit (only if user requested commits)**

```bash
git add README.md docs/PHASES.md docs/ARCHITECTURE-OVERVIEW.md \
  docs/superpowers/specs/2026-07-18-phase-4-emulator-session-design.md
git commit -m "$(cat <<'EOF'
docs: record Phase 4 emulator session scope and deferred golden/CI TODOs

EOF
)"
```

---

## Spec coverage checklist

| Spec requirement | Task |
|------------------|------|
| `TelemetrySource` + live physics source | Task 2 |
| Session runner PowerOn → ticks → trailer | Task 3 |
| Shared `controlled_stop` (Rpm0 + PowerOff) | Task 3 |
| Optional `--readings` | Task 1 |
| Ctrl+C emulator-only stop flag + interruptible sleep | Task 3 + 4 |
| Mock-sink tests (readings + stop flag + sink error) | Task 3 |
| CLI tests | Task 1 |
| No Mode 2 / compare / CI implementation | All tasks (docs TODOs in Task 5) |
| README / PHASES / G4 / G7 updates | Task 5 |
| Manual `vcan0` smoke | Task 4 note + Phase 4 acceptance in PHASES |

## Plan self-review

- No Mode 2 implementation sneaks into Tasks 1–4; EOF `None` is only a seam.
- `run_finite` is fully replaced; tests do not keep the old API.
- Stop-flag test uses sleep callback + `AtomicBool` to avoid flaky wall-clock Ctrl+C.
- `main` temporary bridge in Task 1 is removed by Task 3/4 in the same delivery.
- Commit steps are gated by the global “only if user requested commits” constraint.
