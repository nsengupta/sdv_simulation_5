# Phase 2 Finite CAN Emulator Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the standalone emulator run a finite CAN session (`PowerOn -> N telemetry cycles -> EngineRpm(0) -> PowerOff`), prove the twin's existing startup turn barrier orders immediate ingress correctly, and make Dashboard lifecycle-passive.

**Architecture:** The emulator package gains a small library boundary containing strict argument parsing, finite sequencing, and a narrow CAN-frame sink. The existing physical models and environment-variable probability controls remain authoritative. The twin runtime is not changed for startup ordering; one characterization contract proves its existing head-of-buffer behavior. Dashboard continues to own the in-process twin but only renders twin emissions.

**Tech Stack:** Rust 2024, Tokio, ractor, socketcan 3.5, existing `common` codecs, Ratatui/Crossterm.

## Global Constraints

- No CSV echo, parser, generator, replay, or scenario files.
- Preserve `EMULATOR_TUNNEL_PROB` and `EMULATOR_RAIN_PROB` as environment variables with their current defaults and validation.
- `--readings N` is required and `N > 0`.
- One reading means one ordered RPM/lux/rain telemetry cycle.
- No emulator delay after PowerOn; the twin owns startup ordering.
- The stop signal is `EngineRpm(0)` on CAN `0x102`, never speed `0` on `0x101`.
- The emulator transmits PowerOff but does not decide whether the FSM accepts it.
- Dashboard must not inject lifecycle.
- Do not add a second startup queue or modify `VirtualCarActor` startup ordering.
- Add only tests that cover new emulator behavior, changed diagnostic text/UI, or the previously unasserted startup-ordering contract.
- Do not commit unless the user explicitly authorizes commits.

---

## File Structure

- Create `crates/emulator/src/lib.rs`: public finite-emulator modules.
- Create `crates/emulator/src/cli.rs`: `--readings` and probability parsing.
- Create `crates/emulator/src/runner.rs`: exact lifecycle/telemetry sequence.
- Create `crates/emulator/src/sink.rs`: narrow frame sink and SocketCAN implementation.
- Create `crates/emulator/tests/finite_run_contract.rs`: frame-count/order/error contracts.
- Modify `crates/emulator/src/main.rs`: thin composition root.
- Modify `crates/emulator/src/car_physics.rs`: return one telemetry cycle after updating.
- Modify `crates/common/src/test/startup_barrier_contract.rs`: one characterization test for immediate post-PowerOn ingress.
- Modify `crates/common/src/test/fsm_step_contract.rs`: exact PowerOff rejection warning.
- Modify `crates/common/src/fsm/step.rs`: clearer rejection text.
- Modify `crates/tui_dashboard/src/main.rs`: remove lifecycle controls and update observer copy.
- Modify `docs/PHASES.md`, `docs/ARCHITECTURE-OVERVIEW.md`, and `README.md`: replace CSV-era Phase 2 instructions.

---

### Task 1: Characterize Existing Twin Startup Ordering

**Files:**
- Modify: `crates/common/src/test/startup_barrier_contract.rs`

**Interfaces:**
- Consumes: existing `VehicleController`, `TurnBarrier`, injected `ZoneReady`, transition channel.
- Produces: a regression contract; no production API or runtime change.

- [ ] **Step 1: Add one focused characterization test**

Extend the existing test module imports with `PublishedFsmEvent`, `PublishedFsmState`,
`TwinIngressEvent`, `VssSignal`, and `tokio::sync::mpsc`. Add a single test that covers both FIFO
startup ordering and the controlled full lifecycle:

