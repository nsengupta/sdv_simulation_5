use std::time::{Duration, SystemTime, UNIX_EPOCH};

use observation::schema::v1::{
    DiagnosticKindV1, DiagnosticLevelV1, DiagnosticPayloadV1, RunId, StreamEnvelopeV1,
    UnixTimestampV1,
};
use observation::schema::CURRENT_SCHEMA_VERSION;
use observation::{
    LiveMessage, LiveRecordDto, LiveSink, LiveStream, ObservationError, ZenohLiveSink,
    ZenohLiveSource,
};

fn unique_key(prefix: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("{prefix}/{nanos}")
}

#[tokio::test(flavor = "multi_thread")]
async fn zenoh_waits_for_subscriber_then_hello_and_event() {
    let key = unique_key("sdv/test/obs");
    let hello = LiveMessage::hello("test-vehicle");

    let sink_task = tokio::spawn({
        let key = key.clone();
        let hello = hello.clone();
        async move {
            ZenohLiveSink::open_and_wait_subscriber(key, Duration::from_secs(10), hello).await
        }
    });

    tokio::time::sleep(Duration::from_millis(200)).await;
    let mut source = ZenohLiveSource::subscribe(key.clone())
        .await
        .expect("subscribe");

    let mut sink = sink_task.await.expect("join").expect("open sink");
    let first = source.recv().await.expect("recv").expect("hello msg");
    assert!(matches!(
        first,
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
    LiveSink::emit(&mut sink, &LiveMessage::diagnostic_event(envelope)).unwrap();

    let second = source.recv().await.expect("recv").expect("event");
    match second {
        LiveMessage::Event {
            stream: LiveStream::Diagnostic,
            record: LiveRecordDto::Diagnostic(got),
        } => {
            assert_eq!(got.payload.kind, DiagnosticKindV1::Boot);
        }
        other => panic!("unexpected: {other:?}"),
    }

    LiveSink::finish(&mut sink).unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn zenoh_sink_times_out_without_subscriber() {
    let key = unique_key("sdv/test/obs-timeout");
    let result = ZenohLiveSink::open_and_wait_subscriber(
        key,
        Duration::from_millis(300),
        LiveMessage::hello("test-vehicle"),
    )
    .await;
    match result {
        Err(ObservationError::Zenoh { .. }) => {}
        Ok(_) => panic!("must timeout without subscriber"),
        Err(other) => panic!("unexpected error: {other}"),
    }
}
