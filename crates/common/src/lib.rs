//! One-crate library pyramid — layer map in `docs/design-notes-pyramid-layers.md`.
//!
//! Sibling order is *dependee before dependent* (foundation first), not runtime data-flow order.
//!
//! - **L0** `vehicle_physics` — constants and pure kinematics
//! - **L1** `vehicle_state`, `domain_types`, `signals`, `front_headlamp_log`
//! - **L2** `fsm` — pure decision core (`step`, `transition_map`); imports L0/L1 only
//! - **L3** `digital_twin`, `observation_records` — twin capsule and outward-facing observation records
//! - **L4** `twin_runtime` — actor runtime (sinks live under `observation_records::{transition,diagnostic}::sink`)
//! - **L3 shims** `published`, `transition_sink`, `diagnostic` — compatibility re-exports (migrate to `observation_records`)
//! - **L5** `facade` — public surface for gateway / L6 binaries
//!
//! Acyclic among core layers: `fsm` does not import `digital_twin` or `twin_runtime`;
//! `digital_twin` imports `fsm` and `vehicle_state`; `twin_runtime` sits above `digital_twin`.
pub mod vehicle_physics;
pub mod vehicle_state;
pub mod domain_types;
pub mod signals;
pub mod front_headlamp_log;
pub mod fsm;
pub mod digital_twin;
pub mod observation_records;
pub mod published;
pub mod transition_sink;
pub mod diagnostic;
pub mod twin_runtime;
pub mod facade;

#[cfg(test)]
mod test;

pub use digital_twin::{
    verify_state_laws, CarSnapshot, DigitalTwinCar, DigitalTwinCarError, DigitalTwinCarVocabulary,
    LawViolation, NotFsmVocabulary, StateLaw, STATE_LAWS,
};
pub use domain_types::{PhysicalCarVocabulary, VehicleEvent, VehicleState};
pub use twin_runtime::connectors::{PhysicalToDigitalProjector, Projector, ProjectionError};
pub use twin_runtime::controller::{
    ActuationCommand, ActuationError, ActuationFeedback, ActuationManager, CorrelationId,
    DefaultActuationManager, VehicleController, VehicleControllerError,
    VehicleControllerRuntimeOptions,
};
pub use signals::VssSignal;
pub use front_headlamp_log::{
    ACK_OFF, ACK_ON, CMD_OFF, CMD_ON, MSG_ACK_OFF, MSG_ACK_ON, MSG_NACK_OFF, MSG_NACK_ON,
    MSG_REQUEST_OFF, MSG_REQUEST_ON, MSG_TIMEOUT_OFF, MSG_TIMEOUT_ON, NACK_OFF, NACK_ON,
    TIMEOUT_OFF, TIMEOUT_ON,
};
pub use vehicle_physics::{
    calculate_speed_from_rpm, extreme_operation_active, operational_warning_active,
    speed_threshold_exceeded, EXTREME_OPERATION_WARNING_MESSAGE, FRONT_HEADLAMP_OFF_ACK_WAIT,
    FRONT_HEADLAMP_ON_ACK_WAIT, LUX_OFF_THRESHOLD, LUX_ON_THRESHOLD, RPM_DRIVING_THRESHOLD,
    RPM_EXTREME_OPERATION_THRESHOLD, RPM_IDLE, RPM_REDLINE_THRESHOLD,
    RPM_STRESS_DURATION_THRESHOLD_SECS, SPEED_EXTREME_OPERATION_THRESHOLD_KPH,
    SPEED_THRESHOLD_WARNING_MESSAGE,
};
pub use observation_records::{
    DiagnosticLevel, DiagnosticRecord, PublishedDomainAction,
    PublishedFrontHeadlampIncompleteCause, PublishedFrontHeadlampSwitchDirection,
    PublishedFsmEvent, PublishedFsmState, PublishedHeadlampContext, PublishedHeadlampState,
    PublishedHealthContext, PublishedPowertrainContext, PublishedTransitionRecord,
    PublishedVehicleContext, PublishedVisibilityContext, PublishedWheelRpm, SessionEpoch,
};
pub use observation_records::diagnostic::sink::{
    DiagnosticSink, DiagnosticSinkError, TokioMpscDiagnosticSink, diag_actuation_failure,
    diag_front_headlamp_confirmed, diag_state_transition, diag_timer_tick,
    diag_transition_sink_closed, diag_transition_sink_full, diag_warning,
    spawn_stdout_diagnostic_observer,
};
pub use observation_records::transition::sink::{
    TokioMpscTransitionRecordSink, TransitionRecordSink, TransitionSinkError,
};
/// Deprecated alias — prefer [`DiagnosticRecord`].
pub type DiagnosticMessage = DiagnosticRecord;
