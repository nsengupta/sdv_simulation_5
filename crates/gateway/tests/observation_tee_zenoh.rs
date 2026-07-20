//! Gateway-style tee: boot diagnostic reaches both RunWriter files and a Zenoh client.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use common::DiagnosticKind;
use common::facade::DiagnosticRecord;
use gateway::gateway_runtime::TwinRuntimeBuilder;
use observation::{
    AnyLiveSink, LiveMessage, LiveRecordDto, LiveStream, ObservationTee, RunId, RunMetadata,
    RunReader, UnixTimestampV1, ZenohLiveSink, ZenohLiveSource,
};
use tokio::sync::mpsc;

const IDENTITY: &str = "tee-zenoh-test";

fn unique_key(prefix: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("{prefix}/{nanos}")
}

#[tokio::test(flavor = "multi_thread")]
async fn boot_reaches_files_and_zenoh_client() {
    let dir = tempfile::tempdir().unwrap();
    let key = unique_key("sdv/test/tee");

    let open = tokio::spawn({
        let key = key.clone();
        async move {
            ZenohLiveSink::open_and_wait_subscriber(
                key,
                Duration::from_secs(10),
                LiveMessage::hello(IDENTITY),
            )
            .await
            .expect("open sink")
        }
    });

    tokio::time::sleep(Duration::from_millis(200)).await;
    let mut source = ZenohLiveSource::subscribe(key.clone())
        .await
        .expect("subscribe");
    let sink = open.await.expect("join open");

    let hello = source.recv().await.expect("hello io").expect("hello msg");
    assert!(matches!(hello, LiveMessage::Hello { .. }));

    let (diag_tx, mut diag_rx) = mpsc::unbounded_channel::<DiagnosticRecord>();
    let (trans_tx, _trans_rx) = mpsc::channel(8);
    let mut builder = TwinRuntimeBuilder::new()
        .with_car_identity(IDENTITY)
        .with_auto_power_on(false)
        .with_diagnostic_channel(diag_tx)
        .with_transition_channel(trans_tx);
    let (_controller, _) = builder.install_controller().await.expect("install");

    let boot = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            match diag_rx.recv().await {
                Some(record) if matches!(record.kind, DiagnosticKind::Boot) => break record,
                Some(_) => continue,
                None => panic!("channel closed"),
            }
        }
    })
    .await
    .expect("boot");

    let metadata = RunMetadata::now(
        RunId::new_v4(),
        UnixTimestampV1::from_live(boot.session_started_at),
        IDENTITY,
        None,
    )
    .unwrap();
    let writer = observation::RunWriter::create(dir.path().join("observations"), metadata).unwrap();
    let mut tee = ObservationTee::new(writer, Some(AnyLiveSink::Zenoh(sink)));
    tee.record_diagnostic(&boot).unwrap();
    let run_dir = tee.run_dir().to_path_buf();

    let event = tokio::time::timeout(Duration::from_secs(2), source.recv())
        .await
        .expect("event timeout")
        .expect("event io")
        .expect("event");
    match event {
        LiveMessage::Event {
            stream: LiveStream::Diagnostic,
            record: LiveRecordDto::Diagnostic(env),
        } => {
            assert!(matches!(
                env.payload.kind,
                observation::schema::v1::DiagnosticKindV1::Boot
            ));
        }
        other => panic!("unexpected {other:?}"),
    }

    tee.finish().unwrap();
    let stored = RunReader::open(&run_dir).unwrap().load().unwrap();
    assert_eq!(stored.diagnostics.len(), 1);
}
