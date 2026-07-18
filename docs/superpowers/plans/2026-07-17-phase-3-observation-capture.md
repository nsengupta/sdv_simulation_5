# Phase 3 Observation Capture Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** Persist every diagnostic and transition-ledger record consumed by the transitional
Dashboard as a versioned, human-readable run artifact with deterministic readers, golden tests,
and a summary CLI.

**Architecture:** Add an L6 `observation` persistence-adapter crate that depends only on
`common::facade`. The Twin continues sending typed Rust structs over its existing MPSC channels;
the Dashboard receiver persists each borrowed record before moving it into latest-frame state.
Artifacts use `manifest.json`, `diagnostic.jsonl`, and `ledger.jsonl` beneath
`./observations/<uuid>/`.

**Tech Stack:** Rust 2024 for `observation`, Serde/serde_json, UUID v4, `time` RFC3339 formatting,
standard filesystem and buffered I/O, Tokio MPSC already used by `tui_dashboard`, Ratatui.

## Global Constraints

- Follow `docs/superpowers/specs/2026-07-17-phase-3-observation-capture-design.md`.
- `observation` is L6 and imports live record types only through `common::facade`.
- `common` must not depend on `observation`.
- Schema version 1 uses explicit archival DTOs; never serialize `common` records directly.
- Persist a record successfully before updating Dashboard state.
- Capture is enabled by default at `./observations`; `--observation-dir` overrides the parent.
- Flush every complete JSONL row before returning from the writer call.
- Do not introduce `tokio::broadcast`, an asynchronous generic tee, Dashboard replay, CSV
  scenarios, process splitting, Zenoh/uProtocol, or Phase 4 CAN orchestration.
- Record the future `tokio::select!` Dashboard loop as a TODO; do not implement it in Phase 3.
- Do not create git commits unless the user explicitly requests commits during execution.

---

## File map

### New crate

- `crates/observation/Cargo.toml` — package, library, binary, and dependencies.
- `crates/observation/src/lib.rs` — stable public API.
- `crates/observation/src/error.rs` — contextual persistence and compatibility failures.
- `crates/observation/src/schema/mod.rs` — schema dispatch and current-version constant.
- `crates/observation/src/schema/v1.rs` — manifest, envelopes, archival DTOs, timestamp and live
  record projections.
- `crates/observation/src/writer.rs` — non-overwriting run creation and durable append.
- `crates/observation/src/reader.rs` — manifest validation, lazy JSONL iteration, and full load.
- `crates/observation/src/summary.rs` — deterministic run summary model and formatting.
- `crates/observation/src/bin/observation-summary.rs` — thin command-line composition root.

### Tests and fixtures

- `crates/observation/tests/support/mod.rs` — fixed IDs, timestamps, and live sample records.
- `crates/observation/tests/round_trip.rs` — writer, streaming reader, and load contracts.
- `crates/observation/tests/schema_compatibility.rs` — version/corruption/mismatch contracts.
- `crates/observation/tests/golden_files.rs` — byte-for-byte file stability.
- `crates/observation/tests/summary_cli.rs` — CLI output contract.
- `crates/observation/tests/expected/summary.txt` — exact deterministic summary output.
- `crates/observation/testdata/golden/v1/00000000-0000-4000-8000-000000000001/` — committed
  manifest and two streams.

### Existing code and documentation

- `Cargo.toml`, `Cargo.lock` — workspace and resolved dependencies.
- `crates/common/src/facade.rs` — complete observation projection exports.
- `crates/tui_dashboard/Cargo.toml` — observation dependency.
- `crates/tui_dashboard/src/cli.rs` — strict Dashboard argument parser.
- `crates/tui_dashboard/src/main.rs` — writer composition and receiver-side capture handlers.
- `docs/design-notes-pyramid-layers.md` — canonical L0-L6 boundary.
- `README.md`, `docs/ARCHITECTURE-OVERVIEW.md`, `docs/PHASES.md`,
  `docs/TODO-simulation-5.md` — operation, status, and deferred-loop documentation.

---

### Task 1: Establish the L6 crate and facade boundary

**Files:**
- Modify: `Cargo.toml`
- Modify: `crates/common/src/facade.rs`
- Create: `crates/observation/Cargo.toml`
- Create: `crates/observation/src/lib.rs`
- Create: `crates/observation/src/error.rs`
- Create: `docs/design-notes-pyramid-layers.md`

**Interfaces:**
- Consumes: observation record types currently defined under `common::observation_records`.
- Produces: all live conversion inputs under `common::facade`; empty module seams for schema,
  writer, reader, and summary.

- [ ] **Step 1: Add a compile-time facade-boundary test**

Create `crates/observation/src/lib.rs` first with this failing import test:

```rust
#[cfg(test)]
mod tests {
    use common::facade::{
        DiagnosticLevel, DiagnosticRecord, PublishedDomainAction,
        PublishedFrontHeadlampIncompleteCause, PublishedFrontHeadlampSwitchDirection,
        PublishedFsmEvent, PublishedFsmState, PublishedOperational, PublishedTransitionRecord,
    };

    #[test]
    fn all_archival_inputs_are_available_through_the_facade() {
        fn accepts<T>() {}
        accepts::<DiagnosticLevel>();
        accepts::<DiagnosticRecord>();
        accepts::<PublishedDomainAction>();
        accepts::<PublishedFrontHeadlampIncompleteCause>();
        accepts::<PublishedFrontHeadlampSwitchDirection>();
        accepts::<PublishedFsmEvent>();
        accepts::<PublishedFsmState>();
        accepts::<PublishedOperational>();
        accepts::<PublishedTransitionRecord>();
    }
}
```