```rust
#[tokio::test]
async fn given_ingress_immediately_after_power_on_when_startup_unblocks_then_commits_fifo_from_idle() {
    let (transition_tx, mut transition_rx) = mpsc::channel(32);
    let opts = VehicleControllerRuntimeOptions {
        transition_tx: Some(transition_tx),
        test_silent_headlamp: true,
        ..Default::default()
    };
    let (controller, handle) = VehicleController::install_and_start_with_options(
        "STARTUP-FIFO".to_string(),
        opts,
    )
    .await
    .expect("spawn controller");
    let _guard = ActorGuard {
        addr: controller.get_actor_ref().clone(),
        handle,
    };

    controller.send_power_on().await.expect("power on");
    tokio::task::yield_now().await;

    controller
        .submit_twin_ingress(TwinIngressEvent::Telemetry(VssSignal::AmbientLux(900)))
        .await
        .expect("lux during startup");
    controller
        .submit_twin_ingress(TwinIngressEvent::Telemetry(VssSignal::EngineRpm(1200)))
        .await
        .expect("drive during startup");
    controller
        .submit_twin_ingress(TwinIngressEvent::Telemetry(VssSignal::EngineRpm(0)))
        .await
        .expect("standstill during startup");
    controller.send_power_off().await.expect("power off during startup");

    tokio::time::sleep(Duration::from_millis(25)).await;
    let blocked = controller
        .get_snapshot(Some(ractor::concurrency::Duration::from_millis(50)))
        .await
        .expect("snapshot while startup blocked");
    assert!(matches!(
        blocked.current_state(),
        FsmState::PreparingToStart { .. }
    ));
    assert_ne!(blocked.context().visibility.ambient_lux, 900);
    assert_ne!(blocked.context().powertrain.primary_rpm(), 1200);

    let power_on = transition_rx.recv().await.expect("PowerOn row");
    assert_eq!(power_on.event, PublishedFsmEvent::PowerOn);
    assert!(transition_rx.try_recv().is_err(), "later turns must remain blocked");

    inject_zone_ready(
        &controller,
        STARTUP_BARRIER_TURN,
        HeadlampState::Ready,
    );
    let deadline = std::time::Instant::now() + Duration::from_millis(500);
    loop {
        let snapshot = controller
            .get_snapshot(Some(ractor::concurrency::Duration::from_millis(50)))
            .await
            .expect("snapshot while waiting for PowerOff turn");
        if matches!(snapshot.current_state(), FsmState::PreparingToStop(_)) {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for PreparingToStop; got {:?}",
            snapshot.current_state()
        );
        tokio::task::yield_now().await;
    }

    let mut rows = Vec::new();
    for _ in 0..6 {
        rows.push(transition_rx.recv().await.expect("ordered startup/user row"));
    }
    assert_eq!(rows[2].event, PublishedFsmEvent::UpdateAmbientLux(900));
    assert_eq!(rows[2].old_state, PublishedFsmState::Idle);
    assert_eq!(rows[3].event, PublishedFsmEvent::UpdateRpm(1200));
    assert_eq!(rows[3].next_state, PublishedFsmState::Driving);
    assert_eq!(rows[4].event, PublishedFsmEvent::UpdateRpm(0));
    assert_eq!(rows[4].next_state, PublishedFsmState::Idle);
    assert_eq!(rows[5].event, PublishedFsmEvent::PowerOff);
    assert!(matches!(
        rows[5].next_state,
        PublishedFsmState::PreparingToStop
    ));

    const QUEUED_SHUTDOWN_HEADLAMP_TURN: u64 = 8;
    inject_zone_ready(
        &controller,
        QUEUED_SHUTDOWN_HEADLAMP_TURN,
        HeadlampState::Off,
    );
    wait_fsm_state(&controller, FsmState::Off, Duration::from_millis(500)).await;
    let _headlamp_off = transition_rx.recv().await.expect("headlamp shutdown row");
    let final_off = transition_rx.recv().await.expect("wiper shutdown row");
    assert_eq!(final_off.next_state, PublishedFsmState::Off);
}
```

Do not add helper production APIs for this test.

- [ ] **Step 2: Run the characterization test**

Run:

```bash
cargo test -p common given_ingress_immediately_after_power_on_when_startup_unblocks_then_commits_fifo_from_idle
```

Expected: PASS against the current twin. If it fails, stop and inspect the observed ledger order;
do not change `VirtualCarActor` until the discrepancy is reviewed with the user.

- [ ] **Step 3: Remove redundant assertions**

Keep only assertions proving:

- context does not reflect queued telemetry while startup is blocked;
- no telemetry ledger row overtakes startup;
- post-start telemetry commits FIFO from `Idle`;
- RPM zero precedes PowerOff.

Do not duplicate existing tests for assembly retry, timeout, ordinary startup, silent-Off, or
generic transition-channel publication.

---

