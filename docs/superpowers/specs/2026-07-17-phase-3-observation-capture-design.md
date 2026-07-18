# Phase 3 observation capture library and human-readable files

## Goal

Store every diagnostic and transition-ledger record emitted by the Digital Twin as an
interpretable, versioned run artifact. The artifacts must support engineering inspection now and
provide a stable input for later summary tools, golden regression, and replay.

Phase 3 adds storage only. It does not add Dashboard replay, process separation, Zenoh/uProtocol,
CSV scenarios, or Phase 4 CAN orchestration.

## Architectural position

The new `observation` crate is an L6 persistence adapter:

```text
L6  applications and adapters
    - observation
    - gateway
    - tui_dashboard

L5  common::facade
L4  common::twin_runtime
L3  common::observation_records
L0-L2 domain and FSM foundation
```

Dependency direction is one-way:

```text
tui_dashboard -> observation -> common::facade
       |
       +--------> gateway -----> common::facade
```

`common` never depends on `observation`. The `observation` crate imports only the outward
observation types exposed through `common::facade`; it does not import Gateway, Dashboard, actor,
or FSM implementation modules. `common::facade` will expose any missing observation types needed
at this boundary, including `DiagnosticRecord`.

The live Twin continues to emit typed Rust records through Tokio MPSC channels. It does not know
about JSON, files, schema versions, or run IDs. The `observation` crate converts borrowed
`DiagnosticRecord` and `PublishedTransitionRecord` values into versioned archival DTOs and then
serializes those DTOs.

## Ownership

During the transitional combined-process topology, `tui_dashboard` owns
`--observation-dir`. It is the current composition root and receives both diagnostic and ledger
streams. The standalone Gateway currently selects either normal diagnostic output or
`--print-transitions-only`, so it is not the Phase 3 capture owner.

Phase 5 moves capture ownership to Gateway when Gateway becomes the sole Twin owner. That move
must reuse the Phase 3 library and file contract without changing them.

`gateway_runtime.rs` remains unchanged in Phase 3. Its caller-owned receiver model is the correct
boundary.

## Output location and run identity

The Dashboard accepts:

```text
tui_dashboard [--observation-dir <parent-directory>]
```

The default is `./observations`, relative to the process working directory. In the documented
workflow the process is launched from the project root, so the effective default is:

```text
{PROJECT_ROOT}/observations
```

Each invocation creates:

```text
<observation-dir>/<run-id>/
    manifest.json
    diagnostic.jsonl
    ledger.jsonl
```

Production run IDs are UUID v4 values. The library accepts an explicit `RunId` in `RunMetadata`;
the Dashboard supplies a generated UUID while tests and controlled tools supply fixed values.
There is no test-only environment variable or run-ID generator trait.

An existing run directory is an error. Capture never overwrites or appends to a previous run.
The Dashboard prints the created run directory.

## File layout decision

Phase 3 uses a manifest and two JSONL streams rather than one combined stream.

Separate streams preserve each channel's real order without inventing a total ordering between
independent diagnostic and ledger channels. They can be tailed, loaded, and replayed
independently. The manifest stores run-level metadata once.

`manifest.json` is pretty-printed JSON ending in one newline. JSONL records use one compact JSON
object per line with `\n` line endings. Struct field order defines canonical output for
deterministic tests.

## Schema version 1

### Manifest

```json
{
  "schema_version": 1,
  "run_id": "c8ed19cc-1da8-4ee8-9670-e79c3377c18d",
  "created_at": {
    "unix_seconds": 1752724800,
    "nanosecond": 0
  },
  "session_started_at": {
    "unix_seconds": 1752724800,
    "nanosecond": 0
  },
  "vehicle": {
    "identity": "My-Opel-Corsa-1.4-GSi"
  },
  "scenario": null,
  "streams": {
    "diagnostic": "diagnostic.jsonl",
    "ledger": "ledger.jsonl"
  }
}
```

Scenario metadata is optional and deliberately narrow:

```json
{
  "name": "controlled-shutdown",
  "source": "scenario.csv",
  "sha256": "..."
}
```

Phase 3 live capture writes `null`. Supporting metadata in the library does not add CSV parsing,
scenario execution, or hashing to Phase 3.

### Stream envelopes

Every JSONL row carries enough identity to diagnose a detached or misplaced row:

```json
{
  "schema_version": 1,
  "run_id": "c8ed19cc-1da8-4ee8-9670-e79c3377c18d",
  "vehicle_identity": "My-Opel-Corsa-1.4-GSi",
  "recorded_at": {
    "unix_seconds": 1752724801,
    "nanosecond": 120000000
  },
  "payload": {}
}
```

The reader verifies that row schema version, run ID, and vehicle identity agree with the manifest.