- [ ] **Step 2: Register the crate and add dependencies**

Add `"crates/observation"` to workspace members. Create the package and use Cargo to add current
compatible dependency versions rather than typing guessed versions:

```bash
mkdir -p crates/observation/src
cargo add --package observation common --path ../common
cargo add --package observation serde --features derive
cargo add --package observation serde_json
cargo add --package observation thiserror
cargo add --package observation uuid --features v4,serde
cargo add --package observation time --features formatting,parsing,macros
cargo add --package observation --dev tempfile
```

The initial manifest must declare edition 2024 and the summary binary:

```toml
[package]
name = "observation"
version = "0.1.0"
edition = "2024"

[[bin]]
name = "observation-summary"
path = "src/bin/observation-summary.rs"
```

- [ ] **Step 3: Run the boundary test and verify it fails**

Run:

```bash
cargo test -p observation all_archival_inputs_are_available_through_the_facade
```

Expected: compilation fails because diagnostic and supporting published enums are not all
available from `common::facade`.

- [ ] **Step 4: Complete the facade exports**

Extend the observation section in `crates/common/src/facade.rs`:

```rust
pub use crate::observation_records::diagnostic::{DiagnosticLevel, DiagnosticRecord};
pub use crate::observation_records::diagnostic::sink::spawn_stdout_diagnostic_observer;
pub use crate::observation_records::transition::{
    PublishedDomainAction, PublishedFrontHeadlampIncompleteCause,
    PublishedFrontHeadlampSwitchDirection, PublishedFsmEvent, PublishedFsmState,
    PublishedHeadlampContext, PublishedHeadlampState, PublishedHealthContext,
    PublishedOperational, PublishedPowertrainContext, PublishedTransitionRecord,
    PublishedVehicleContext, PublishedVisibilityContext, PublishedWheelRpm,
};
```

- [ ] **Step 5: Create the error vocabulary**

Replace the crate root with:

```rust
mod error;
pub use error::ObservationError;

#[cfg(test)]
mod tests {
    use common::facade::{
        DiagnosticLevel, DiagnosticRecord, PublishedDomainAction,
        PublishedFrontHeadlampIncompleteCause, PublishedFrontHeadlampSwitchDirection,
        PublishedFsmEvent, PublishedFsmState, PublishedOperational, PublishedTransitionRecord,
    };

    #[test]
    fn all_archival_inputs_are_available_through_the_facade() {
        fn accepts<T>() {}
        accepts::<DiagnosticLevel>();
        accepts::<DiagnosticRecord>();
        accepts::<PublishedDomainAction>();
        accepts::<PublishedFrontHeadlampIncompleteCause>();
        accepts::<PublishedFrontHeadlampSwitchDirection>();
        accepts::<PublishedFsmEvent>();
        accepts::<PublishedFsmState>();
        accepts::<PublishedOperational>();
        accepts::<PublishedTransitionRecord>();
    }
}
```

Create `error.rs`:

```rust
use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum ObservationError {
    #[error("observation I/O failed at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("observation JSON failed at {path}: {source}")]
    Json {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("invalid observation timestamp {value:?}: {reason}")]
    InvalidTimestamp { value: String, reason: String },
    #[error("unsupported observation schema version {found}; supported version is {supported}")]
    UnsupportedSchema { found: u32, supported: u32 },
    #[error("{stream} line {line}: {message}")]
    InvalidRecord {
        stream: PathBuf,
        line: usize,
        message: String,
    },
    #[error("invalid observation manifest: {message}")]
    InvalidManifest { message: String },
    #[error("run directory already exists: {0}")]
    RunAlreadyExists(PathBuf),
    #[error("record run ID {found} does not match manifest run ID {expected}")]
    RunIdMismatch { expected: String, found: String },
    #[error("record vehicle {found:?} does not match manifest vehicle {expected:?}")]
    VehicleMismatch { expected: String, found: String },
}
```

- [ ] **Step 6: Write the canonical pyramid document**

Create `docs/design-notes-pyramid-layers.md` with the exact L0-L6 list from the approved design,
the allowed arrow `observation -> common::facade`, and the prohibited arrow
`common -X-> observation`. State that application crates may depend on multiple L6 sibling
adapters, but no L6 adapter may create a cycle.

- [ ] **Step 7: Verify the boundary**

Run:

```bash
cargo test -p observation all_archival_inputs_are_available_through_the_facade
cargo check -p common
```

Expected: both pass.

---

### Task 2: Define schema v1 and lossless live-record projections

**Files:**
- Create: `crates/observation/src/schema/mod.rs`
- Create: `crates/observation/src/schema/v1.rs`
- Create: `crates/observation/tests/support/mod.rs`
- Create: `crates/observation/tests/round_trip.rs`

**Interfaces:**
- Consumes: `DiagnosticRecord`, `PublishedTransitionRecord`, and nested facade types.
- Produces: `ManifestV1`, `StreamEnvelopeV1<T>`, `DiagnosticPayloadV1`, `LedgerPayloadV1`, and
  validated RFC3339 `Timestamp`.

