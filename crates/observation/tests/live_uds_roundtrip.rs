use std::env;
use std::path::PathBuf;
use std::time::Duration;

use observation::schema::CURRENT_SCHEMA_VERSION;
use observation::schema::v1::{
    DiagnosticKindV1, DiagnosticLevelV1, DiagnosticPayloadV1, RunId, StreamEnvelopeV1,
    UnixTimestampV1,
};
use observation::{
    LiveMessage, LiveRecordDto, LiveStream, UdsLiveSink, UdsLiveSource, ensure_tmp_parent,
    resolve_uds_path,
};

struct CwdGuard {
    original: PathBuf,
}

impl CwdGuard {
    fn enter(path: &std::path::Path) -> Self {
        let original = env::current_dir().unwrap();
        env::set_current_dir(path).unwrap();
        Self { original }
    }
}

impl Drop for CwdGuard {
    fn drop(&mut self) {
        let _ = env::set_current_dir(&self.original);
    }
}

#[tokio::test]
async fn uds_hello_and_event_round_trip_under_cwd_tmp() {
    let dir = tempfile::tempdir().unwrap();
    let _guard = CwdGuard::enter(dir.path());
    let path = resolve_uds_path(None).unwrap();
    ensure_tmp_parent(&path).unwrap();

    let accept = tokio::spawn({
        let path = path.clone();
        async move {
            UdsLiveSink::bind_and_accept(
                path,
                Duration::from_secs(5),
                LiveMessage::hello("test-vehicle"),
            )
            .await
            .unwrap()
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;
    let mut source = UdsLiveSource::connect(&path).await.unwrap();
    let mut sink = accept.await.unwrap();

    let hello = source.recv().await.unwrap().expect("hello");
    assert!(matches!(
        hello,
        LiveMessage::Hello {
            schema_version: CURRENT_SCHEMA_VERSION,
            ..
        }
    ));

    let envelope = StreamEnvelopeV1 {
        schema_version: CURRENT_SCHEMA_VERSION,
        run_id: RunId::parse("00000000-0000-4000-8000-000000000001").unwrap(),
        vehicle_identity: "test-vehicle".into(),
        recorded_at: UnixTimestampV1::new(1, 0).unwrap(),
        payload: DiagnosticPayloadV1 {
            level: DiagnosticLevelV1::Info,
            source: "test".into(),
            kind: DiagnosticKindV1::Boot,
            session_started_at: UnixTimestampV1::new(1, 0).unwrap(),
        },
    };
    observation::LiveSink::emit(&mut sink, &LiveMessage::diagnostic_event(envelope)).unwrap();

    let event = source.recv().await.unwrap().expect("event");
    match event {
        LiveMessage::Event {
            stream: LiveStream::Diagnostic,
            record: LiveRecordDto::Diagnostic(got),
        } => {
            assert_eq!(got.payload.kind, DiagnosticKindV1::Boot);
        }
        other => panic!("unexpected: {other:?}"),
    }

    observation::LiveSink::finish(&mut sink).unwrap();
}
