mod support;

use observation::{
    LiveMessage, LiveRecordDto, LiveSource, LiveStream, MemoryLiveLink, ObservationTee, RunReader,
    RunWriter, diagnostic_from_envelope,
};

#[test]
fn tee_writes_files_and_live_sink() {
    let temp = tempfile::tempdir().unwrap();
    let metadata = support::fixed_run_metadata();
    let writer = RunWriter::create(temp.path(), metadata).unwrap();
    let (sink, mut source) = MemoryLiveLink::pair();
    let mut tee = ObservationTee::new(writer, Some(sink));

    let diagnostic = support::sample_diagnostic();
    let ledger = support::sample_ledger();
    tee.record_diagnostic(&diagnostic).unwrap();
    tee.record_ledger(&ledger).unwrap();
    let run_dir = tee.run_dir().to_path_buf();
    tee.finish().unwrap();

    let stored = RunReader::open(&run_dir).unwrap().load().unwrap();
    assert_eq!(stored.diagnostics.len(), 1);
    assert_eq!(stored.ledger.len(), 1);

    let first = source.recv_blocking().unwrap().expect("diagnostic event");
    let second = source.recv_blocking().unwrap().expect("ledger event");
    match first {
        LiveMessage::Event {
            stream: LiveStream::Diagnostic,
            record: LiveRecordDto::Diagnostic(env),
        } => {
            let restored = diagnostic_from_envelope(&env).unwrap();
            assert_eq!(restored.kind, diagnostic.kind);
        }
        other => panic!("unexpected first message: {other:?}"),
    }
    match second {
        LiveMessage::Event {
            stream: LiveStream::Ledger,
            record: LiveRecordDto::Ledger(env),
        } => {
            assert_eq!(env.payload.record_seq, ledger.record_seq);
        }
        other => panic!("unexpected second message: {other:?}"),
    }
}