- [ ] **Step 1: Write failing timestamp and projection tests**

Add fixed samples in `tests/support/mod.rs`:

```rust
use std::time::Duration;

use common::facade::{
    DiagnosticLevel, DiagnosticRecord, PublishedDomainAction, PublishedFsmEvent,
    PublishedFsmState, PublishedHeadlampContext, PublishedHeadlampState,
    PublishedHealthContext, PublishedPowertrainContext, PublishedTransitionRecord,
    PublishedVehicleContext, PublishedVisibilityContext, PublishedWheelRpm,
};

pub const RUN_ID: &str = "00000000-0000-4000-8000-000000000001";
pub const CREATED_AT: &str = "2026-07-17T04:00:00.000000000Z";
pub const VEHICLE: &str = "test-vehicle";
pub const SESSION_NANOS: u128 = 1_784_260_800_000_000_000;

pub fn fixed_run_metadata() -> observation::RunMetadata {
    observation::RunMetadata::new(
        observation::RunId::parse(RUN_ID).unwrap(),
        observation::Timestamp::parse(CREATED_AT).unwrap(),
        VEHICLE,
        None,
    )
}

pub fn sample_diagnostic() -> DiagnosticRecord {
    DiagnosticRecord {
        level: DiagnosticLevel::Warning,
        source: "VirtualCarActor",
        message: "fixed warning".into(),
        session_start_unix_nanos: SESSION_NANOS,
        recorded_at_unix: Duration::from_nanos((SESSION_NANOS + 1_000_000_000) as u64),
    }
}

pub fn sample_ledger() -> PublishedTransitionRecord {
    let context = PublishedVehicleContext {
        powertrain: PublishedPowertrainContext {
            wheel_rpm: PublishedWheelRpm {
                front_left: 1,
                front_right: 2,
                rear_left: 3,
                rear_right: 4,
            },
            speed_kph: 42,
        },
        health: PublishedHealthContext {
            fuel_level_pct: 75,
            oil_pressure_kpa: 90,
            tyre_pressure_ok: true,
        },
        visibility: PublishedVisibilityContext { ambient_lux: 20 },
        headlamp: PublishedHeadlampContext {
            state: PublishedHeadlampState::OnRequested,
            ack_pending_since_at_unix: Some(Duration::from_nanos(
                (SESSION_NANOS + 750_000_000) as u64,
            )),
        },
    };

    PublishedTransitionRecord {
        car_identity: VEHICLE.into(),
        session_start_unix_nanos: SESSION_NANOS,
        record_seq: 7,
        recorded_at_unix: Duration::from_nanos((SESSION_NANOS + 2_000_000_000) as u64),
        event: PublishedFsmEvent::UpdateRpm(1500),
        old_state: PublishedFsmState::ExtremeOperationWarning {
            entered_at_unix: Duration::from_nanos(
                (SESSION_NANOS + 500_000_000) as u64,
            ),
        },
        next_state: PublishedFsmState::Driving,
        old_ctx: context,
        current_ctx: context,
        actions: vec![PublishedDomainAction::LogWarning("fixed warning".into())],
    }
}
```

In `round_trip.rs`, assert:

```rust
#[test]
fn timestamp_round_trips_at_nanosecond_precision() {
    let timestamp = Timestamp::parse(CREATED_AT).unwrap();
    assert_eq!(timestamp.as_str(), CREATED_AT);
    assert_eq!(
        Timestamp::from_unix_nanos(1_784_260_800_000_000_000)
            .unwrap()
            .as_str(),
        CREATED_AT
    );
}

#[test]
fn invalid_timestamp_is_rejected() {
    assert!(Timestamp::parse("not-a-timestamp").is_err());
}

#[test]
fn diagnostic_projection_uses_snake_case_level_and_explicit_timestamp() {
    let entry = diagnostic_envelope(&fixed_run_metadata(), &sample_diagnostic()).unwrap();
    let json = serde_json::to_value(entry).unwrap();
    assert_eq!(json["payload"]["level"], "warning");
    assert_eq!(json["vehicle_identity"], VEHICLE);
    assert!(json["recorded_at"].as_str().unwrap().ends_with('Z'));
}
```

Add this ledger projection test:

```rust
#[test]
fn ledger_projection_is_lossless_and_explicitly_tagged() {
    let entry = ledger_envelope(&fixed_run_metadata(), &sample_ledger()).unwrap();
    let json = serde_json::to_value(entry).unwrap();
    assert_eq!(json["payload"]["record_seq"], 7);
    assert_eq!(json["payload"]["event"]["type"], "update_rpm");
    assert_eq!(json["payload"]["event"]["rpm"], 1500);
    assert_eq!(
        json["payload"]["old_state"]["type"],
        "extreme_operation_warning"
    );
    assert_eq!(json["payload"]["next_state"]["type"], "driving");
    assert_eq!(json["payload"]["old_ctx"]["powertrain"]["speed_kph"], 42);
    assert_eq!(json["payload"]["current_ctx"]["visibility"]["ambient_lux"], 20);
    assert!(json["payload"]["current_ctx"]["headlamp"]["ack_pending_since"]
        .as_str()
        .unwrap()
        .ends_with('Z'));
    assert_eq!(json["payload"]["actions"][0]["type"], "log_warning");
    assert_eq!(json["payload"]["actions"][0]["message"], "fixed warning");
}
```

