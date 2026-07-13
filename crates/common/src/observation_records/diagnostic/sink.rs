//! Diagnostic-record sink abstraction and domain helpers (L4-facing emission plumbing).

use super::{DiagnosticLevel, DiagnosticRecord};
use crate::front_headlamp_log::{ACK_OFF, ACK_ON, MSG_ACK_OFF, MSG_ACK_ON};
use crate::fsm::{FrontHeadlampSwitchDirection, FsmState};
use crate::vehicle_physics::{
    extreme_operation_active, speed_threshold_exceeded, SPEED_EXTREME_OPERATION_THRESHOLD_KPH,
};
use crate::vehicle_state::VehicleContext;
use tokio::sync::mpsc;

/// Abstract sink for diagnostic records emitted by the digital twin.
///
/// The twin is unconcerned with who reads the other end; the runtime injects the
/// appropriate implementation and decides on display / persistence.
pub trait DiagnosticSink: Send + Sync {
    fn try_emit(&self, record: DiagnosticRecord) -> Result<(), DiagnosticSinkError>;
}

/// Errors that can occur when emitting a diagnostic record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticSinkError {
    Full,
    Closed,
}

/// Wraps a `tokio::sync::mpsc::Sender` as a [`DiagnosticSink`].
///
/// The twin calls `try_emit` (non-blocking); the receiver is on the runtime side.
pub struct TokioMpscDiagnosticSink {
    tx: mpsc::UnboundedSender<DiagnosticRecord>,
}

impl TokioMpscDiagnosticSink {
    pub fn new(tx: mpsc::UnboundedSender<DiagnosticRecord>) -> Self {
        Self { tx }
    }
}

impl DiagnosticSink for TokioMpscDiagnosticSink {
    fn try_emit(&self, record: DiagnosticRecord) -> Result<(), DiagnosticSinkError> {
        self.tx.send(record).map_err(|_| DiagnosticSinkError::Closed)
    }
}

/// Human-readable transition line enriched with the post-transition powertrain context
/// (speed / RPM) and a plain-language safety qualifier — meant for the diagnostic stream's
/// operator audience, kept to a single compact line.
pub fn diag_state_transition(
    identity: &str,
    new_state: &FsmState,
    ctx: &VehicleContext,
) -> DiagnosticRecord {
    let speed = ctx.powertrain.speed_kph;
    let rpm = ctx.powertrain.primary_rpm();

    let (label, detail) = match new_state {
        FsmState::Off => ("Off", String::new()),
        FsmState::Idle => ("Idle", format!(", speed = {speed} km/h, RPM = {rpm}")),
        FsmState::Driving => {
            let safety = if speed_threshold_exceeded(speed) {
                format!("over the {SPEED_EXTREME_OPERATION_THRESHOLD_KPH} km/h limit")
            } else {
                "within safe limit".to_string()
            };
            (
                "Driving",
                format!(", speed = {speed} km/h, RPM = {rpm} ({safety})"),
            )
        }
        FsmState::DrivingDangerously => (
            "DrivingDangerously",
            format!(", speed = {speed} km/h, RPM = {rpm}, lighting unsafe"),
        ),
        FsmState::ExtremeOperationWarning(_) => {
            let cause = if extreme_operation_active(rpm, speed) {
                "speed & RPM both extreme"
            } else {
                "speed over limit"
            };
            (
                "ExtremeOperationWarning",
                format!(", speed = {speed} km/h, RPM = {rpm} (EXCEEDS safe limit — {cause})"),
            )
        }
        FsmState::PreparingToStart { .. } => ("PreparingToStart", String::new()),
        FsmState::PreparingToStop { .. } => ("PreparingToStop", String::new()),
    };

    DiagnosticRecord::info(
        "VirtualCarActor",
        format!("[{identity}]: Transitioned to {label}{detail}"),
    )
}

pub fn diag_timer_tick(identity: &str) -> DiagnosticRecord {
    DiagnosticRecord::info("VirtualCarActor", format!("[{identity}]: received heartbeat TimerTick"))
}

pub fn diag_actuation_failure(identity: &str, action: &str, err: &str) -> DiagnosticRecord {
    DiagnosticRecord::error("VirtualCarActor", format!("[{identity}]: actuation failure for {action}: {err}"))
}

/// Warning surfaced from a `DomainAction::LogWarning` intent emitted by the pure step.
///
/// `LogWarning` is observability, not actuation (WI-5 / Q5), so the actor routes it here to the
/// diagnostic sink rather than through the actuation path.
pub fn diag_warning(identity: &str, message: &str) -> DiagnosticRecord {
    DiagnosticRecord::warning("VirtualCarActor", format!("[{identity}]: {message}"))
}

/// Info diagnostic surfaced when a front-headlamp command is **positively acknowledged**.
///
/// Symmetric with the NACK/timeout warning path (`diag_warning` ← `DomainAction::LogWarning`): a
/// clean ACK used to be silent on the diagnostic stream even though it was recorded in the
/// transition ledger (`old_ctx`→`current_ctx`). The actor emits this from the step result when the
/// lighting state settles `*Requested → On/Off`. Wording matches the gateway ingress line so the
/// two surfaces read identically.
pub fn diag_front_headlamp_confirmed(
    identity: &str,
    direction: FrontHeadlampSwitchDirection,
) -> DiagnosticRecord {
    let (icon, msg) = match direction {
        FrontHeadlampSwitchDirection::On => (ACK_ON, MSG_ACK_ON),
        FrontHeadlampSwitchDirection::Off => (ACK_OFF, MSG_ACK_OFF),
    };
    DiagnosticRecord::info("VirtualCarActor", format!("[{identity}]: {icon} {msg}"))
}

pub fn diag_transition_sink_full(identity: &str) -> DiagnosticRecord {
    DiagnosticRecord::warning("VirtualCarActor", format!("[{identity}]: dropping transition record: sink full"))
}

pub fn diag_transition_sink_closed(identity: &str) -> DiagnosticRecord {
    DiagnosticRecord::warning("VirtualCarActor", format!("[{identity}]: dropping transition record: sink closed"))
}

/// Spawns a task that reads [`DiagnosticRecord`] values from `rx` and prints each
/// to stdout (or stderr for error-level).
///
/// The runtime calls this when it wants to attach stdout to the twin's diagnostic stream.
pub fn spawn_stdout_diagnostic_observer(
    mut rx: mpsc::UnboundedReceiver<DiagnosticRecord>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        use std::io::Write;
        while let Some(record) = rx.recv().await {
            match record.level {
                DiagnosticLevel::Error | DiagnosticLevel::Alert => {
                    let _ = writeln!(std::io::stderr(), "{record}");
                }
                _ => {
                    let _ = writeln!(std::io::stdout(), "{record}");
                }
            }
        }
    })
}
