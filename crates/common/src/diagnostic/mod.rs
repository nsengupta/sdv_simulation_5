//! Compatibility shim — use [`crate::observation_records::diagnostic`] instead.
//!
//! Will be removed once all call sites migrate to `observation_records`.

pub use crate::observation_records::diagnostic::{DiagnosticLevel, DiagnosticRecord};
pub use crate::observation_records::diagnostic::sink::{
    DiagnosticSink, DiagnosticSinkError, TokioMpscDiagnosticSink, diag_actuation_failure,
    diag_front_headlamp_confirmed, diag_state_transition, diag_timer_tick,
    diag_transition_sink_closed, diag_transition_sink_full, diag_warning,
    spawn_stdout_diagnostic_observer,
};

/// Deprecated alias — prefer [`DiagnosticRecord`].
pub type DiagnosticMessage = DiagnosticRecord;
