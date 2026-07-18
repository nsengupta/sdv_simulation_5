//! Diagnostic record types — streamed car-state observability from the digital twin (L3).
//!
//! The twin emits [`DiagnosticRecord`] values through an injected sink (see [`sink`]).
//! The runtime decides who reads the RX side and how to display them.
//!
//! Each record carries the same session timing pair as the transition ledger:
//! [`session_started_at`](DiagnosticRecord::session_started_at) (when the twin started) and
//! [`recorded_at`](DiagnosticRecord::recorded_at) (when this diagnostic was emitted), both
//! projected through [`SessionClock`].
//!
//! **Wire format:** archival codecs live in the L6 `observation` crate. Do not embed protobuf or
//! JSON schema derives on the record struct itself.

pub mod sink;

use std::time::{Duration, Instant};

use super::transition::{SessionClock, UnixTimestamp};

/// Severity classification for diagnostic records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticLevel {
    Info,
    Action,
    Alert,
    Warning,
    Error,
}

/// A single diagnostic event emitted by the digital twin actor or its components.
#[derive(Debug, Clone)]
pub struct DiagnosticRecord {
    pub level: DiagnosticLevel,
    pub source: &'static str,
    pub message: String,
    /// When this twin run started — same anchor as ledger
    /// [`super::transition::PublishedTransitionRecord::session_started_at`].
    pub session_started_at: UnixTimestamp,
    /// When this diagnostic was recorded, projected through [`SessionClock`].
    pub recorded_at: UnixTimestamp,
}

impl DiagnosticRecord {
    /// Build a diagnostic stamped through the twin's [`SessionClock`].
    pub fn at_session(
        clock: &SessionClock,
        level: DiagnosticLevel,
        source: &'static str,
        message: impl Into<String>,
    ) -> Self {
        Self {
            level,
            source,
            message: message.into(),
            session_started_at: clock.session_started_at(),
            recorded_at: clock.project(&Instant::now()),
        }
    }

    /// Elapsed time since session start, on the twin clock (derivable from the emitted pair).
    pub fn elapsed_since_session(&self) -> Duration {
        elapsed_since_session(self.recorded_at, self.session_started_at)
    }
}

/// Shared with the transition ledger: `recorded_at - session_started_at`.
pub fn elapsed_since_session(
    recorded_at: UnixTimestamp,
    session_started_at: UnixTimestamp,
) -> Duration {
    recorded_at.saturating_duration_since(session_started_at)
}

/// Shorthand constructors for common twin diagnostics.
impl DiagnosticRecord {
    pub fn info(clock: &SessionClock, source: &'static str, msg: impl Into<String>) -> Self {
        Self::at_session(clock, DiagnosticLevel::Info, source, msg)
    }

    pub fn action(clock: &SessionClock, source: &'static str, msg: impl Into<String>) -> Self {
        Self::at_session(clock, DiagnosticLevel::Action, source, msg)
    }

    pub fn alert(clock: &SessionClock, source: &'static str, msg: impl Into<String>) -> Self {
        Self::at_session(clock, DiagnosticLevel::Alert, source, msg)
    }

    pub fn warning(clock: &SessionClock, source: &'static str, msg: impl Into<String>) -> Self {
        Self::at_session(clock, DiagnosticLevel::Warning, source, msg)
    }

    pub fn error(clock: &SessionClock, source: &'static str, msg: impl Into<String>) -> Self {
        Self::at_session(clock, DiagnosticLevel::Error, source, msg)
    }
}

impl std::fmt::Display for DiagnosticRecord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let icon = match self.level {
            DiagnosticLevel::Info => "ℹ️",
            DiagnosticLevel::Action => "⚡",
            DiagnosticLevel::Alert => "🚨",
            DiagnosticLevel::Warning => "⚠️",
            DiagnosticLevel::Error => "❌",
        };
        write!(f, "[{icon}][{}] {}", self.source, self.message)
    }
}
