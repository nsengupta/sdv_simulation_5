mod support;

use observation::schema::v1::{
    diagnostic_envelope, diagnostic_from_envelope, ledger_envelope, ledger_from_envelope,
};

#[test]
fn diagnostic_round_trips_through_envelope() {
    let metadata = support::fixed_run_metadata();
    let original = support::sample_diagnostic();
    let envelope = diagnostic_envelope(&metadata, &original).unwrap();
    let restored = diagnostic_from_envelope(&envelope).unwrap();

    assert_eq!(restored.level, original.level);
    assert_eq!(restored.source, original.source);
    assert_eq!(restored.kind, original.kind);
    assert_eq!(restored.session_started_at, original.session_started_at);
    assert_eq!(restored.recorded_at, original.recorded_at);
}

#[test]
fn ledger_round_trips_through_envelope() {
    let metadata = support::fixed_run_metadata();
    let original = support::sample_ledger();
    let envelope = ledger_envelope(&metadata, &original).unwrap();
    let restored = ledger_from_envelope(&envelope).unwrap();

    assert_eq!(restored.car_identity, original.car_identity);
    assert_eq!(restored.record_seq, original.record_seq);
    assert_eq!(restored.event, original.event);
    assert_eq!(restored.old_state, original.old_state);
    assert_eq!(restored.next_state, original.next_state);
    assert_eq!(restored.old_ctx, original.old_ctx);
    assert_eq!(restored.current_ctx, original.current_ctx);
    assert_eq!(restored.actions, original.actions);
    assert_eq!(restored.session_started_at, original.session_started_at);
    assert_eq!(restored.recorded_at, original.recorded_at);
}