All wall-clock timestamps use the explicit numeric schema
`{ "unix_seconds": u64, "nanosecond": u32 }`. `unix_seconds` is whole seconds since the Unix
Epoch; `nanosecond` is the subsecond remainder and must be less than `1_000_000_000`. This split
preserves exact nanosecond precision without exceeding the exact-integer range of common JSON
consumers. Duration values that are genuinely elapsed durations remain explicit integer
nanoseconds. The file schema never relies on Serde's default representation of
`std::time::Duration`. RFC3339 is presentation output generated by tools and UI, not the canonical
stored representation.

The live `common` boundary uses one semantic `UnixTimestamp` newtype backed by `Duration` since
the Unix Epoch. Both diagnostic and ledger records use `UnixTimestamp` for `session_started_at`,
`recorded_at`, and all nested projected wall times. Raw integers are not used at the live boundary:
the timestamp type preserves units and arithmetic, while the schema-v1 projection emits the
portable numeric pair.

### Diagnostic payload

```json
{
  "level": "warning",
  "source": "VirtualCarActor",
  "message": "[REJECTED]: vehicle must be Idle before PowerOff; current state is Driving",
  "session_started_at": {
    "unix_seconds": 1752724800,
    "nanosecond": 0
  }
}
```

### Ledger payload

The ledger payload is a lossless archival projection containing:

- `record_seq`
- session start timestamp
- event
- old state and next state
- old vehicle context and current vehicle context
- published domain actions

Schema-v1 DTOs use explicit snake-case tagged enums, for example:

```json
{
  "event": {
    "type": "update_rpm",
    "rpm": 1500
  }
}
```

The storage contract does not directly serialize `common` structs. Explicit archival DTOs prevent
ordinary Rust or Serde representation changes in `common` from silently changing persisted files.
Conversions from live records to DTOs live in `observation::schema::v1`.

## Writer API and durability

The library exposes these concepts:

- `RunId`
- `RunMetadata`
- `ScenarioMetadata`
- `RunWriter`
- versioned diagnostic and ledger DTOs
- `ObservationError`

`RunWriter::create` creates the run directory and all three files without overwriting existing
paths. The manifest is written before stream records and stores both capture `created_at` and the
Twin-authored `session_started_at`. The writer provides:

- `record_diagnostic(&DiagnosticRecord)`
- `record_ledger(&PublishedTransitionRecord)`
- `finish()`

Each record is serialized completely, followed by one newline, and flushed before the call
returns. Phase 3 prioritizes trustworthy artifacts over buffered throughput; observation volume
is low. `finish()` flushes both streams and reports errors.

Filesystem creation, timestamp conversion, serialization, write, and flush failures are returned
as contextual `ObservationError` values. A capture failure terminates the Dashboard after terminal
restoration rather than continuing with an artifact that appears complete.

## Receiver-side capture adapter

Phase 3 does not replace MPSC with `tokio::broadcast` and does not add a generic asynchronous tee.
`broadcast` permits lagging consumers to lose records and is unsuitable for durable capture.

The Dashboard remains the single receiver. Its record handlers perform an explicit pipeline:

```text
receive typed record -> persist borrowed record -> update Dashboard state
                                                |
                                                +-> periodic Ratatui render
```

The application owns focused `handle_diagnostic` and `handle_ledger` functions. Capture is enabled
by default, so each runtime handler receives the active writer rather than an optional sink. Each
function:

1. receives ownership of one live record;
2. lends it to the active capture writer;
3. propagates any persistence error;
4. moves it into the latest Dashboard state.

No clone, extra channel, or extra task is required. Every drained record is persisted even when
several records arrive between screen frames and only the latest is visible to the operator.
Stored timestamps are Twin-authored record timestamps, not channel-receive or render times.

The Dashboard requires the boot diagnostic before creating the run. It obtains the Twin-authored
session timestamp from that record, captures the separate observation-creation timestamp, creates
the manifest, and then persists the same boot record through the normal diagnostic handler before
entering terminal mode. A missing boot record is a startup error and must not leave a partial run
directory.

## Deferred Dashboard event-loop refactor

The current Dashboard drains both receivers, draws the latest state, and then polls keyboard input
for up to 50 ms. It can therefore render an update later than its Twin timestamp and can visually
skip intermediate records while still capturing all of them.

A `tokio::select!` event loop is a separate TODO. It may later select over observation arrival,
keyboard/control events, and a render interval. Phase 3 does not perform that refactor because it
changes event-loop and input behavior independently of persistence. Future keystroke controls
should be introduced on that event-loop design.

## Reader API

`RunReader::open(run_directory)` reads and validates the manifest before opening stream files. It
provides:

- a lazy diagnostic iterator;
- a lazy ledger iterator;
- `load()` to collect a complete `StoredRun` for small tools and tests.

The lazy readers validate every row as it is consumed. Invalid JSON reports the stream filename
and one-based line number. Unsupported versions report both the encountered and supported
versions. Missing files, invalid timestamps, run-ID mismatches, and vehicle mismatches are
distinct contextual errors.

