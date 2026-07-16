//! Diagnostic record types — streamed car-state observability from the digital twin (L3).
//!
//! The twin emits [`DiagnosticRecord`] values through an injected sink (see [`sink`]).
//! The runtime decides who reads the RX side and how to display them.
//!
//! Each record carries the same session timing pair as the transition ledger:
//! [`session_start_unix_nanos`](DiagnosticRecord::session_start_unix_nanos) (when the twin
//! started) and [`recorded_at_unix`](DiagnosticRecord::recorded_at_unix) (when this diagnostic
//! was emitted), both projected through [`SessionClock`].
//!
//! **Wire format:** no (de)serialization yet. When we adopt Protobuf (or another schema), add a
//! dedicated codec module that maps from [`DiagnosticRecord`] — do not embed protobuf derives on
//! the record struct itself.

pub mod sink;

use std::time::{Duration, Instant};

use super::transition::SessionClock;

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
    /// When this twin run started — same anchor as ledger [`super::transition::PublishedTransitionRecord::session_start_unix_nanos`].
    pub session_start_unix_nanos: u128,
    /// When this diagnostic was recorded, projected through [`SessionClock`] — same clock as ledger `recorded_at_unix`.
    pub recorded_at_unix: Duration,
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
            session_start_unix_nanos: clock.session_start_unix_nanos(),
            recorded_at_unix: clock.project(&Instant::now()),
        }
    }

    /// Elapsed time since session start, on the twin clock (derivable from the emitted pair).
    pub fn elapsed_since_session(&self) -> Duration {
        elapsed_since_session(self.recorded_at_unix, self.session_start_unix_nanos)
    }
}

/// Shared with the transition ledger: `recorded_at_unix - session_start`.
pub fn elapsed_since_session(recorded_at_unix: Duration, session_start_unix_nanos: u128) -> Duration {
    let session = Duration::from_nanos(session_start_unix_nanos as u64);
    recorded_at_unix.saturating_sub(session)
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
