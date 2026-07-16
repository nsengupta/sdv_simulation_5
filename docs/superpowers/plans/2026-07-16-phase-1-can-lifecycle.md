# Phase 1 CAN Lifecycle Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Decode lifecycle CAN frames into the twin, establish precise ingress terminology, and guarantee side-effect-free ingress while the FSM is `Off`.

**Architecture:** Raw CAN frames become transport-independent `TwinIngressEvent` values, which `IngressToFsmProjector` converts to `TwinMessage::Fsm(FsmEvent)`. `VirtualCarActor` owns the authoritative silent-Off guard before any observable or zone work.

**Tech Stack:** Rust workspace, SocketCAN, Tokio MPSC, ractor, crate-local contract tests.

## Global Constraints

- Preserve all pre-existing uncommitted changes; work in the current checkout.
- Do not create commits unless the user explicitly requests them.
- Keep `VssSignal`; document its future KUKSA blueprint-path role.
- Remove `VehicleEvent`.
- Rename `PhysicalCarVocabulary` to `TwinIngressEvent`.
- Rename `TelemetryUpdate` to `Telemetry` and `VehicleSpeed` to `Speed`.
- Rename `PhysicalToDigitalProjector` to `IngressToFsmProjector`.
- Rename `DigitalTwinCarVocabulary` to `TwinMessage`.
- Keep programmatic `send_power_on/off()` but route both through canonical ingress.
- Require strict eight-byte lifecycle CAN payloads.
- Do not implement emulator transmission, observation files, process splitting, dashboard key removal, or Zenoh.

---

### Task 1: Canonical vocabulary and naming

**Files:**
- Modify: `crates/common/src/domain_types.rs`
- Modify: `crates/common/src/signals.rs`
- Modify: `crates/common/src/digital_twin/mod.rs`
- Modify: `crates/common/src/lib.rs`
- Modify: `crates/common/src/facade.rs`
- Modify: `crates/common/src/twin_runtime/connectors/mod.rs`
- Modify: `crates/common/src/twin_runtime/connectors/physical_to_digital.rs`
- Modify: all Rust call sites found by exact searches for the replaced names
- Test: `crates/common/src/test/projection_contract.rs`

**Interfaces:**
- Produces: `LifecycleCommand::{PowerOn, PowerOff}`
- Produces: `TwinIngressEvent::{Lifecycle, Telemetry, TimerTick, SystemReset, ...}`
- Produces: `IngressToFsmProjector`
- Produces: `TwinMessage`

- [ ] **Step 1: Write a compile-failing vocabulary contract**

Update `projection_contract.rs` to import and construct the desired public names:

```rust
use crate::{IngressToFsmProjector, LifecycleCommand, TwinIngressEvent, TwinMessage, VssSignal};

let input = TwinIngressEvent::Telemetry(VssSignal::Speed(50.0));
```

- [ ] **Step 2: Verify the contract fails because desired names do not exist**

Run: `cargo test -p common projection_contract`

Expected: compilation fails on unresolved desired names or variants.

- [ ] **Step 3: Introduce the canonical names and remove the redundant wrapper**

Define:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleCommand {
    PowerOn,
    PowerOff,
}

