# Phase 2 finite CAN emulator and startup ordering verification

## Goal

Run the current combined Dashboard/Twin application as a passive observer while a standalone,
finite emulator drives lifecycle and telemetry over CAN:

```text
PowerOn -> N telemetry cycles -> EngineRpm(0) -> PowerOff
```

The twin remains authoritative. The emulator sends physical inputs; it does not duplicate FSM
state, decide whether PowerOff is valid, or exist primarily as a test harness.

## Scope

- Add a required positive `--readings N` argument to the emulator.
- Send one strict lifecycle PowerOn frame before generated telemetry.
- Send exactly `N` telemetry cycles; each cycle contains RPM, ambient lux, and rain frames.
- Preserve the existing `EMULATOR_TUNNEL_PROB` and `EMULATOR_RAIN_PROB` startup overrides.
- Send `EngineRpm(0)` after the final telemetry cycle, then send strict lifecycle PowerOff.
- Exit successfully after the final frame is written.
- Add twin contract tests proving that the existing turn barrier holds post-PowerOn ingress behind
  assembly-startup turns and commits it FIFO after startup reaches `Idle`.
- Remove Dashboard `s`/`o` lifecycle injection and direct lifecycle-coordinator usage.
- Keep Dashboard and Twin in one process for now.
- Make the PowerOff rejection diagnostic explicitly say that the vehicle must be `Idle`.
- Add unit, actor-contract, integration, and manual `vcan0` verification.

## Out of scope

- CSV echo, parsing, generation, replay, or scenario files.
- Offline scenario authoring.
- Uniform deceleration; Phase 2 uses one final `EngineRpm(0)` input.
- Ctrl-C lifecycle handling.
- Dashboard embedding of emulator logic.
- Gateway/Dashboard process separation.
- Observation files.
- Transport abstraction, Zenoh, or uProtocol.
- Changing the current bounded-random physical model merely to force a successful PowerOff.

## Runtime topology

The processes run in this order:

```text
1. front_headlamp_actuator
2. wiper_actuator
3. tui_dashboard
   - installs the twin in Off
   - starts CAN ingress and actuation egress
   - renders twin diagnostic and ledger emissions only
4. emulator --readings N
```

Actuators remain independent shell commands, as they are today. Missing or failed actuator
responses remain real inputs to twin policy and are reported through the twin's emissions.

## Emulator command and sequence

Example:

```bash
cargo run -p emulator -- --readings 30
```

The existing probability controls remain available:

```bash
EMULATOR_TUNNEL_PROB=0.05 \
EMULATOR_RAIN_PROB=0.02 \
cargo run -p emulator -- --readings 30
```

`EMULATOR_TUNNEL_PROB` controls the per-tick probability of entering a low-lux tunnel period;
the existing lux jitter and tunnel duration continue to produce visibility decreases and
subsequent increases. `EMULATOR_RAIN_PROB` controls the per-tick probability of rain starting
while dry. Phase 2 does not change their ranges, defaults, or invalid-value fallback behavior.

`--readings` is required, accepts a positive integer, and counts logical telemetry cycles rather
than individual CAN frames. Each cycle writes frames in the existing stable serialization order:

1. `EngineRpm` on `0x102`
2. `AmbientLux` on `0x103`
3. `RainDetected` on `0x104`

This order is deterministic wire serialization only. It does not assert that visibility or rain
is meaningful only while driving. Low visibility and rain while `Idle` are valid and must be
handled according to the existing zone and FSM rules.

The complete frame sequence is:

```text
LifecycleCommand::PowerOn
N * (EngineRpm, AmbientLux, RainDetected)
EngineRpm(0)
LifecycleCommand::PowerOff
```

There is no post-PowerOn delay. The first telemetry cycle may arrive while startup is in progress;
readiness is a twin responsibility, not an emulator responsibility. The existing 100 ms interval
remains between generated telemetry cycles. The final RPM-zero and PowerOff frames follow the last
cycle without adding another logical reading.

The total frame count is `3N + 3`. For `--readings 30`, the emulator writes 93 frames.

The final stop input must be `EngineRpm(0)`, not `VssSignal::Speed(0)`: Gateway currently ignores
CAN `0x101`, and the twin derives speed from RPM.

The emulator guarantees transmission order, not FSM acceptance. If generated inputs leave the
twin in a state whose rules reject PowerOff, the twin remains in that state, emits the required
diagnostic and ledger information, and the emulator still exits after transmitting PowerOff.

## Testability boundary

The emulator package gains a small library seam around finite-run sequencing:

- a frame sink that accepts encoded classic CAN frames;
- a finite-run function parameterized by the sink and physical model/random source;
- a real SocketCAN sink used by the binary;
- an in-memory sink used by emulator tests.

This seam exists to test emulator behavior without requiring `vcan0`. It is not a general
transport abstraction and does not anticipate Zenoh.

The binary remains the composition root: parse `--readings`, open `vcan0`, construct the current
physical model, execute the finite run, and report errors.

## Existing twin startup ordering

CAN producers cannot know when the twin has completed assembly startup. The emulator therefore
sends PowerOn and subsequent readings without a readiness delay.

The twin is already equipped to preserve the required ordering:

1. PowerOn commits `Off -> PreparingToStart` and appends the assembly-startup barriers.
2. Subsequent external ingress creates normal turns at the back of the same barrier queue.
3. The head-of-buffer invariant prevents those later turns from committing while an earlier
   assembly-startup barrier is incomplete.
4. Assembly readiness commits startup to `Idle`.
5. Already-enqueued external turns then commit FIFO against the resulting `Idle` state.

Phase 2 does not add another startup queue or change this behavior. It adds actor-level contracts
that send telemetry immediately after PowerOn and prove:

