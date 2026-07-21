//! NDJSON live-wire messages: `hello` + `event` (schema v2 DTOs).

use serde::{Deserialize, Serialize};

use crate::ObservationError;
use crate::schema::CURRENT_SCHEMA_VERSION;
use crate::schema::v1::{DiagnosticPayloadV1, LedgerPayloadV1, StreamEnvelopeV1, VehicleV1};

/// One newline-delimited JSON object on the live UDS stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[allow(clippy::large_enum_variant)] // Event carries full schema-v2 envelopes by design.
pub enum LiveMessage {
    Hello {
        schema_version: u32,
        vehicle: VehicleV1,
    },
    Event {
        stream: LiveStream,
        record: LiveRecordDto,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LiveStream {
    Diagnostic,
    Ledger,
}

/// Schema-v2 stream envelope carried inside a live `event` line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum LiveRecordDto {
    Diagnostic(StreamEnvelopeV1<DiagnosticPayloadV1>),
    Ledger(StreamEnvelopeV1<LedgerPayloadV1>),
}

impl LiveMessage {
    pub fn hello(vehicle_identity: impl Into<String>) -> Self {
        Self::Hello {
            schema_version: CURRENT_SCHEMA_VERSION,
            vehicle: VehicleV1 {
                identity: vehicle_identity.into(),
            },
        }
    }

    pub fn diagnostic_event(record: StreamEnvelopeV1<DiagnosticPayloadV1>) -> Self {
        Self::Event {
            stream: LiveStream::Diagnostic,
            record: LiveRecordDto::Diagnostic(record),
        }
    }

    pub fn ledger_event(record: StreamEnvelopeV1<LedgerPayloadV1>) -> Self {
        Self::Event {
            stream: LiveStream::Ledger,
            record: LiveRecordDto::Ledger(record),
        }
    }

    /// Serialize as one JSON object with a trailing newline.
    pub fn to_json_line(&self) -> Result<String, ObservationError> {
        let mut line = serde_json::to_string(self).map_err(|source| ObservationError::Json {
            path: std::path::PathBuf::from("<live>"),
            source,
        })?;
        line.push('\n');
        Ok(line)
    }

    /// Parse one JSON object line (trailing newline optional).
    pub fn from_json_line(line: &str) -> Result<Self, ObservationError> {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        let message: Self =
            serde_json::from_str(trimmed).map_err(|source| ObservationError::Json {
                path: std::path::PathBuf::from("<live>"),
                source,
            })?;
        message.validate_stream_record()?;
        Ok(message)
    }

    fn validate_stream_record(&self) -> Result<(), ObservationError> {
        match self {
            Self::Hello { .. } => Ok(()),
            Self::Event {
                stream: LiveStream::Diagnostic,
                record: LiveRecordDto::Diagnostic(_),
            }
            | Self::Event {
                stream: LiveStream::Ledger,
                record: LiveRecordDto::Ledger(_),
            } => Ok(()),
            Self::Event { stream, .. } => Err(ObservationError::InvalidRecord {
                stream: std::path::PathBuf::from("<live>"),
                line: 0,
                message: format!("stream {stream:?} does not match record payload kind"),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::CURRENT_SCHEMA_VERSION;
    use crate::schema::v1::{
        DiagnosticKindV1, DiagnosticLevelV1, RunId, UnixTimestampV1, diagnostic_envelope,
    };
    use common::facade::{DiagnosticKind, DiagnosticLevel, DiagnosticRecord, UnixTimestamp};
    use std::time::Duration;

    fn sample_diagnostic_envelope() -> StreamEnvelopeV1<DiagnosticPayloadV1> {
        let metadata = crate::RunMetadata::new(
            RunId::parse("00000000-0000-4000-8000-000000000001").unwrap(),
            UnixTimestampV1::new(1_784_260_800, 0).unwrap(),
            UnixTimestampV1::new(1_784_260_800, 0).unwrap(),
            "test-vehicle",
            None,
        );
        let record = DiagnosticRecord {
            level: DiagnosticLevel::Warning,
            source: "VirtualCarActor",
            kind: DiagnosticKind::Text {
                text: "fixed warning".into(),
            },
            session_started_at: UnixTimestamp::from_duration_since_epoch(Duration::from_secs(
                1_784_260_800,
            )),
            recorded_at: UnixTimestamp::from_duration_since_epoch(Duration::from_secs(
                1_784_260_801,
            )),
        };
        diagnostic_envelope(&metadata, &record).unwrap()
    }

    #[test]
    fn hello_round_trips_as_json_line() {
        let message = LiveMessage::hello("My-Opel-Corsa-1.4-GSi");
        let line = message.to_json_line().unwrap();
        assert!(line.ends_with('\n'));
        assert!(line.contains("\"type\":\"hello\""));
        assert!(line.contains(&format!("\"schema_version\":{CURRENT_SCHEMA_VERSION}")));
        let parsed = LiveMessage::from_json_line(&line).unwrap();
        assert_eq!(parsed, message);
    }

    #[test]
    fn diagnostic_event_round_trips_as_json_line() {
        let envelope = sample_diagnostic_envelope();
        let message = LiveMessage::diagnostic_event(envelope.clone());
        let line = message.to_json_line().unwrap();
        assert!(line.contains("\"type\":\"event\""));
        assert!(line.contains("\"stream\":\"diagnostic\""));
        let parsed = LiveMessage::from_json_line(&line).unwrap();
        assert_eq!(parsed, message);
        match parsed {
            LiveMessage::Event {
                stream: LiveStream::Diagnostic,
                record: LiveRecordDto::Diagnostic(got),
            } => {
                assert_eq!(
                    got.payload.kind,
                    DiagnosticKindV1::Text {
                        text: "fixed warning".into()
                    }
                );
                assert_eq!(got.payload.level, DiagnosticLevelV1::Warning);
            }
            other => panic!("unexpected message: {other:?}"),
        }
    }
}
