//! Headless capture: install twin, wait for boot, write RunWriter (no UDS, no CAN spawn).

use std::time::Duration;

use common::DiagnosticKind;
use common::facade::DiagnosticRecord;
use gateway::gateway_runtime::TwinRuntimeBuilder;
use observation::{ObservationTee, RunId, RunMetadata, RunReader, UnixTimestampV1};
use tokio::sync::mpsc;

const IDENTITY: &str = "capture-headless-test";

#[tokio::test]
async fn install_boot_is_captured_to_run_directory() {
    let temp = tempfile::tempdir().unwrap();
    let (diag_tx, mut diag_rx) = mpsc::unbounded_channel::<DiagnosticRecord>();
    let (trans_tx, _trans_rx) = mpsc::channel(16);

    let mut builder = TwinRuntimeBuilder::new()
        .with_car_identity(IDENTITY)
        .with_auto_power_on(false)
        .with_diagnostic_channel(diag_tx)
        .with_transition_channel(trans_tx);

    let (_controller, _opts) = builder.install_controller().await.expect("install");

    let boot = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            match diag_rx.recv().await {
                Some(record) if matches!(record.kind, DiagnosticKind::Boot) => break record,
                Some(_) => continue,
                None => panic!("diagnostic channel closed"),
            }
        }
    })
    .await
    .expect("boot timeout");

    let metadata = RunMetadata::now(
        RunId::new_v4(),
        UnixTimestampV1::from_live(boot.session_started_at),
        IDENTITY,
        None,
    )
    .unwrap();
    let writer = observation::RunWriter::create(temp.path(), metadata).unwrap();
    let mut tee = ObservationTee::<observation::MemoryLiveSink>::new(writer, None);
    tee.record_diagnostic(&boot).unwrap();
    let run_dir = tee.run_dir().to_path_buf();
    tee.finish().unwrap();

    let stored = RunReader::open(&run_dir).unwrap().load().unwrap();
    assert_eq!(stored.diagnostics.len(), 1);
    assert!(matches!(
        stored.diagnostics[0].payload.kind,
        observation::schema::v1::DiagnosticKindV1::Boot
    ));
}