### Task 2: Implement the Finite Emulator with Tests First

**Files:**
- Create: `crates/emulator/src/lib.rs`
- Create: `crates/emulator/src/cli.rs`
- Create: `crates/emulator/src/runner.rs`
- Create: `crates/emulator/src/sink.rs`
- Create: `crates/emulator/tests/finite_run_contract.rs`
- Modify: `crates/emulator/src/car_physics.rs`
- Modify: `crates/emulator/src/main.rs`

**Interfaces:**
- Produces: `EmulatorArgs { readings: NonZeroUsize }`.
- Produces: `parse_args`, `parse_probability_override`.
- Produces: `FrameSink::write_frame(CanFrame) -> anyhow::Result<()>`.
- Produces: `run_finite(&mut sink, &mut car, readings, sleep) -> anyhow::Result<()>`.
- Consumes: existing `LifecycleCommand` and `VssSignal` codecs.

- [ ] **Step 1: Write failing CLI unit tests**

Create `cli.rs` with tests specifying the API before implementation:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmulatorArgs {
    pub readings: std::num::NonZeroUsize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readings_argument_is_required_and_positive() {
        assert_eq!(
            parse_args(["--readings", "30"]).unwrap().readings.get(),
            30
        );
        assert!(parse_args(std::iter::empty::<&str>()).is_err());
        assert!(parse_args(["--readings", "0"]).is_err());
        assert!(parse_args(["--readings", "abc"]).is_err());
        assert!(parse_args(["--readings", "30", "extra"]).is_err());
    }

    #[test]
    fn probability_override_accepts_only_closed_unit_interval() {
        assert_eq!(parse_probability_override(None).unwrap(), None);
        assert_eq!(parse_probability_override(Some("0")).unwrap(), Some(0.0));
        assert_eq!(parse_probability_override(Some("1")).unwrap(), Some(1.0));
        assert!(parse_probability_override(Some("-0.1")).is_err());
        assert!(parse_probability_override(Some("1.1")).is_err());
        assert!(parse_probability_override(Some("not-a-number")).is_err());
    }
}
```

- [ ] **Step 2: Write the failing finite-run integration tests**

Create `finite_run_contract.rs` with an in-memory sink and no-op sleeper:

```rust
use anyhow::{anyhow, Result};
use common::{LifecycleCommand, VssSignal};
use emulator::car_physics::PhysicalCar;
use emulator::runner::{run_finite, TICK};
use emulator::sink::FrameSink;
use socketcan::{CanFrame, EmbeddedFrame};
use std::num::NonZeroUsize;

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

#[test]
fn finite_run_writes_exact_order_and_count() {
    let mut sink = RecordingSink::default();
    let mut car = PhysicalCar::new();
    let mut sleeps = Vec::new();

    run_finite(
        &mut sink,
        &mut car,
        NonZeroUsize::new(2).unwrap(),
        |duration| sleeps.push(duration),
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
    assert_eq!(sleeps, vec![TICK]);
}

#[test]
fn sink_error_stops_the_run() {
    let mut sink = RecordingSink {
        frames: Vec::new(),
        fail_after: Some(1),
    };
    let mut car = PhysicalCar::new();

    let error = run_finite(
        &mut sink,
        &mut car,
        NonZeroUsize::new(1).unwrap(),
        |_| {},
    )
    .unwrap_err();

    assert!(error.to_string().contains("injected sink failure"));
    assert_eq!(sink.frames.len(), 1);
}
```

- [ ] **Step 3: Run tests to verify RED**

Run:

```bash
cargo test -p emulator
```

Expected: FAIL because `lib.rs`, `cli`, `runner`, `sink`, and their APIs do not yet exist.

- [ ] **Step 4: Implement strict parsing**

Implement in `cli.rs`:

```rust
use anyhow::{bail, Context, Result};
use std::num::NonZeroUsize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmulatorArgs {
    pub readings: NonZeroUsize,
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
    if values.len() != 2 || values[0] != "--readings" {
        bail!("usage: emulator --readings <positive integer>");
    }
    let parsed = values[1]
        .parse::<usize>()
        .with_context(|| format!("invalid --readings value {:?}", values[1]))?;
    let readings = NonZeroUsize::new(parsed)
        .context("--readings must be greater than zero")?;
    Ok(EmulatorArgs { readings })
}

pub fn parse_probability_override(raw: Option<&str>) -> Result<Option<f32>> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    let parsed = raw
        .trim()
        .parse::<f32>()
        .with_context(|| format!("expected a float in 0.0..=1.0, got {raw:?}"))?;
    if !(0.0..=1.0).contains(&parsed) {
        bail!("expected a float in 0.0..=1.0, got {raw:?}");
    }
    Ok(Some(parsed))
}
```

- [ ] **Step 5: Expose one telemetry cycle**

Add to `car_physics.rs` without changing model probabilities:

```rust
pub fn update_and_read(&mut self) -> [common::VssSignal; 3] {
    self.update();
    [
        common::VssSignal::EngineRpm(self.rpm()),
        common::VssSignal::AmbientLux(self.ambient_lux()),
        common::VssSignal::RainDetected(self.rain_detected()),
    ]
}
```

- [ ] **Step 6: Implement the sink and runner**

Create `sink.rs`:

```rust
use anyhow::Result;
use socketcan::{CanFrame, CanSocket, Socket};

