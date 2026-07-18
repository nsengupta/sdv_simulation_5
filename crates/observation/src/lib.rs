mod error;
mod reader;
pub mod schema;
mod summary;
mod writer;

pub use error::ObservationError;
pub use reader::{DiagnosticRecords, LedgerRecords, RunReader, StoredRun};
pub use schema::v1::{RunId, RunMetadata, ScenarioMetadata, Timestamp, UnixTimestampV1};
pub use summary::{RunSummary, summarize};
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
