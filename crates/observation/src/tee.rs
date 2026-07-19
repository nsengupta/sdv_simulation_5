//! Convert live twin records once and fan out to archive + optional live sink.

use common::facade::{DiagnosticRecord, PublishedTransitionRecord};

use crate::ObservationError;
use crate::RunWriter;
use crate::live::{LiveMessage, LiveSink};
use crate::schema::v1::{diagnostic_envelope, ledger_envelope};

pub struct ObservationTee<S: LiveSink> {
    writer: RunWriter,
    sink: Option<S>,
}

impl<S: LiveSink> ObservationTee<S> {
    pub fn new(writer: RunWriter, sink: Option<S>) -> Self {
        Self { writer, sink }
    }

    pub fn run_dir(&self) -> &std::path::Path {
        self.writer.run_dir()
    }

    pub fn record_diagnostic(&mut self, record: &DiagnosticRecord) -> Result<(), ObservationError> {
        let envelope = diagnostic_envelope(self.writer.metadata(), record)?;
        self.writer.write_diagnostic_envelope(&envelope)?;
        if let Some(sink) = &mut self.sink {
            sink.emit(&LiveMessage::diagnostic_event(envelope))?;
        }
        Ok(())
    }

    pub fn record_ledger(
        &mut self,
        record: &PublishedTransitionRecord,
    ) -> Result<(), ObservationError> {
        let envelope = ledger_envelope(self.writer.metadata(), record)?;
        self.writer.write_ledger_envelope(&envelope)?;
        if let Some(sink) = &mut self.sink {
            sink.emit(&LiveMessage::ledger_event(envelope))?;
        }
        Ok(())
    }

    pub fn finish(mut self) -> Result<(), ObservationError> {
        if let Some(sink) = &mut self.sink {
            sink.finish()?;
        }
        self.writer.finish()
    }
}