pub trait FrameSink {
    fn write_frame(&mut self, frame: CanFrame) -> Result<()>;
}

pub struct SocketCanSink {
    socket: CanSocket,
}

impl SocketCanSink {
    pub fn open(interface: &str) -> Result<Self> {
        Ok(Self {
            socket: CanSocket::open(interface)?,
        })
    }
}

impl FrameSink for SocketCanSink {
    fn write_frame(&mut self, frame: CanFrame) -> Result<()> {
        self.socket.write_frame(&frame)?;
        Ok(())
    }
}
```

Create `runner.rs`:

```rust
use crate::car_physics::PhysicalCar;
use crate::sink::FrameSink;
use anyhow::Result;
use common::{LifecycleCommand, VssSignal};
use std::num::NonZeroUsize;
use std::time::Duration;

pub const TICK: Duration = Duration::from_millis(100);

pub fn run_finite<S, Sleep>(
    sink: &mut S,
    car: &mut PhysicalCar,
    readings: NonZeroUsize,
    mut sleep: Sleep,
) -> Result<()>
where
    S: FrameSink,
    Sleep: FnMut(Duration),
{
    sink.write_frame(LifecycleCommand::PowerOn.to_can_frame()?)?;

    for index in 0..readings.get() {
        for signal in car.update_and_read() {
            sink.write_frame(signal.to_can_frame()?)?;
        }
        if index + 1 < readings.get() {
            sleep(TICK);
        }
    }

    sink.write_frame(VssSignal::EngineRpm(0).to_can_frame()?)?;
    sink.write_frame(LifecycleCommand::PowerOff.to_can_frame()?)?;
    Ok(())
}
```

Create `lib.rs`:

```rust
pub mod car_physics;
pub mod cli;
pub mod models;
pub mod runner;
pub mod sink;
```

- [ ] **Step 7: Make `main.rs` a thin composition root**

Retain the current environment variable names and warning text. Replace the infinite loop with:

```rust
use anyhow::Result;
use emulator::car_physics::PhysicalCar;
use emulator::cli::{parse_args, parse_probability_override};
use emulator::models::PhysicalWorldModelConfig;
use emulator::runner::run_finite;
use emulator::sink::SocketCanSink;
use std::{env, thread};

const ENV_TUNNEL_PROB: &str = "EMULATOR_TUNNEL_PROB";
const ENV_RAIN_PROB: &str = "EMULATOR_RAIN_PROB";

fn main() -> Result<()> {
    let args = parse_args(env::args().skip(1))?;
    let mut cfg = PhysicalWorldModelConfig::daytime_tunnel_profile();

    apply_probability_override(
        ENV_TUNNEL_PROB,
        &mut cfg.ambient_road_light.tunnel_event_probability_per_tick,
    );
    apply_probability_override(
        ENV_RAIN_PROB,
        &mut cfg.rain.rain_event_probability_per_tick,
    );

    let mut sink = SocketCanSink::open("vcan0")?;
    let mut car = PhysicalCar::new_with_config(cfg);
    run_finite(&mut sink, &mut car, args.readings, thread::sleep)
}