- [ ] **Step 2: Run tests and verify schema symbols are missing**

Run:

```bash
cargo test -p observation --test round_trip
```

Expected: compilation fails for missing schema-v1 types and projection functions.

- [ ] **Step 3: Implement validated timestamps and run metadata**

`schema/mod.rs`:

```rust
pub const CURRENT_SCHEMA_VERSION: u32 = 1;
pub mod v1;
```

Expose the completed schema from `lib.rs`:

```rust
mod error;
pub mod schema;

pub use error::ObservationError;
pub use schema::v1::{RunId, RunMetadata, ScenarioMetadata, Timestamp};
```

In `schema/v1.rs`, define:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RunId(Uuid);

impl RunId {
    pub fn new_v4() -> Self { Self(Uuid::new_v4()) }
    pub fn parse(value: &str) -> Result<Self, uuid::Error> {
        Uuid::parse_str(value).map(Self)
    }
}

impl std::fmt::Display for RunId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct Timestamp(String);

impl Timestamp {
    pub fn parse(value: &str) -> Result<Self, ObservationError>;
    pub fn from_unix_nanos(nanos: u128) -> Result<Self, ObservationError>;
    pub fn from_unix_duration(value: Duration) -> Result<Self, ObservationError>;
    pub fn as_str(&self) -> &str;
}
```

`Timestamp::parse` must parse with `time::OffsetDateTime::parse`, then immediately reformat to the
canonical fixed-width representation rather than retaining the caller's spelling. Implement
`Deserialize` manually through `Timestamp::parse`. Format using this nanosecond UTC description:

```rust
time::macros::format_description!(
    "[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:9]Z"
)
```

Define:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunMetadata {
    pub run_id: RunId,
    pub created_at: Timestamp,
    pub vehicle_identity: String,
    pub scenario: Option<ScenarioMetadata>,
}

impl RunMetadata {
    pub fn new(
        run_id: RunId,
        created_at: Timestamp,
        vehicle_identity: impl Into<String>,
        scenario: Option<ScenarioMetadata>,
    ) -> Self;

    pub fn now(
        run_id: RunId,
        vehicle_identity: impl Into<String>,
        scenario: Option<ScenarioMetadata>,
    ) -> Result<Self, ObservationError>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScenarioMetadata {
    pub name: String,
    pub source: Option<String>,
    pub sha256: Option<String>,
}
```

- [ ] **Step 4: Implement the complete v1 DTO vocabulary**

