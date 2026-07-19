//! Transport-agnostic live observation sink/source.

use crate::ObservationError;
use crate::live::message::LiveMessage;

/// Outbound live observation feed (Gateway side). UDS and future Zenoh impls.
pub trait LiveSink: Send {
    fn emit(&mut self, message: &LiveMessage) -> Result<(), ObservationError>;
    fn finish(&mut self) -> Result<(), ObservationError>;
}

/// Inbound live observation feed (Dashboard side).
pub trait LiveSource: Send {
    fn recv_blocking(&mut self) -> Result<Option<LiveMessage>, ObservationError>;
}
