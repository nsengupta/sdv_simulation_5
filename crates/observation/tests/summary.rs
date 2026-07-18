mod support;

use std::time::Duration;

use common::facade::{DiagnosticLevel, UnixTimestamp};
use observation::{RunReader, RunWriter, summarize};
use support::{RUN_ID, SESSION_SECONDS, fixed_run_metadata, sample_diagnostic, sample_ledger};

#[test]
fn empty_streams_render_absent_summary_fields_as_dashes() {
    let temp = tempfile::tempdir().unwrap();
    let metadata = fixed_run_metadata();
    let writer = RunWriter::create(temp.path(), metadata).unwrap();
    writer.finish().unwrap();

    let run = RunReader::open(temp.path().join(RUN_ID))
        .unwrap()
        .load()
        .unwrap();
    let summary = summarize(&run);

    assert_eq!(summary.first_recorded_at, None);
    assert_eq!(summary.last_recorded_at, None);
    assert_eq!(summary.initial_state, None);
    assert_eq!(summary.final_state, None);
    assert_eq!(summary.final_ledger_sequence, None);
    let rendered = summary.to_string();
    assert!(rendered.contains("session_started_at: "));
    assert!(rendered.contains("first_recorded_at: -\n"));
    assert!(rendered.contains("last_recorded_at: -\n"));
    assert!(rendered.contains("initial_state: -\n"));
    assert!(rendered.contains("final_state: -\n"));
    assert!(rendered.contains("final_ledger_sequence: -\n"));
}

#[test]
fn interleaved_streams_use_cross_stream_time_range_and_diagnostic_levels() {
    let temp = tempfile::tempdir().unwrap();
    let metadata = fixed_run_metadata();
    let mut writer = RunWriter::create(temp.path(), metadata).unwrap();

    let mut late_warning = sample_diagnostic();
    late_warning.recorded_at =
        UnixTimestamp::from_duration_since_epoch(Duration::new(SESSION_SECONDS + 4, 0));
    writer.record_diagnostic(&late_warning).unwrap();

    let mut early_error = sample_diagnostic();
    early_error.level = DiagnosticLevel::Error;
    early_error.recorded_at =
        UnixTimestamp::from_duration_since_epoch(Duration::new(SESSION_SECONDS + 2, 0));
    writer.record_diagnostic(&early_error).unwrap();

    let mut middle_ledger = sample_ledger();
    middle_ledger.recorded_at =
        UnixTimestamp::from_duration_since_epoch(Duration::new(SESSION_SECONDS + 3, 0));
    writer.record_ledger(&middle_ledger).unwrap();

    let mut latest_ledger = sample_ledger();
    latest_ledger.record_seq = 8;
    latest_ledger.recorded_at =
        UnixTimestamp::from_duration_since_epoch(Duration::new(SESSION_SECONDS + 5, 0));
    writer.record_ledger(&latest_ledger).unwrap();
    writer.finish().unwrap();

    let run = RunReader::open(temp.path().join(RUN_ID))
        .unwrap()
        .load()
        .unwrap();
    let summary = summarize(&run);

    assert_eq!(summary.warning_count, 1);
    assert_eq!(summary.error_count, 1);
    assert_eq!(
        summary.first_recorded_at.unwrap().to_string(),
        "2026-07-17 | 04:00:02:000000000 (UTC)"
    );
    assert_eq!(
        summary.last_recorded_at.unwrap().to_string(),
        "2026-07-17 | 04:00:05:000000000 (UTC)"
    );
    assert_eq!(summary.final_ledger_sequence, Some(8));
}
