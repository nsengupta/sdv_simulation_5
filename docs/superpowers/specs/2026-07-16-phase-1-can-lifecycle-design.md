# Phase 1 CAN lifecycle and silent-Off design

## Goal

Make the CAN ingress path semantically correct before emulator echo, process separation, observation files, or Zenoh: CAN `0x100` controls lifecycle, and an installed twin in `Off` ignores every FSM input except `PowerOn`.

## Scope

- Decode CAN `0x100` into `PowerOn` and `PowerOff`.
- Introduce one transport-independent twin ingress vocabulary.
- Route CAN and transitional programmatic lifecycle calls through the same projection boundary.
- Silently drop all non-`PowerOn` FSM events while the primary FSM is `Off`.
- Keep dashboard `s`/`o` and gateway auto-PowerOn behavior.
- Add codec, projection, gateway mapping, and actor contract tests.

Emulator transmission, dashboard key removal, observation files, process splitting, and Zenoh remain out of scope.

## Vocabulary

`VssSignal` remains the semantic telemetry type. Its documentation must state that it represents interpreted VSS values and is intended to become the target of future KUKSA blueprint-path interpretation. It is not a lifecycle, control, or actuator-feedback vocabulary.

Its variants are:

- `Speed(f64)`
- `EngineRpm(u16)`
- `AmbientLux(u16)`
- `RainDetected(bool)`

`VehicleEvent` is removed because it is a gateway-only wrapper that duplicates the canonical twin input.

`TwinIngressEvent` replaces `PhysicalCarVocabulary`. It is the canonical, transport-independent external input accepted by the twin before FSM projection:

- `Lifecycle(LifecycleCommand)`
- `Telemetry(VssSignal)`
- `TimerTick`
- `SystemReset`
- front-headlamp command confirmation/rejection feedback

`LifecycleCommand` contains `PowerOn` and `PowerOff`.

`IngressToFsmProjector` replaces `PhysicalToDigitalProjector` and maps `TwinIngressEvent` to `TwinMessage::Fsm(FsmEvent)`.

`FsmEvent` remains the state-machine vocabulary. `TwinMessage` replaces `DigitalTwinCarVocabulary` as the actor mailbox protocol; it contains FSM events plus actor-only requests, zone replies, and timeout messages.

## Data flow

```text
CAN frame
  -> gateway CAN decoder
  -> TwinIngressEvent::Lifecycle(...) or TwinIngressEvent::Telemetry(...)
  -> IngressToFsmProjector
  -> TwinMessage::Fsm(FsmEvent)
  -> VirtualCarActor
  -> FSM
```

Actuator feedback enters the same `TwinIngressEvent` boundary after its device-specific CAN policy validates correlation.

`VehicleController::send_power_on()` and `send_power_off()` remain public transitional APIs but construct `TwinIngressEvent::Lifecycle(...)` and use the same projector. They no longer bypass the ingress boundary by constructing `FsmEvent` directly.

## CAN `0x100` codec

Lifecycle frames use standard 11-bit ID `0x100` and exactly eight bytes:

- `01 00 00 00 00 00 00 00` -> `LifecycleCommand::PowerOn`
- `00 00 00 00 00 00 00 00` -> `LifecycleCommand::PowerOff`

Decoding is strict. Ignore frames with an extended or different ID, DLC other than eight, byte zero outside `{0, 1}`, or any nonzero reserved byte. Encoding always emits exactly eight bytes.

## Silent ignore while `Off`

The single enforcement point is the `TwinMessage::Fsm` arm in `VirtualCarActor`. Before timer diagnostics, turn allocation, barrier creation, zone routing, FSM stepping, context persistence, or ledger publication:

- if current state is `Off` and event is `PowerOn`, process normally;
- if current state is `Off` and event is anything else, return successfully without side effects.

Actor protocol messages such as `GetStatus` remain available. Internal zone protocol messages are not external FSM ingress and are not gateway-gated.

This placement covers CAN, programmatic controller calls, tests, and future carriers without duplicating authoritative state in the gateway.

## Tests and acceptance

- Lifecycle codec tests cover exact encode/decode, round trips, and every strict rejection rule.
- Gateway mapping tests prove CAN `0x100` becomes canonical lifecycle ingress.
- Projection tests prove lifecycle ingress becomes matching `FsmEvent`.
- Actor tests prove RPM and lux while `Off` produce no transition traffic, context mutation, sequence increment, diagnostic, actuation, or zone-driven behavior.
- Actor tests prove `PowerOff` while `Off` is silently dropped with no `RejectedPowerOff`.
- The same actor proceeds through normal startup after a subsequent `PowerOn`.
- `cargo test --workspace` passes.
- Manual `vcan0` smoke confirms pre-Start frames do not advance `Seq` and injected CAN `0x100` PowerOn starts a normal session.

## Documentation completion

After tests pass, mark Phase 1 done in `docs/PHASES.md`, close architecture gaps G1/G2, and clarify that emulator transmission belongs to Phase 2.