fn apply_probability_override(name: &str, target: &mut f32) {
    let Ok(raw) = env::var(name) else {
        return;
    };
    match parse_probability_override(Some(&raw)) {
        Ok(Some(value)) => {
            *target = value;
            println!("[emulator] {name}={value} — probability per 100 ms tick");
        }
        Ok(None) => {}
        Err(_) => {
            eprintln!(
                "[emulator] ignoring {name}={raw:?} — expected a float in 0.0..=1.0"
            );
        }
    }
}
```

- [ ] **Step 8: Run emulator tests GREEN**

Run:

```bash
cargo fmt --all
cargo test -p emulator
```

Expected: all emulator unit and integration tests PASS.

---

### Task 3: Clarify PowerOff Rejection Without Duplicating Sink Tests

**Files:**
- Modify: `crates/common/src/test/fsm_step_contract.rs`
- Modify: `crates/common/src/fsm/step.rs`

**Interfaces:**
- Consumes: existing `DomainAction::LogWarning`.
- Produces: exact warning text; existing `scenario_log_warning_is_routed_to_diagnostic_sink` continues to prove routing.

- [ ] **Step 1: Add one failing FSM-step assertion**

Add:

```rust
#[test]
fn power_off_while_driving_requires_idle_and_preserves_state() {
    let mut ctx = VehicleContext::default();
    ctx.powertrain.apply_rpm(1200);
    ctx.powertrain.refresh_speed();

    let result = twin_turn(
        &FsmState::Driving,
        &ctx,
        &FsmEvent::PowerOff,
        Instant::now(),
    );

    assert_eq!(result.next_state, FsmState::Driving);
    assert!(result.actions.contains(&DomainAction::LogWarning(
        "[REJECTED]: vehicle must be Idle before PowerOff; current state is Driving".to_string()
    )));
}
```

- [ ] **Step 2: Run RED**

Run:

```bash
cargo test -p common power_off_while_driving_requires_idle_and_preserves_state
```

Expected: FAIL because the current warning says only that PowerOff is invalid.

- [ ] **Step 3: Change only the warning text**

In `fsm/step.rs`, replace the rejection formatting with:

```rust
actions.push(DomainAction::LogWarning(format!(
    "[REJECTED]: vehicle must be Idle before PowerOff; current state is {:?}",
    current_state
)));
```

- [ ] **Step 4: Run focused and existing routing tests**

Run:

```bash
cargo test -p common power_off_while_driving_requires_idle_and_preserves_state
cargo test -p common scenario_log_warning_is_routed_to_diagnostic_sink
```

Expected: both PASS. Do not add a second actor test for generic diagnostic routing.

---

### Task 4: Make Dashboard Lifecycle-Passive

**Files:**
- Modify: `crates/tui_dashboard/src/main.rs`

**Interfaces:**
- Consumes: twin diagnostic and transition receivers.
- Produces: observer-only TUI; only `q`/Escape controls process exit.

- [ ] **Step 1: Change the two existing UI tests first**

Replace lifecycle-key expectations with:

```rust
#[test]
fn standby_panels_show_can_lifecycle_message() {
    let lines = standby_panel_lines();
    assert_eq!(lines.len(), 1);
    assert!(lines[0].spans[0].content.contains("PowerOn"));
    assert!(lines[0].spans[0].content.contains("CAN"));
}

#[test]
fn keys_footer_lists_quit_only() {
    assert_eq!(KEYS_FOOTER, "Keys: 'q' quit");
}
```

- [ ] **Step 2: Run RED**

Run:

```bash
cargo test -p tui_dashboard standby_panels_show_can_lifecycle_message
cargo test -p tui_dashboard keys_footer_lists_quit_only
```

Expected: FAIL against the current Start/Stop copy.

- [ ] **Step 3: Remove Dashboard lifecycle injection**

Make these exact structural changes:

- remove `VehicleController` and `TwinLifecycleCoordinator` imports;
- remove `controller` and `lifecycle` from `DigitalTwinRuntime`;
- pass the installed controller directly to `spawn_runtime`;
- remove `handle_start_key` and `handle_stop_key`;
- remove `s`, `S`, `o`, and `O` match arms;
- retain only `q` and Escape as active keys.

The setup becomes:

```rust
let (controller, _opts) = builder.install_controller().await?;
let runtime_handle = builder.spawn_runtime(controller)?;

