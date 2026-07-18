# Numeric Unix Timestamps Design

**Date:** 2026-07-18  
**Status:** Approved  
**Scope:** Live Twin observation timestamps and schema-v1 archival representation  
**Related:** [Phase 3 observation capture design](2026-07-17-phase-3-observation-capture-design.md),
[implementation plan](../plans/2026-07-18-numeric-unix-timestamps.md)

## Problem

The Twin already stamps diagnostic and ledger records with Unix-epoch wall times projected through
`SessionClock`. Schema v1 then converted those values to RFC3339 strings. That conversion:

- hid the Twin's native numeric meaning behind a presentation format;
- made exact arithmetic and sorting harder for future tools;
- left an inconsistent live API (`u128` nanoseconds vs `Duration` for the same concept).

## Decisions

1. **Live semantic type:** `UnixTimestamp` wrapping `Duration` since the Unix Epoch. Not raw `u128`.
2. **Storage JSON:** exact `{ "unix_seconds": u64, "nanosecond": u32 }` objects. Whole seconds, not
   milliseconds. Subsecond is always `0..=999_999_999`.
3. **Schema compatibility:** revise schema v1 in place (unreleased). Do not add v2.
4. **Manifest:** keep capture `created_at` and add Twin-authored `session_started_at`.
5. **Dashboard init:** require the boot diagnostic before creating the run directory so the
   manifest can store the Twin session start. Timeout creates no run directory.
6. **Presentation:** summary/UI print `yyyy-mm-dd | HH:mm:ss:nnnnnnnnn (UTC)`. Never the
   canonical stored form.

## Live API (`common`)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UnixTimestamp(Duration);

impl UnixTimestamp {
    pub fn from_duration_since_epoch(value: Duration) -> Self;
    pub fn duration_since_epoch(self) -> Duration;
    pub fn unix_seconds(self) -> u64;   // Duration::as_secs()
    pub fn nanosecond(self) -> u32;     // Duration::subsec_nanos()
}
```

Field naming on live records:

| Role | Field |
|------|--------|
| Twin session start | `session_started_at: UnixTimestamp` |
| Record wall time | `recorded_at: UnixTimestamp` |
| Warning entry | `entered_at: UnixTimestamp` |
| Headlamp ACK wait | `ack_pending_since: Option<UnixTimestamp>` |

`SessionClock::project` and `session_started_at()` return `UnixTimestamp`. Elapsed session time is
`recorded_at.duration_since_epoch().saturating_sub(session_started_at.duration_since_epoch())`.

## Archival DTO (`observation` schema v1)

```rust
pub struct UnixTimestampV1 {
    pub unix_seconds: u64,
    pub nanosecond: u32,
}
```

Deserialization rejects `nanosecond >= 1_000_000_000`. Ordering compares
`(unix_seconds, nanosecond)`. Live ↔ DTO conversion is lossless via `Duration::new`.

Manifest:

```json
{
  "schema_version": 1,
  "run_id": "...",
  "created_at": { "unix_seconds": 1752812073, "nanosecond": 0 },
  "session_started_at": { "unix_seconds": 1752812070, "nanosecond": 120000000 },
  "vehicle": { "identity": "..." },
  "scenario": null,
  "streams": {
    "diagnostic": "diagnostic.jsonl",
    "ledger": "ledger.jsonl"
  }
}
```

Every stream row carries `recorded_at` as the same object shape. Diagnostic and ledger payloads
carry `session_started_at`, which must equal the manifest value. Mismatches are hard errors.

## Dashboard capture startup

```text
parse args
→ install Twin
→ require boot diagnostic (timeout → no run directory)
→ RunMetadata { created_at = now, session_started_at = boot.session_started_at }
→ RunWriter::create
→ print Observation run path
→ persist boot through the normal diagnostic handler
→ set up terminal / UI loop
```

Persist-before-display, final drain, terminal restoration, and finish-error precedence remain
unchanged.

## Out of scope

- Changing `SessionClock`'s one-shot wall-clock capture semantics
- Schema v2 / dual-format readers
- Replacing summary presentation with numeric-only output
- Phase 4 golden regression tooling

## Acceptance

- All wall-time fields on live records use `UnixTimestamp`.
- Golden fixtures and round-trips store only numeric timestamp objects.
- Invalid `nanosecond` values fail with a timestamp-specific error.
- Manifest and every row agree on `session_started_at`.
- Dashboard creates no run directory when boot times out.
- `cargo test -p common`, `-p observation`, `-p tui_dashboard`, and `--workspace` pass.
- Default-path `vcan0` smoke still produces readable artifacts and a successful summary.
