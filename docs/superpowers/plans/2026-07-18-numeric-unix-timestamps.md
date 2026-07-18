# Numeric Unix Timestamps Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Standardize all Twin-authored wall times on a semantic Rust `UnixTimestamp` and persist schema-v1 timestamps as exact numeric `{ unix_seconds, nanosecond }` objects.

**Architecture:** `common` owns the live semantic timestamp type backed by `Duration` since the Unix Epoch. `observation::schema::v1` owns the portable numeric DTO and converts live timestamps at the L6 boundary. Dashboard requires the Twin boot diagnostic before creating a run so the manifest contains both capture creation time and the Twin-authored session start.

**Tech Stack:** Rust stable, `std::time`, Serde/serde_json, `time` for presentation formatting, Tokio channels/timeouts, existing observation golden fixtures.

## Global Constraints

- Preserve monotonic event projection: `SessionClock` reads wall time once and projects later `Instant` values from that anchor.
- Do not serialize `std::time::Duration` directly.
- Schema-v1 timestamp JSON is exactly `{ "unix_seconds": u64, "nanosecond": u32 }`.
- Reject `nanosecond >= 1_000_000_000` during deserialization.
- Keep RFC3339 only as presentation output in summary/UI.
- Revise schema v1 in place because it is uncommitted and unreleased; do not add schema v2.
- Preserve persist-before-display behavior and writer error propagation.
- Require the boot diagnostic before creating a run; timeout creates no run directory.
- Do not stage or commit changes unless the user separately requests it.

---

### Task 1: Introduce the live `UnixTimestamp` type

**Files:**
- Modify: `crates/common/src/observation_records/transition/mod.rs`
- Modify: `crates/common/src/observation_records/diagnostic/mod.rs`
- Modify: `crates/common/src/facade.rs`
- Modify: `crates/common/src/test/observation_streams_contract.rs`
- Modify: other focused `common` tests constructing affected records

**Interfaces:**
- Produces: `UnixTimestamp::from_duration_since_epoch(Duration) -> Self`
- Produces: `UnixTimestamp::duration_since_epoch(self) -> Duration`
- Produces: `UnixTimestamp::unix_seconds(self) -> u64`
- Produces: `UnixTimestamp::nanosecond(self) -> u32`
- Produces: `SessionClock::session_started_at() -> UnixTimestamp`
- Produces: `SessionClock::project(&Instant) -> UnixTimestamp`

- [ ] **Step 1: Write failing unit and contract tests**

Add focused assertions proving exact split/reconstruction, ordering, one shared session timestamp across diagnostic/ledger records, and elapsed-time subtraction:

```rust
let value = UnixTimestamp::from_duration_since_epoch(Duration::new(1_752_724_801, 120_000_000));
assert_eq!(value.unix_seconds(), 1_752_724_801);
assert_eq!(value.nanosecond(), 120_000_000);
assert_eq!(
    value.duration_since_epoch(),
    Duration::new(1_752_724_801, 120_000_000)
);
```

- [ ] **Step 2: Run focused tests and verify failure**

Run:

```bash
cargo test -p common unix_timestamp
cargo test -p common -- test::observation_streams_contract
```

Expected: FAIL because `UnixTimestamp` and the renamed fields do not exist.

- [ ] **Step 3: Implement the semantic type and replace inconsistent fields**

Use a private representation and explicit methods:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UnixTimestamp(Duration);

impl UnixTimestamp {
    pub fn from_duration_since_epoch(value: Duration) -> Self { Self(value) }
    pub fn duration_since_epoch(self) -> Duration { self.0 }
    pub fn unix_seconds(self) -> u64 { self.0.as_secs() }
    pub fn nanosecond(self) -> u32 { self.0.subsec_nanos() }
}
```

Replace `session_start_unix_nanos: u128`, `recorded_at_unix: Duration`, and nested projected wall-time `Duration` fields with consistently named `UnixTimestamp` fields. Keep elapsed calculations saturating by subtracting the two wrapped durations.

- [ ] **Step 4: Run common tests**

Run:

```bash
cargo fmt -p common
cargo test -p common
```

Expected: PASS.

---

### Task 2: Replace schema-v1 timestamp strings with numeric DTOs

**Files:**
- Modify: `crates/observation/src/schema/v1.rs`
- Modify: `crates/observation/src/error.rs`
- Modify: `crates/observation/src/summary.rs`
- Modify: `crates/observation/src/lib.rs`
- Modify: `crates/observation/tests/schema_compatibility.rs`
- Modify: `crates/observation/tests/round_trip.rs`
- Modify: `crates/observation/tests/summary.rs`
- Modify: `crates/observation/tests/support/mod.rs`

**Interfaces:**
- Consumes: `common::facade::UnixTimestamp`
- Produces: `UnixTimestampV1 { unix_seconds: u64, nanosecond: u32 }`
- Produces: `UnixTimestampV1::from_live(UnixTimestamp) -> Self`
- Produces: `UnixTimestampV1::to_live(&self) -> UnixTimestamp`
- Produces: RFC3339 presentation conversion for summary output
- Changes: `RunMetadata` contains `created_at` and `session_started_at`

- [ ] **Step 1: Write failing schema tests**

Cover:

```rust
assert_eq!(
    serde_json::to_value(timestamp).unwrap(),
    serde_json::json!({"unix_seconds": 1_752_724_801_u64, "nanosecond": 120_000_000_u32})
);
```

Also assert:

- `nanosecond == 999_999_999` is accepted;
- `nanosecond == 1_000_000_000` is rejected with a timestamp-specific error;
- serialization contains numeric JSON values, not strings;
- ordering crosses a second boundary correctly;
- live → DTO → live round-trip is exact.

- [ ] **Step 2: Run focused tests and verify failure**

Run:

```bash
cargo test -p observation --test schema_compatibility
```

Expected: FAIL against the current string-backed `Timestamp`.

- [ ] **Step 3: Implement `UnixTimestampV1` and metadata changes**

Define:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct UnixTimestampV1 {
    pub unix_seconds: u64,
    pub nanosecond: u32,
}
```