Ok(DigitalTwinRuntime {
    diagnostic_rx,
    transition_rx,
    _runtime_handle: runtime_handle,
})
```

Use:

```rust
const KEYS_FOOTER: &str = "Keys: 'q' quit";
```

Use this standby copy:

```rust
"Twin installed; waiting for PowerOn on CAN — ledger and diagnostics appear after lifecycle starts."
```

- [ ] **Step 4: Run Dashboard tests GREEN**

Run:

```bash
cargo fmt --all
cargo test -p tui_dashboard
```

Expected: all Dashboard tests PASS.

---

### Task 5: Align Roadmap and Run Full Verification

**Files:**
- Modify: `docs/PHASES.md`
- Modify: `docs/ARCHITECTURE-OVERVIEW.md`
- Modify: `README.md`
- Verify: `docs/superpowers/specs/2026-07-16-phase-2-finite-can-emulator-design.md`

**Interfaces:**
- Produces: one consistent Phase 2 description and documented process order.

- [ ] **Step 1: Replace the Phase 2 roadmap section**

Document:

- status `In progress` until automated tests and manual `vcan0` acceptance pass;
- required `--readings N`;
- exact `3N + 3` frame formula;
- PowerOn first, RPM zero penultimate, PowerOff final;
- existing environment-variable probability controls;
- existing twin startup barrier verified by contract test;
- Dashboard lifecycle keys removed;
- CSV work deferred.

- [ ] **Step 2: Update overview and README**

Use this run order:

```bash
cargo run -p front_headlamp_actuator
cargo run -p wiper_actuator
cargo run -p tui_dashboard
EMULATOR_TUNNEL_PROB=0.01 \
EMULATOR_RAIN_PROB=0.008 \
cargo run -p emulator -- --readings 30
```

State that Dashboard observes the actual twin outcome; emulator PowerOff transmission does not
guarantee acceptance if FSM guards reject it.

- [ ] **Step 3: Scan for stale active Phase 2 instructions**

Run:

```bash
rg -n "echo|--csv|emulator-core|press Start|'s' start|'o' stop" \
  README.md docs/PHASES.md docs/ARCHITECTURE-OVERVIEW.md
```

Expected: no active instructions describing CSV as current Phase 2 or Dashboard lifecycle keys as
available. Historical/future references must be explicitly labelled deferred.

- [ ] **Step 4: Run focused and workspace verification**

Run:

```bash
cargo fmt --all --check
cargo test -p common startup_barrier_contract
cargo test -p common power_off_while_driving_requires_idle_and_preserves_state
cargo test -p emulator
cargo test -p tui_dashboard
cargo test --workspace
```

Expected: every command exits 0 with zero failed tests.

- [ ] **Step 5: Run manual `vcan0` acceptance**

With `vcan0` live, start actuators, Dashboard, then:

```bash
cargo run -p emulator -- --readings 30
```

Verify:

- emulator exits after the final PowerOff frame;
- Dashboard has no Start/Stop controls;
- Dashboard shows PowerOn and subsequent twin-authored ledger/diagnostic activity;
- immediate readings are ordered after startup by the twin;
- final RPM zero is observed before PowerOff;
- the displayed final state reflects the FSM's actual acceptance or rejection.

- [ ] **Step 6: Mark Phase 2 Done only after all acceptance evidence exists**

Change `docs/PHASES.md` from `In progress` to `Done` only after Step 4 and Step 5 pass. Do not
claim completion from emulator output alone.

---

## Plan Self-Review

- Spec coverage: finite emulator, probability overrides, startup ordering proof, observer-only
  Dashboard, explicit PowerOff warning, documentation, and `vcan0` acceptance are each assigned.
- Test economy: one startup characterization test reuses `startup_barrier_contract.rs`; one pure
  warning test reuses the existing actor diagnostic-routing contract; existing startup, shutdown,
  silent-Off, codec, and generic sink tests are not duplicated.
- Type consistency: `NonZeroUsize`, `FrameSink`, `run_finite`, and `SocketCanSink` have one
  definition and stable signatures across tasks.
- Placeholder scan: no TBD/TODO implementation steps remain.
