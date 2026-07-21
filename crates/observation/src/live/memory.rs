//! In-memory live link for unit tests.

use std::sync::mpsc::{self, Receiver, Sender};

use crate::ObservationError;
use crate::live::message::LiveMessage;
use crate::live::traits::{LiveSink, LiveSource};

pub struct MemoryLiveSink {
    tx: Option<Sender<LiveMessage>>,
}

pub struct MemoryLiveSource {
    rx: Receiver<LiveMessage>,
}

pub struct MemoryLiveLink;

impl MemoryLiveLink {
    pub fn pair() -> (MemoryLiveSink, MemoryLiveSource) {
        let (tx, rx) = mpsc::channel();
        (MemoryLiveSink { tx: Some(tx) }, MemoryLiveSource { rx })
    }
}

impl LiveSink for MemoryLiveSink {
    fn emit(&mut self, message: &LiveMessage) -> Result<(), ObservationError> {
        let Some(tx) = &self.tx else {
            return Ok(());
        };
        tx.send(message.clone())
            .map_err(|_| ObservationError::InvalidRecord {
                stream: std::path::PathBuf::from("<memory-live>"),
                line: 0,
                message: "live sink disconnected".into(),
            })
    }

    fn finish(&mut self) -> Result<(), ObservationError> {
        self.tx = None;
        Ok(())
    }
}

impl LiveSource for MemoryLiveSource {
    fn recv_blocking(&mut self) -> Result<Option<LiveMessage>, ObservationError> {
        match self.rx.recv() {
            Ok(message) => Ok(Some(message)),
            Err(_) => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_link_preserves_order() {
        let (mut sink, mut source) = MemoryLiveLink::pair();
        let hello = LiveMessage::hello("test-vehicle");
        let event = LiveMessage::hello("ignored-for-shape");
        // Use two hellos for ordering; event construction needs envelopes elsewhere.
        sink.emit(&hello).unwrap();
        sink.emit(&event).unwrap();
        sink.finish().unwrap();

        assert_eq!(source.recv_blocking().unwrap(), Some(hello));
        assert_eq!(source.recv_blocking().unwrap(), Some(event));
        assert_eq!(source.recv_blocking().unwrap(), None);
    }
}