Use custom `Deserialize` (or a validated raw helper) to reject invalid subsecond values. Replace every schema timestamp field, including manifest, envelope, payload session start, warning entry time, and headlamp ACK pending time. Remove string parsing from canonical storage; retain `time` conversion only for summary presentation.

- [ ] **Step 4: Add strict manifest/session consistency tests**

Assert that every diagnostic and ledger payload `session_started_at` equals the manifest value. Reject rows whose session timestamp differs even when run ID and vehicle identity match.

- [ ] **Step 5: Run observation tests**

Run:

```bash
cargo fmt -p observation
cargo test -p observation
```

Expected: PASS except golden/CLI expectations deliberately updated in Task 4.

---

### Task 3: Require boot before creating Dashboard capture

**Files:**
- Modify: `crates/tui_dashboard/src/main.rs`
- Modify: focused tests in `crates/tui_dashboard/src/main.rs`

**Interfaces:**
- Consumes: first `DiagnosticRecord` from `DigitalTwinRuntime::diagnostic_rx`
- Produces: `RunMetadata` with capture `created_at` and boot-authored `session_started_at`
- Preserves: the consumed boot record is persisted exactly once before terminal setup

- [ ] **Step 1: Write failing lifecycle tests**

Add no-TTY tests proving:

- a boot record creates metadata with the same session timestamp;
- that same boot record is the first persisted diagnostic;
- boot timeout/error occurs before `RunWriter::create`;
- boot timeout leaves the chosen observation parent without a run directory;
- writer and terminal finalization error precedence remains unchanged.

- [ ] **Step 2: Run Dashboard tests and verify failure**

Run:

```bash
cargo test -p tui_dashboard
```

Expected: FAIL because capture is currently created before awaiting boot and boot timeout is optional.

- [ ] **Step 3: Implement boot-first initialization**

Restructure startup as:

```text
parse args
install Twin
require boot diagnostic
build RunMetadata(now, boot.session_started_at)
create RunWriter
print run path
persist boot through normal handler
set up terminal and run UI
```

Return a contextual startup error on timeout before creating the writer. Do not add a second boot-specific serialization path.

- [ ] **Step 4: Run Dashboard tests**

Run:

```bash
cargo fmt -p tui_dashboard
cargo test -p tui_dashboard
```

Expected: PASS.

---

### Task 4: Regenerate fixtures, documentation, and verify the complete feature

**Files:**
- Modify: `crates/observation/testdata/golden/v1/**`
- Modify: `crates/observation/tests/expected/summary.txt`
- Modify: `README.md`
- Modify: `docs/ARCHITECTURE-OVERVIEW.md`
- Modify: `docs/PHASES.md`
- Modify: `.superpowers/sdd/progress.md`

**Interfaces:**
- Consumes: completed live and schema timestamp behavior
- Produces: byte-stable numeric schema-v1 fixtures and accurate operator documentation

- [ ] **Step 1: Run golden and summary tests red**

Run:

```bash
cargo test -p observation --test golden_files
cargo test -p observation --test summary_cli
```

Expected: FAIL with old RFC3339 fixture bytes or expected output.

- [ ] **Step 2: Regenerate expected artifacts deliberately**

Update all timestamp objects by semantic value, not blind textual replacement. Keep RFC3339 in `observation-summary` output and document JSON numeric fields as whole Unix seconds plus subsecond nanoseconds.

- [ ] **Step 3: Run package verification**

Run:

```bash
cargo fmt -p common -- --check
cargo fmt -p observation -- --check
cargo fmt -p tui_dashboard -- --check
cargo clippy -p common -p observation -p tui_dashboard --all-targets --no-deps -- -D warnings
cargo test -p common
cargo test -p observation
cargo test -p tui_dashboard
```

Expected: all commands PASS. If pre-existing `common` warnings prevent the exact strict command, separate existing warnings from introduced warnings and do not claim a clean strict gate.

- [ ] **Step 4: Run full workspace verification**

Run:

```bash
cargo test --workspace
```

Expected: PASS.

- [ ] **Step 5: Repeat the real default-path vcan smoke**

With `vcan0` live, start both actuators, Dashboard, and the finite 30-reading emulator. Quit Dashboard and verify:

```bash
test -s "$RUN_DIR/manifest.json"
test -s "$RUN_DIR/diagnostic.jsonl"
test -s "$RUN_DIR/ledger.jsonl"
cargo run -p observation --bin observation-summary -- "$RUN_DIR"
```

Expected: all artifacts are non-empty, manifest/rows contain numeric timestamp objects, summary succeeds, and final ledger sequence/state matches the run.

- [ ] **Step 6: Record exact evidence**

Update `.superpowers/sdd/progress.md` with command results, artifact run ID, counts, final state, and any explicitly pre-existing warnings. Keep Phase 4 Not started.