- startup reaches `Idle` before the telemetry turn commits;
- telemetry arrival order is preserved;
- telemetry context and ledger effects do not appear before its turn commits;
- a final RPM-zero turn precedes PowerOff.

Existing silent-Off behavior is unchanged: while `Off`, every FSM event except PowerOn is silently
ignored.

## Dashboard responsibility

Dashboard remains the transitional owner of the in-process twin, but becomes lifecycle-passive:

- remove `s`/`o` key handling;
- remove direct `send_power_on()` and `send_power_off()` calls;
- remove Dashboard lifecycle-coordinator state used only by those keys;
- retain `q`/Escape process exit;
- replace “press Start” text with waiting-for-CAN lifecycle text;
- render only diagnostic and ledger emissions from the twin, plus static install metadata.

Dashboard does not infer or reproduce FSM rules. In the successful controlled path it observes:

```text
Off -> PreparingToStart -> Idle -> Driving -> Idle -> Off
```

## PowerOff policy and diagnostics

PowerOff remains legal only from `Idle`. The emulator's final RPM-zero input normally gives the
twin an opportunity to reach `Idle`, but it does not override other FSM guards.

Rejected PowerOff must:

- leave the current FSM state unchanged;
- emit a warning diagnostic clearly stating that the vehicle must be `Idle` before PowerOff;
- remain visible in the transition ledger as the rejected input and warning action.

Use this diagnostic wording, substituting the actual current state:

```text
[REJECTED]: vehicle must be Idle before PowerOff; current state is Driving
```

## Expected file-level changes

### Emulator

- `crates/emulator/src/lib.rs` — finite-run API and module exports.
- `crates/emulator/src/cli.rs` — strict parsing and validation of `--readings N`.
- `crates/emulator/src/main.rs` — thin CLI, SocketCAN setup, and finite-run invocation.
- `crates/emulator/src/runner.rs` — lifecycle and finite telemetry sequencing.
- `crates/emulator/src/car_physics.rs` — expose one telemetry-cycle result without moving twin
  policy into the emulator.
- `crates/emulator/src/sink.rs` — narrow real/mock CAN frame sink.
- `crates/emulator/tests/finite_run_contract.rs` — frame count, ordering, boundaries, and sink-error
  integration contracts.

### Twin/common

- `crates/common/src/fsm/step.rs` — explicit Idle-required PowerOff diagnostic.
- `crates/common/src/test/startup_barrier_contract.rs` — add one focused characterization test for
  the existing startup-barrier and FIFO-commit behavior.
- `crates/common/src/test/fsm_step_contract.rs` — assert the exact PowerOff rejection action;
  existing actor tests already prove warning actions reach the diagnostic sink.

### Dashboard

- `crates/tui_dashboard/src/main.rs` — remove lifecycle controls/coordinator and update passive
  observer text and tests.

### Documentation

- `docs/PHASES.md` — replace the former CSV echo Phase 2 scope and acceptance criteria.
- `docs/ARCHITECTURE-OVERVIEW.md` — update the near-term emulator and Dashboard arrangement without
  changing later process-split goals.
- `README.md` — document finite emulator invocation and process order.

## Mandatory tests

### Emulator unit and integration tests

- Missing, zero, non-numeric, and overflowing `--readings` values fail clearly.
- Valid tunnel and rain probability overrides still configure their respective models.
- Missing or invalid probability overrides retain the existing defaults and warning behavior.
- For `N`, the sink receives exactly `3N + 3` frames.
- The first frame is strict CAN `0x100` PowerOn.
- Every counted cycle contains `0x102`, `0x103`, and `0x104` in stable serialization order.
- The penultimate frame is `EngineRpm(0)` on `0x102`.
- The final frame is strict CAN `0x100` PowerOff.
- Sink failures stop the run and are returned to the caller.
- Existing model bounds remain covered.

### Twin actor contracts

- PowerOn followed immediately by telemetry places telemetry behind assembly-startup barriers.
- Telemetry causes no early context or ledger effects.
- Startup reaches `Idle` before queued telemetry commits FIFO through the existing turn barrier.
- An `EngineRpm(0)` turn followed by PowerOff can produce `Idle -> Off` in the controlled path.
- Silent ignore while `Off` remains unchanged.
- PowerOff while `Driving` is rejected with an explicit must-be-Idle diagnostic and unchanged
  state.

### Dashboard tests

- Dashboard has no lifecycle key path.
- Pre-PowerOn rendering says it is waiting for lifecycle over CAN.
- Diagnostic and ledger rendering remains driven only by received twin records.

### Controlled integration and manual verification

- An automated controlled-input test proves
  `Off -> PreparingToStart -> Idle -> Driving -> Idle -> Off` without depending on emulator
  randomness.
- A controlled rejection test proves PowerOff while driving remains rejected.
- A manual `vcan0` run starts actuators, Dashboard, then `emulator --readings N`; Dashboard displays
  the twin's actual outcome.
- Emulator runs remain useful for exploration and may reveal scenarios that deserve new focused
  unit or end-to-end tests.
- `cargo test --workspace` passes.

## Acceptance criteria

- Phase 1 remains green, including the `vcan0` lifecycle smoke.
- `cargo run -p emulator -- --readings 30` writes 93 frames in the specified order and exits.
- Existing twin turn-barrier tests prove immediate post-PowerOn telemetry commits only after
  assembly startup reaches `Idle`; the emulator adds no readiness delay.
- Dashboard cannot inject lifecycle and displays only twin-authored diagnostic and ledger state.
- Controlled tests prove both successful shutdown and rejected PowerOff behavior.
- All mandatory tests pass.
- CSV-related work remains explicitly deferred for future reconsideration.
