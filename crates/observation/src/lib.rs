mod error;
pub mod live;
mod reader;
pub mod schema;
mod summary;
mod tee;
mod uds_path;
mod writer;

pub use error::ObservationError;
pub use live::{
    AnyLiveSink, AnyLiveSource, LiveMessage, LiveRecordDto, LiveSink, LiveSource, LiveStream,
    MemoryLiveLink, MemoryLiveSink, MemoryLiveSource, UdsLiveSink, UdsLiveSource, ZenohLiveSink,
    ZenohLiveSource,
};
pub use reader::{DiagnosticRecords, LedgerRecords, RunReader, StoredRun};
pub use schema::v1::{
    RunId, RunMetadata, ScenarioMetadata, Timestamp, UnixTimestampV1, diagnostic_from_envelope,
    ledger_from_envelope,
};
pub use summary::{RunSummary, summarize};
pub use tee::ObservationTee;
pub use uds_path::{DEFAULT_UDS_FILE_NAME, ensure_tmp_parent, resolve_uds_path};
pub use writer::RunWriter;

#[cfg(test)]
mod tests {
    use common::facade::{
        DiagnosticLevel, DiagnosticRecord, PublishedDomainAction,
        PublishedFrontHeadlampIncompleteCause, PublishedFrontHeadlampSwitchDirection,
        PublishedFsmEvent, PublishedFsmState, PublishedOperational, PublishedTransitionRecord,
    };

    #[test]
    fn all_archival_inputs_are_available_through_the_facade() {
        fn accepts<T>() {}
        accepts::<DiagnosticLevel>();
        accepts::<DiagnosticRecord>();
        accepts::<PublishedDomainAction>();
        accepts::<PublishedFrontHeadlampIncompleteCause>();
        accepts::<PublishedFrontHeadlampSwitchDirection>();
        accepts::<PublishedFsmEvent>();
        accepts::<PublishedFsmState>();
        accepts::<PublishedOperational>();
        accepts::<PublishedTransitionRecord>();
    }
}