Define `ManifestV1`, `VehicleV1`, `StreamsV1`, and generic `StreamEnvelopeV1<T>` in declared field
order:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestV1 {
    pub schema_version: u32,
    pub run_id: RunId,
    pub created_at: Timestamp,
    pub vehicle: VehicleV1,
    pub scenario: Option<ScenarioMetadata>,
    pub streams: StreamsV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VehicleV1 {
    pub identity: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamsV1 {
    pub diagnostic: String,
    pub ledger: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamEnvelopeV1<T> {
    pub schema_version: u32,
    pub run_id: RunId,
    pub vehicle_identity: String,
    pub recorded_at: Timestamp,
    pub payload: T,
}
```

Define explicit archival mirrors with `#[serde(rename_all = "snake_case")]` and internally tagged
event/state/action enums:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FsmEventV1 {
    PowerOn,
    PowerOff,
    UpdateRpm { rpm: u16 },
    UpdateAmbientLux { lux: u16 },
    FrontHeadlampOnAck,
    FrontHeadlampOffAck,
    FrontHeadlampActuationIncomplete {
        direction: FrontHeadlampSwitchDirectionV1,
        cause: FrontHeadlampIncompleteCauseV1,
    },
    TimerTick,
    Internal { operational: OperationalV1 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FsmStateV1 {
    Off,
    PreparingToStart,
    Idle,
    Driving,
    DrivingDangerously,
    ExtremeOperationWarning { entered_at: Timestamp },
    PreparingToStop,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DomainActionV1 {
    StartBuzzer,
    StopBuzzer,
    PublishStateSync,
    LogWarning { message: String },
    RequestFrontHeadlampOn,
    RequestFrontHeadlampOff,
    RequestWiperStart,
    RequestWiperStop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticLevelV1 { Info, Action, Alert, Warning, Error }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticPayloadV1 {
    pub level: DiagnosticLevelV1,
    pub source: String,
    pub message: String,
    pub session_started_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LedgerPayloadV1 {
    pub session_started_at: Timestamp,
    pub record_seq: u64,
    pub event: FsmEventV1,
    pub old_state: FsmStateV1,
    pub next_state: FsmStateV1,
    pub old_ctx: VehicleContextV1,
    pub current_ctx: VehicleContextV1,
    pub actions: Vec<DomainActionV1>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WheelRpmV1 {
    pub front_left: u16,
    pub front_right: u16,
    pub rear_left: u16,
    pub rear_right: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PowertrainContextV1 {
    pub wheel_rpm: WheelRpmV1,
    pub speed_kph: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthContextV1 {
    pub fuel_level_pct: u8,
    pub oil_pressure_kpa: u8,
    pub tyre_pressure_ok: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct VisibilityContextV1 {
    pub ambient_lux: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HeadlampStateV1 { Off, Ready, OnRequested, On, OffRequested }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeadlampContextV1 {
    pub state: HeadlampStateV1,
    pub ack_pending_since: Option<Timestamp>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VehicleContextV1 {
    pub powertrain: PowertrainContextV1,
    pub health: HealthContextV1,
    pub visibility: VisibilityContextV1,
    pub headlamp: HeadlampContextV1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FrontHeadlampSwitchDirectionV1 { On, Off }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FrontHeadlampIncompleteCauseV1 { TimedOut, NegativeAck }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationalV1 { LightingUnsafe }
```

- [ ] **Step 5: Implement exhaustive projections**

Implement:

```rust
pub fn diagnostic_envelope(
    metadata: &RunMetadata,
    record: &DiagnosticRecord,
) -> Result<StreamEnvelopeV1<DiagnosticPayloadV1>, ObservationError>;

pub fn ledger_envelope(
    metadata: &RunMetadata,
    record: &PublishedTransitionRecord,
) -> Result<StreamEnvelopeV1<LedgerPayloadV1>, ObservationError>;
```

Map every live enum variant explicitly. Do not add wildcard arms. `ledger_envelope` must return
`ObservationError::VehicleMismatch` if `record.car_identity != metadata.vehicle_identity`.
Convert `recorded_at_unix`, `session_start_unix_nanos`, warning-entry timestamps, and pending-ACK
timestamps through `Timestamp`.

- [ ] **Step 6: Run projection tests**

Run:

```bash
cargo test -p observation --test round_trip timestamp
cargo test -p observation --test round_trip projection
```

Expected: all schema tests pass and emitted JSON uses stable snake-case tags.

---

### Task 3: Implement durable writers and deterministic golden files

**Files:**
- Create: `crates/observation/src/writer.rs`
- Extend: `crates/observation/tests/round_trip.rs`
- Create: `crates/observation/tests/golden_files.rs`
- Create: `crates/observation/testdata/golden/v1/00000000-0000-4000-8000-000000000001/manifest.json`
- Create: `crates/observation/testdata/golden/v1/00000000-0000-4000-8000-000000000001/diagnostic.jsonl`
- Create: `crates/observation/testdata/golden/v1/00000000-0000-4000-8000-000000000001/ledger.jsonl`

**Interfaces:**
- Consumes: fixed `RunMetadata` and borrowed live records.
- Produces: `RunWriter::{create, run_dir, record_diagnostic, record_ledger, finish}` and canonical
  bytes.

- [ ] **Step 1: Write failing writer contracts**

Add tests that:

```rust
let temp = tempfile::tempdir().unwrap();
let metadata = fixed_run_metadata();
let mut writer = RunWriter::create(temp.path(), metadata.clone()).unwrap();
assert_eq!(writer.run_dir(), temp.path().join(RUN_ID));
writer.record_diagnostic(&sample_diagnostic()).unwrap();
writer.record_ledger(&sample_ledger()).unwrap();
writer.finish().unwrap();
assert!(temp.path().join(RUN_ID).join("manifest.json").is_file());
```

Also call `RunWriter::create` twice with the same metadata and assert the second error text contains
`run directory already exists`.

- [ ] **Step 2: Run writer tests and verify failure**

Run:

```bash
cargo test -p observation --test round_trip writer
```

Expected: fails because `RunWriter` has no implementation.

- [ ] **Step 3: Implement non-overwriting run creation**

Implement:

```rust
pub struct RunWriter {
    metadata: RunMetadata,
    run_dir: PathBuf,
    diagnostic: BufWriter<File>,
    ledger: BufWriter<File>,
}

impl RunWriter {
    pub fn create(
        parent: impl AsRef<Path>,
        metadata: RunMetadata,
    ) -> Result<Self, ObservationError>;
    pub fn run_dir(&self) -> &Path;
    pub fn record_diagnostic(
        &mut self,
        record: &DiagnosticRecord,
    ) -> Result<(), ObservationError>;
    pub fn record_ledger(
        &mut self,
        record: &PublishedTransitionRecord,
    ) -> Result<(), ObservationError>;
    pub fn finish(mut self) -> Result<(), ObservationError>;
}
```

Use `create_dir_all(parent)` followed by `create_dir(run_dir)`, mapping
`ErrorKind::AlreadyExists` to `RunAlreadyExists`. Create each file with
`OpenOptions::new().write(true).create_new(true)`.

Write manifest through `serde_json::to_writer_pretty`, then `write_all(b"\n")` and `flush()`.
Write each stream row through `serde_json::to_writer`, newline, and flush. Centralize this in:

```rust
fn write_json_line<T: Serialize>(
    writer: &mut BufWriter<File>,
    path: &Path,
    value: &T,
) -> Result<(), ObservationError>;
```

Expose it from `lib.rs` with:

```rust
mod writer;
pub use writer::RunWriter;
```

- [ ] **Step 4: Make writer tests pass**

Run:

```bash
cargo test -p observation --test round_trip writer
```

Expected: pass.

- [ ] **Step 5: Add the deterministic golden test**

The test creates a run with fixed run ID, creation timestamp, diagnostic timestamp, ledger
timestamp, session timestamp, warning-entry timestamp, and ACK-pending timestamp. After
`finish()`, compare bytes for all three filenames:

```rust
for name in ["manifest.json", "diagnostic.jsonl", "ledger.jsonl"] {
    assert_eq!(
        std::fs::read(actual.join(name)).unwrap(),
        std::fs::read(expected.join(name)).unwrap(),
        "golden mismatch in {name}"
    );
}
```

Generate the initial fixture once from these fixed inputs, inspect it for schema correctness, and
commit the resulting text files as test data. Do not regenerate fixtures from live clocks.

- [ ] **Step 6: Verify deterministic output**

Run the golden test twice:

```bash
cargo test -p observation --test golden_files
cargo test -p observation --test golden_files
```

Expected: both runs pass byte-for-byte.

---

### Task 4: Implement validated streaming and loading readers

**Files:**
- Create: `crates/observation/src/reader.rs`
- Extend: `crates/observation/tests/round_trip.rs`
- Create: `crates/observation/tests/schema_compatibility.rs`

**Interfaces:**
- Consumes: one run directory.
- Produces: validated `RunReader`, lazy diagnostic/ledger iterators, and `StoredRun`.

- [ ] **Step 1: Write failing reader round-trip tests**

Write two diagnostics and two ledger rows with the writer. Assert:

```rust
let reader = RunReader::open(run_dir).unwrap();
let diagnostics = reader.diagnostics().unwrap().collect::<Result<Vec<_>, _>>().unwrap();
let ledger = reader.ledger().unwrap().collect::<Result<Vec<_>, _>>().unwrap();
assert_eq!(diagnostics.len(), 2);
assert_eq!(ledger.len(), 2);

let stored = reader.load().unwrap();
assert_eq!(stored.diagnostics, diagnostics);
assert_eq!(stored.ledger, ledger);
assert_eq!(stored.manifest.scenario, metadata.scenario);
```

- [ ] **Step 2: Write failing compatibility tests**

Copy a valid temporary fixture, then independently mutate:

- manifest `schema_version` to `2`;
- first diagnostic row `schema_version` to `2`;
- first row `run_id`;
- first row `vehicle_identity`;
- diagnostic line 2 to malformed JSON;
- `recorded_at` to `"not-a-timestamp"`;
- a manifest stream filename to `"../diagnostic.jsonl"`;
- remove `ledger.jsonl`.

Assert exact error categories and that malformed JSON includes `diagnostic.jsonl` and `line 2`.

- [ ] **Step 3: Run reader tests and verify failure**

Run:

```bash
cargo test -p observation --test round_trip reader
cargo test -p observation --test schema_compatibility
```

Expected: fail for missing `RunReader`.

- [ ] **Step 4: Implement manifest-first schema dispatch**

Define:

```rust
pub struct RunReader {
    run_dir: PathBuf,
    manifest: ManifestV1,
}

pub struct StoredRun {
    pub manifest: ManifestV1,
    pub diagnostics: Vec<StreamEnvelopeV1<DiagnosticPayloadV1>>,
    pub ledger: Vec<StreamEnvelopeV1<LedgerPayloadV1>>,
}
```

`RunReader::open` first parses the manifest as `serde_json::Value`, extracts and checks
`schema_version`, then deserializes `ManifestV1`. Only after validation may it resolve the two
manifest stream filenames. Reject the manifest unless they are exactly `diagnostic.jsonl` and
`ledger.jsonl`; do not permit absolute paths or traversal.

- [ ] **Step 5: Implement lazy line readers**

Expose:

```rust
impl RunReader {
    pub fn manifest(&self) -> &ManifestV1;
    pub fn diagnostics(&self) -> Result<DiagnosticRecords, ObservationError>;
    pub fn ledger(&self) -> Result<LedgerRecords, ObservationError>;
    pub fn load(&self) -> Result<StoredRun, ObservationError>;
}
```

Each iterator owns `Lines<BufReader<File>>`, the expected `RunId`, vehicle identity, path, and
one-based line counter. For each line:

1. map I/O errors with path and line;
2. parse `serde_json::Value`;
3. reject any version other than 1;
4. deserialize the concrete envelope;
5. validate run ID and vehicle identity;
6. return the envelope.

Do not read the whole stream for lazy APIs.

Expose the completed reader API from `lib.rs`:

```rust
mod reader;
pub use reader::{DiagnosticRecords, LedgerRecords, RunReader, StoredRun};
```

- [ ] **Step 6: Verify readers**

Run:

```bash
cargo test -p observation --test round_trip
cargo test -p observation --test schema_compatibility
```

Expected: all reader, corruption, mismatch, and scenario round-trip tests pass.

---

### Task 5: Add the deterministic summary CLI

**Files:**
- Create: `crates/observation/src/summary.rs`
- Create: `crates/observation/src/bin/observation-summary.rs`
- Create: `crates/observation/tests/summary_cli.rs`
- Create: `crates/observation/tests/expected/summary.txt`

**Interfaces:**
- Consumes: `StoredRun`.
- Produces: `RunSummary`, deterministic plain-text output, and a one-argument CLI.

- [ ] **Step 1: Write the failing CLI contract**

Invoke the compiled binary against the committed fixture:

```rust
let output = std::process::Command::new(env!("CARGO_BIN_EXE_observation-summary"))
    .arg(golden_run_dir())
    .output()
    .unwrap();
assert!(output.status.success());
assert_eq!(
    String::from_utf8(output.stdout).unwrap(),
    include_str!("expected/summary.txt")
);
```

Also assert no argument and two arguments fail with:

```text
usage: observation-summary <run-directory>
```

Create `tests/expected/summary.txt` with the exact expected output for the fixed sample records:

```text
schema_version: 1
run_id: 00000000-0000-4000-8000-000000000001
vehicle_identity: test-vehicle
created_at: 2026-07-17T04:00:00.000000000Z
diagnostic_count: 1
ledger_count: 1
warning_count: 1
error_count: 0
first_recorded_at: 2026-07-17T04:00:01.000000000Z
last_recorded_at: 2026-07-17T04:00:02.000000000Z
initial_state: ExtremeOperationWarning
final_state: Driving
final_ledger_sequence: 7
```

- [ ] **Step 2: Run and verify failure**

Run:

```bash
cargo test -p observation --test summary_cli
```

Expected: fails because the binary and formatter are absent.

- [ ] **Step 3: Implement summary calculation**

Define:

```rust
pub struct RunSummary {
    pub schema_version: u32,
    pub run_id: RunId,
    pub vehicle_identity: String,
    pub created_at: Timestamp,
    pub diagnostic_count: usize,
    pub ledger_count: usize,
    pub warning_count: usize,
    pub error_count: usize,
    pub first_recorded_at: Option<Timestamp>,
    pub last_recorded_at: Option<Timestamp>,
    pub initial_state: Option<&'static str>,
    pub final_state: Option<&'static str>,
    pub final_ledger_sequence: Option<u64>,
}

pub fn summarize(run: &StoredRun) -> RunSummary;
```

Add `FsmStateV1::label() -> &'static str`. Count warning/error diagnostics from DTO levels.
Implement `Ord` for `Timestamp` by comparing parsed `OffsetDateTime` values, then use that ordering
to compute the time range across both streams. Do not compare arbitrary strings.

Implement `Display` in exactly the field order shown in `tests/expected/summary.txt`, with
`initial_state` taken from the first ledger row's `old_state`, `final_state` from the last row's
`next_state`, and `-` for absent values.

Expose summary calculation from `lib.rs`:

```rust
mod summary;
pub use summary::{RunSummary, summarize};
```

- [ ] **Step 4: Implement the thin binary**

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 1 {
        eprintln!("usage: observation-summary <run-directory>");
        std::process::exit(2);
    }
    let reader = observation::RunReader::open(&args[0])?;
    let run = reader.load()?;
    print!("{}", observation::summarize(&run));
    Ok(())
}
```

- [ ] **Step 5: Verify the CLI**

Run:

```bash
cargo test -p observation --test summary_cli
cargo run -p observation --bin observation-summary -- \
  crates/observation/testdata/golden/v1/00000000-0000-4000-8000-000000000001
```

Expected: test passes and command prints the exact committed summary.

---

### Task 6: Integrate default capture into the transitional Dashboard

**Files:**
- Modify: `crates/tui_dashboard/Cargo.toml`
- Create: `crates/tui_dashboard/src/cli.rs`
- Modify: `crates/tui_dashboard/src/main.rs`

**Interfaces:**
- Consumes: existing diagnostic and ledger receivers.
- Produces: strict Dashboard CLI and persist-before-state-update handlers.

- [ ] **Step 1: Write failing CLI tests**

In `cli.rs`:

```rust
#[test]
fn observation_directory_defaults_to_project_relative_observations() {
    assert_eq!(
        parse_args(std::iter::empty::<&str>()).unwrap(),
        DashboardArgs { observation_dir: PathBuf::from("observations") }
    );
}

#[test]
fn explicit_observation_directory_is_accepted() {
    assert_eq!(
        parse_args(["--observation-dir", "/tmp/runs"]).unwrap().observation_dir,
        PathBuf::from("/tmp/runs")
    );
}

#[test]
fn malformed_dashboard_arguments_are_rejected() {
    assert!(parse_args(["--observation-dir"]).is_err());
    assert!(parse_args(["--unknown"]).is_err());
    assert!(parse_args(["--observation-dir", "a", "--observation-dir", "b"]).is_err());
}
```

- [ ] **Step 2: Implement strict parsing**

Use the emulator parser's small explicit style:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DashboardArgs {
    pub observation_dir: PathBuf,
}

pub fn parse_args<I, S>(args: I) -> anyhow::Result<DashboardArgs>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let values: Vec<OsString> = args.into_iter().map(|v| v.as_ref().to_owned()).collect();
    match values.as_slice() {
        [] => Ok(DashboardArgs { observation_dir: "observations".into() }),
        [flag, path]
            if flag.as_os_str() == OsStr::new("--observation-dir") && !path.is_empty() =>
        {
            Ok(DashboardArgs { observation_dir: PathBuf::from(path) })
        }
        _ => anyhow::bail!("usage: tui_dashboard [--observation-dir <parent-directory>]"),
    }
}
```

- [ ] **Step 3: Write failing receiver-handler tests**

Add a private application-boundary trait:

```rust
trait RecordCapture {
    fn record_diagnostic(&mut self, record: &DiagnosticRecord) -> Result<()>;
    fn record_ledger(&mut self, record: &PublishedTransitionRecord) -> Result<()>;
}
```

Use a fake capture to prove:

- each handler calls capture exactly once;
- state updates only after success;
- a capture error leaves the previous latest record unchanged;
- a boot diagnostic received through `await_boot_diagnostic` is passed to `handle_diagnostic`.

- [ ] **Step 4: Implement the capture adapter and focused handlers**

Implement the trait for `RunWriter`, mapping `ObservationError` into `anyhow::Error`. Add:

```rust
#[derive(Default)]
struct DashboardState {
    latest_diagnostic: Option<DiagnosticRecord>,
    latest_transition: Option<PublishedTransitionRecord>,
}

fn handle_diagnostic(
    record: DiagnosticRecord,
    capture: &mut impl RecordCapture,
    state: &mut DashboardState,
) -> Result<()> {
    capture.record_diagnostic(&record)?;
    state.latest_diagnostic = Some(record);
    Ok(())
}

fn handle_ledger(
    record: PublishedTransitionRecord,
    capture: &mut impl RecordCapture,
    state: &mut DashboardState,
) -> Result<()> {
    capture.record_ledger(&record)?;
    state.latest_transition = Some(record);
    Ok(())
}
```

Change `drain_twin_emissions` to return `Result<()>` and call these handlers for every `try_recv`
result. Keep the existing drain/draw/50-ms key poll loop; do not add `tokio::select!`.

- [ ] **Step 5: Compose the production writer**

Add `observation = { path = "../observation" }` to Dashboard dependencies. In `main`:

```rust
let args = cli::parse_args(std::env::args_os().skip(1))?;
let mut twin = install_digital_twin().await?;
let metadata = RunMetadata::now(
    RunId::new_v4(),
    VIRTUAL_CAR_IDENTITY,
    None,
)?;
let capture = RunWriter::create(&args.observation_dir, metadata)?;
eprintln!("Observation run: {}", capture.run_dir().display());
run_dashboard(&mut twin, capture).await
```

`run_dashboard` owns the writer. Initialize empty `DashboardState`; if
`await_boot_diagnostic` returns a record, route it through `handle_diagnostic`. Pass the writer to
the drain function. After the UI loop exits and terminal mode is restored, call `finish()`.

Do not retain the existing behavior that prints and swallows `run_ui_loop` errors. Return capture
errors after restoring the terminal so the process exits unsuccessfully and clearly.

- [ ] **Step 6: Verify Dashboard integration**

Run:

```bash
cargo test -p tui_dashboard
cargo check -p tui_dashboard
```

Expected: CLI, handler, boot-capture, existing rendering, observer-only, and quit-key tests pass.

---

### Task 7: Update roadmap documentation and perform final verification

**Files:**
- Modify: `README.md`
- Modify: `docs/ARCHITECTURE-OVERVIEW.md`
- Modify: `docs/PHASES.md`
- Modify: `docs/TODO-simulation-5.md`
- Verify: all Phase 3 code and fixtures

**Interfaces:**
- Consumes: completed and verified behavior.
- Produces: accurate run instructions, architectural placement, deferred event-loop TODO, and
  Phase 3 completion evidence.

- [ ] **Step 1: Document operation**

Update `README.md` run order so the Dashboard command explains its default output:

```bash
cargo run -p tui_dashboard
# writes ./observations/<uuid>/{manifest.json,diagnostic.jsonl,ledger.jsonl}

cargo run -p tui_dashboard -- --observation-dir /tmp/sdv-runs

cargo run -p observation --bin observation-summary -- \
  observations/<run-id>
```

Document that files preserve every consumed record even when the latest-state UI visually skips
intermediate updates.

- [ ] **Step 2: Update architecture and pyramid references**

In `docs/ARCHITECTURE-OVERVIEW.md`, replace “Observation capture: None” with the implemented L6
adapter, separate files, and transitional Dashboard ownership. Link
`docs/design-notes-pyramid-layers.md` and the Phase 3 design spec.

- [ ] **Step 3: Record the deferred select loop**

Append a standalone item to `docs/TODO-simulation-5.md`:

```markdown
## Dashboard Tokio event loop

**Status:** Deferred after Phase 3.

Replace drain/draw/50-ms keyboard polling with a `tokio::select!` loop over observation arrival,
keyboard/control events, and a render interval. Preserve capture of every record while allowing
future keystroke-based driver controls. This is not part of Phase 3 observation persistence.
```

- [ ] **Step 4: Run focused formatting, lint, and tests**

Run:

```bash
cargo fmt --all -- --check
cargo clippy -p observation -p tui_dashboard --all-targets -- -D warnings
cargo test -p observation
cargo test -p tui_dashboard
```

Expected: all commands exit 0.

- [ ] **Step 5: Run full workspace verification**

Run:

```bash
cargo test --workspace
```

Expected: all workspace tests pass.

- [ ] **Step 6: Perform a real default-path smoke**

With `vcan0` and actuators available, run the documented process order, quit the Dashboard, then:

```bash
# Set this to the exact run path printed by the Dashboard at startup.
read -r -p "Observation run path: " RUN_DIR
test -f "$RUN_DIR/manifest.json"
test -f "$RUN_DIR/diagnostic.jsonl"
test -f "$RUN_DIR/ledger.jsonl"
cargo run -p observation --bin observation-summary -- "$RUN_DIR"
```

Expected: all files exist, summary succeeds, and its final ledger sequence/state match the
captured run. If `vcan0` is unavailable, report the manual smoke as pending rather than marking it
passed.

- [ ] **Step 7: Mark Phase 3 done only after evidence exists**

Update the Phase 3 checkboxes and status in `docs/PHASES.md` only after focused tests, workspace
tests, and the available smoke checks pass. Keep Phase 4 explicitly not started.
