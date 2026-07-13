//! Diagnostic record types — streamed car-state observability from the digital twin (L3).
//!
//! The twin emits [`DiagnosticRecord`] values through an injected sink (see [`sink`]).
//! The runtime decides who reads the RX side and how to display them.
//!
//! **Wire format:** no (de)serialization yet. When we adopt Protobuf (or another schema), add a
//! dedicated codec module that maps from [`DiagnosticRecord`] — do not embed protobuf derives on
//! the record struct itself.

pub mod sink;

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
    pub timestamp_utc_nanos: u128,
}

impl DiagnosticRecord {
    pub fn new(level: DiagnosticLevel, source: &'static str, message: impl Into<String>) -> Self {
        Self {
            level,
            source,
            message: message.into(),
            timestamp_utc_nanos: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0),
        }
    }
}

/// Shorthand constructors for common twin diagnostics.
impl DiagnosticRecord {
    pub fn info(source: &'static str, msg: impl Into<String>) -> Self {
        Self::new(DiagnosticLevel::Info, source, msg)
    }

    pub fn action(source: &'static str, msg: impl Into<String>) -> Self {
        Self::new(DiagnosticLevel::Action, source, msg)
    }

    pub fn alert(source: &'static str, msg: impl Into<String>) -> Self {
        Self::new(DiagnosticLevel::Alert, source, msg)
    }

    pub fn warning(source: &'static str, msg: impl Into<String>) -> Self {
        Self::new(DiagnosticLevel::Warning, source, msg)
    }

    pub fn error(source: &'static str, msg: impl Into<String>) -> Self {
        Self::new(DiagnosticLevel::Error, source, msg)
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