#[derive(Debug, Clone)]
pub enum TwinIngressEvent {
    Lifecycle(LifecycleCommand),
    Telemetry(VssSignal),
    TimerTick,
    SystemReset,
    FrontHeadlampCommandConfirmed { on_command: bool },
    FrontHeadlampCommandRejected { on_command: bool },
}
```

Rename the projector and actor mailbox protocol, update exports and call sites, and delete `VehicleEvent`. Document `VssSignal` and `TwinIngressEvent` according to the design spec.

- [ ] **Step 4: Verify the naming refactor is green**

Run: `cargo test -p common projection_contract`

Expected: projection contracts compile and pass.

---

### Task 2: Strict lifecycle CAN codec

**Files:**
- Modify: `crates/common/src/signals.rs`
- Create: `crates/common/src/test/lifecycle_signal_contract.rs`
- Modify: `crates/common/src/test/mod.rs`

**Interfaces:**
- Produces: `pub const ID_LIFECYCLE: u16 = 0x100`
- Produces: `LifecycleCommand::from_can_frame(&CanFrame) -> Option<Self>`
- Produces: `LifecycleCommand::to_can_frame(&self) -> Result<CanFrame, socketcan::Error>`

- [ ] **Step 1: Add failing lifecycle codec contracts**

Cover:

```rust
assert_eq!(
    LifecycleCommand::from_can_frame(
        &LifecycleCommand::PowerOn.to_can_frame().expect("encode")
    ),
    Some(LifecycleCommand::PowerOn)
);
```

Add equivalent PowerOff coverage and rejection tests for wrong ID, extended ID, non-eight DLC, opcode other than zero/one, and nonzero reserved bytes.

- [ ] **Step 2: Verify codec tests fail for missing API**

Run: `cargo test -p common lifecycle_signal_contract`

Expected: compilation fails because lifecycle codec methods are absent.

- [ ] **Step 3: Implement the minimal strict codec**

Decode only standard ID `0x100` with eight bytes, accepted opcode, and zero reserved bytes. Encode exactly the documented eight-byte arrays.

- [ ] **Step 4: Verify codec tests pass**

Run: `cargo test -p common lifecycle_signal_contract`

Expected: all lifecycle codec tests pass.

---

### Task 3: Gateway CAN mapping and FSM projection

**Files:**
- Modify: `crates/gateway/src/ingress/mapping.rs`
- Modify: `crates/gateway/src/gateway_runtime.rs`
- Modify: `crates/common/src/twin_runtime/connectors/physical_to_digital.rs`
- Modify: `crates/common/src/test/projection_contract.rs`

**Interfaces:**
- Produces: `can_frame_to_twin_ingress(&CanFrame) -> Option<TwinIngressEvent>`
- Consumes: strict `LifecycleCommand` and existing `VssSignal` codecs

- [ ] **Step 1: Add failing gateway mapping and lifecycle projection tests**

Assert:

```rust
assert!(matches!(
    can_frame_to_twin_ingress(&power_on_frame),
    Some(TwinIngressEvent::Lifecycle(LifecycleCommand::PowerOn))
));
```

Add PowerOff and telemetry preservation tests. In `projection_contract.rs`, assert both lifecycle commands become matching `FsmEvent` values inside `TwinMessage::Fsm`.

- [ ] **Step 2: Verify mapping/projection tests fail for missing behavior**

Run: `cargo test -p gateway ingress::mapping && cargo test -p common projection_contract`

Expected: lifecycle cases fail or do not compile.

- [ ] **Step 3: Implement gateway mapping and projection**

Make gateway ingress try lifecycle decoding and VSS decoding before device-specific actuator-response decoding. Preserve the current unsupported observed-speed drop. Map:

```rust
TwinIngressEvent::Lifecycle(LifecycleCommand::PowerOn) => FsmEvent::PowerOn,
TwinIngressEvent::Lifecycle(LifecycleCommand::PowerOff) => FsmEvent::PowerOff,
```

- [ ] **Step 4: Verify gateway and projection tests pass**

Run: `cargo test -p gateway ingress::mapping && cargo test -p common projection_contract`

Expected: all targeted tests pass.

---

### Task 4: Canonical programmatic lifecycle and silent-Off enforcement

**Files:**
- Modify: `crates/common/src/twin_runtime/controller/vehicle_controller.rs`
- Modify: `crates/common/src/twin_runtime/controller/virtual_car_actor.rs`
- Modify: `crates/common/src/test/actor_contract.rs`
- Modify: `crates/common/src/test/controller_api_contract.rs`

**Interfaces:**
- Consumes: `TwinIngressEvent::Lifecycle`
- Enforces: while current FSM state is `Off`, only `FsmEvent::PowerOn` reaches turn processing

- [ ] **Step 1: Add failing actor contracts**

Install a controller with transition, diagnostic, and actuation receivers. While still `Off`, submit RPM and lux telemetry and assert:

```rust
assert!(tokio::time::timeout(SHORT_TIMEOUT, transition_rx.recv()).await.is_err());
assert_eq!(snapshot.as_of_seq(), 0);
assert_eq!(*snapshot.current_state(), FsmState::Off);
assert_eq!(snapshot.context(), &VehicleContext::default());
```

Separately call `send_power_off()` while `Off` and assert no transition record and no rejection diagnostic. Then call `send_power_on()` and assert the normal startup record appears.

- [ ] **Step 2: Verify actor contracts fail on current Off-to-Off records**

Run the new tests individually with `cargo test -p common <exact_test_name> -- --nocapture`.

Expected: transition traffic or sequence advancement is observed before the guard exists.

- [ ] **Step 3: Route programmatic lifecycle through ingress**

Implement `send_power_on/off()` by calling the canonical submission path with `TwinIngressEvent::Lifecycle(...)`.

- [ ] **Step 4: Add the single actor-boundary guard**

At the start of the `TwinMessage::Fsm(event)` arm, before timer diagnostics:

```rust
if matches!(runtime_state.twin_car.current_state(), FsmState::Off)
    && !matches!(event, FsmEvent::PowerOn)
{
    return Ok(());
}
```

- [ ] **Step 5: Verify silent-Off and normal startup tests pass**

Run: `cargo test -p common actor_contract && cargo test -p common controller_api_contract`

Expected: all targeted actor and controller contracts pass.

---

### Task 5: Documentation and complete verification

**Files:**
- Modify: `docs/PHASES.md`
- Modify: `docs/ARCHITECTURE-OVERVIEW.md`
- Modify: `docs/TODO-simulation-5.md`
- Modify: `DESIGN.md` only where terminology or the documented implementation gap is now stale

**Interfaces:**
- Documents: Phase 1 status, G1/G2 closure, canonical ingress vocabulary, Phase 2 emulator ownership

- [ ] **Step 1: Update completion documentation**

Mark Phase 1 done only after targeted tests pass. Record that the common/gateway codec exists while emulator `0x100` transmission remains Phase 2.

- [ ] **Step 2: Check formatting**

Run: `cargo fmt --all -- --check`

Expected: no formatting differences. If differences are reported, run `cargo fmt --all` and re-check.

- [ ] **Step 3: Run lints**

Run: `cargo clippy --workspace --all-targets`

Expected: no new warnings beyond the baseline gateway dead-code warnings. Their cleanup must not broaden Phase 1 scope.

- [ ] **Step 4: Run complete regression suite**

Run: `cargo test --workspace`

Expected: all workspace unit, contract, integration, and doc tests pass.

- [ ] **Step 5: Confirm no stale names remain**

Search Rust sources for:

```text
VehicleEvent
PhysicalCarVocabulary
PhysicalToDigitalProjector
DigitalTwinCarVocabulary
TelemetryUpdate
VehicleSpeed
```

Expected: no code occurrences except intentional historical documentation, if any.