Readers support schema version 1 only in Phase 3. Compatibility is explicit: a future version adds
a new schema module and dispatch path rather than weakening version checks.

## Summary CLI

The `observation` package provides:

```bash
cargo run -p observation --bin observation-summary -- <run-directory>
```

It prints:

- schema version, run ID, vehicle identity, and creation timestamp;
- diagnostic and ledger counts;
- first and last record timestamps;
- warning and error diagnostic counts;
- initial and final ledger states;
- final ledger sequence.

Diffing, replay, and CAN orchestration remain out of scope.

## Expected file-level changes

### New observation crate

- `crates/observation/Cargo.toml`
- `crates/observation/src/lib.rs`
- `crates/observation/src/error.rs`
- `crates/observation/src/schema/mod.rs`
- `crates/observation/src/schema/v1.rs`
- `crates/observation/src/writer.rs`
- `crates/observation/src/reader.rs`
- `crates/observation/src/bin/observation-summary.rs`
- `crates/observation/tests/round_trip.rs`
- `crates/observation/tests/schema_compatibility.rs`
- `crates/observation/tests/golden_files.rs`
- `crates/observation/tests/summary_cli.rs`
- `crates/observation/testdata/golden/v1/<fixed-run>/`

### Existing crates

- `Cargo.toml` — add the workspace member.
- `Cargo.lock` — dependency resolution.
- `crates/common/src/facade.rs` — expose the complete outward observation input surface needed by
  the L6 adapter.
- `crates/tui_dashboard/Cargo.toml` — depend on `observation`.
- `crates/tui_dashboard/src/cli.rs` — parse the output parent with the documented default.
- `crates/tui_dashboard/src/main.rs` — create capture and route every received record through the
  focused handlers.

### Documentation

- `README.md` — document default and overridden capture commands and artifact layout.
- `docs/ARCHITECTURE-OVERVIEW.md` — record the L6 persistence adapter and Phase 3 storage format.
- `docs/PHASES.md` — update Phase 3 acceptance status after verification.
- `docs/TODO-simulation-5.md` — record the deferred `tokio::select!` Dashboard event-loop refactor.
- `docs/design-notes-pyramid-layers.md` — create the currently referenced but absent canonical
  layer document and explicitly place `observation` at L6.

## Mandatory tests

1. Write multiple diagnostics and ledger rows, stream them back, and assert semantic equality.
2. Load the same run through `load()` and assert complete equality.
3. Round-trip absent and populated optional scenario metadata.
4. Reject an unsupported manifest schema version with a clear error.
5. Reject an unsupported row schema version.
6. Reject row run-ID and vehicle-identity mismatches.
7. Report malformed JSON with stream filename and line number.
8. Reject missing stream files and invalid timestamps clearly.
   Invalid timestamps include any schema-v1 `nanosecond` value greater than or equal to
   `1_000_000_000`.
9. Refuse to overwrite an existing run directory.
10. Parse the Dashboard default, explicit `--observation-dir`, missing value, unknown option, and
    duplicate option cases.
11. Prove each Dashboard handler persists its record and updates the latest state.
12. Prove persistence failure is returned rather than silently updating the display.
13. Prove the boot diagnostic is captured.
    Also prove that capture creation requires the boot record, the manifest session timestamp
    equals that record's session timestamp, and a missing boot record creates no run directory.
14. Verify deterministic summary CLI output from a fixed fixture.
15. Compare `manifest.json`, `diagnostic.jsonl`, and `ledger.jsonl` byte-for-byte against a
    committed golden run using fixed run ID, creation timestamp, record timestamps, records, field
    ordering, and line endings.
16. Run `cargo test --workspace`.

The deterministic golden test does not use live clocks or asynchronous cross-stream ordering.
Run-ID injection alone is insufficient; every time-bearing input is fixed.

Because schema v1 has not been committed or released, this approved correction replaces its
RFC3339 timestamp strings in place rather than introducing schema v2. Golden fixtures are
regenerated and reviewed as part of the change.

## Acceptance criteria

- A default Dashboard run creates
  `{PROJECT_ROOT}/observations/<uuid>/manifest.json`, `diagnostic.jsonl`, and `ledger.jsonl` when
  launched from the project root.
- `--observation-dir <path>` changes the output parent.
- Every typed record consumed by the Dashboard is written before it updates Dashboard state.
- Engineers can read the files directly and summarize a run with `observation-summary`.
- Readers reject unsupported versions and corrupt or inconsistent artifacts clearly.
- Round-trip, compatibility, Dashboard capture, summary, deterministic golden, and full workspace
  tests pass.
- Dashboard replay, process splitting, Zenoh/uProtocol, CSV scenarios, Phase 4 CAN orchestration,
  and the `tokio::select!` event-loop refactor remain deferred.
