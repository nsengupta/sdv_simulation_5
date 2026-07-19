//! Gateway-style tee: boot diagnostic reaches both RunWriter files and a UDS client.

use std::env;
use std::path::PathBuf;
use std::time::Duration;

use common::DiagnosticKind;
use common::facade::DiagnosticRecord;
use gateway::gateway_runtime::TwinRuntimeBuilder;
use observation::{
    LiveMessage, LiveRecordDto, LiveStream, ObservationTee, RunId, RunMetadata, RunReader,
    UdsLiveSink, UdsLiveSource, UnixTimestampV1, ensure_tmp_parent, resolve_uds_path,
};
use tokio::sync::mpsc;

const IDENTITY: &str = "tee-uds-test";

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
async fn boot_reaches_files_and_uds_client() {
    let dir = tempfile::tempdir().unwrap();
    let _cwd = CwdGuard::enter(dir.path());
    let uds = resolve_uds_path(None).unwrap();
    ensure_tmp_parent(&uds).unwrap();

    let accept = tokio::spawn({
        let uds = uds.clone();
        async move {
            UdsLiveSink::bind_and_accept(
                uds,
                Duration::from_secs(5),
                LiveMessage::hello(IDENTITY),
            )
            .await
            .expect("accept")
        }
    });

    tokio::time::sleep(Duration::from_millis(30)).await;
    let mut source = UdsLiveSource::connect(&uds).await.expect("connect");
    let sink = accept.await.expect("join accept");

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
    let mut tee = ObservationTee::new(writer, Some(sink));
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
