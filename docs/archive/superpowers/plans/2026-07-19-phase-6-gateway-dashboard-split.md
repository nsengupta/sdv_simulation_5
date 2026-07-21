# Phase 6 Gateway ↔ Dashboard Process Split Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Gateway the sole Twin owner (capture tee + optional UDS live feed); make `tui_dashboard` an observation-only UDS consumer with no in-process twin.

**Architecture:** `observation` gains detachable `LiveSink` / `LiveSource` traits, schema-v2 NDJSON framing, `ObservationTee` (convert live → DTO once → `RunWriter` + sink), and UDS impls. Gateway binds under `<cwd>/tmp`, optionally connect-gates install, then tees. Dashboard connects via `--uds`, shows footer connection status, applies `DTO → live` into existing Phase 5 views. Zenoh later swaps trait impls only.

**Tech Stack:** Rust, Tokio (`net`/`io`/`sync`/`time`), existing `observation` schema v2 + `RunWriter`, `TwinRuntimeBuilder` (unchanged channel ownership), Ratatui Dashboard views.

**Spec:** [`docs/superpowers/specs/2026-07-19-phase-6-gateway-dashboard-split-design.md`](../specs/2026-07-19-phase-6-gateway-dashboard-split-design.md)

## Global Constraints

- Live link is **UDS only** (not file-tail, not TCP). Zenoh is Phase 9.
- Wire payloads are **schema v2 DTOs** inside NDJSON envelopes (`hello` + `event` only — **no** `run_started`).
- Gateway **tees** file archive + optional live sink; Dashboard does **not** write runs in live mode.
- UDS paths must resolve under **`<cwd>/tmp/`** (never system `/tmp`). Default: `./tmp/observation.sock`.
- Single live client; **no reconnect**; connect-gated install when `--uds` is set.
- Headless Gateway (no `--uds`) still installs immediately and writes `RunWriter` files.
- `TwinRuntimeBuilder` caller-owned receivers unchanged.
- Do not reopen Phase 5 presentation work (PaneLine, speed bands, Rain/Wipers gaps, etc.).
- Do not implement Phase 7 embedded emulator or Phase 8 `--replay`.
- Do not stage or commit unless the user separately requests it.
- Every task’s requirements implicitly include this section.

## File structure

| Path | Responsibility |
|------|----------------|
| `crates/observation/src/uds_path.rs` | Resolve/validate paths under `<cwd>/tmp` |
| `crates/observation/src/live/mod.rs` | `LiveMessage`, `LiveEvent`, re-exports |
| `crates/observation/src/live/traits.rs` | `LiveSink` / `LiveSource` traits |
| `crates/observation/src/live/memory.rs` | In-memory sink/source for unit tests |
| `crates/observation/src/live/uds.rs` | `UdsLiveSink` / `UdsLiveSource` |
| `crates/observation/src/tee.rs` | `ObservationTee` — convert once → file + sink |
| `crates/observation/src/schema/v1.rs` | Add `*_from_envelope` / reverse projectors (`DTO → live`) |
| `crates/observation/src/lib.rs` | Export live + tee + uds_path |
| `crates/observation/Cargo.toml` | Add `tokio` features needed for UDS |
| `crates/observation/tests/live_uds_roundtrip.rs` | Hello + events over real UDS under temp cwd/tmp |
| `crates/gateway/src/cli.rs` | `--uds`, `--observation-dir`, `--connect-timeout` |
| `crates/gateway/src/main.rs` | Bind/accept/hello, capture tee, headless path |
| `crates/gateway/Cargo.toml` | Depend on `observation` |
| `crates/gateway/tests/observation_tee_uds.rs` | Gateway-style tee: channels → files + UDS client |
| `crates/tui_dashboard/src/cli.rs` | `--uds` required; drop live `--observation-dir` |
| `crates/tui_dashboard/src/main.rs` | Remove twin install; consume `LiveSource`; footer status |
| `crates/tui_dashboard/Cargo.toml` | Drop `gateway` dep if unused |
| `scripts/smoke-phase6-two-process.sh` | Two-process smoke |
| `.gitignore` | Ignore `/tmp/` at repo root (cwd tmp sockets) |
| `docs/PHASES.md`, `docs/ARCHITECTURE-OVERVIEW.md`, `TODO-connect-to-twin.md` | Phase 6 status / G5 / stale stub |

---

### Task 1: UDS path under `<cwd>/tmp`

**Files:**
- Create: `crates/observation/src/uds_path.rs`
- Modify: `crates/observation/src/lib.rs`
- Modify: `.gitignore`

**Interfaces:**
- Produces: `pub const DEFAULT_UDS_FILE_NAME: &str = "observation.sock";`
- Produces: `pub fn resolve_uds_path(user: Option<&Path>) -> Result<PathBuf, ObservationError>`
- Produces: `pub fn ensure_tmp_parent(path: &Path) -> Result<(), ObservationError>`
- Rules: default → `std::env::current_dir()?.join("tmp").join(DEFAULT_UDS_FILE_NAME)`; bare filename → under `cwd/tmp`; absolute/relative paths must canonicalize inside `cwd/tmp` or error `ObservationError::InvalidUdsPath`.

- [ ] **Step 1: Write failing unit tests** in `uds_path.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn default_path_is_under_cwd_tmp() {
        let dir = tempdir().unwrap();
        let _guard = std::env::set_current_dir(dir.path());
        fs::create_dir_all(dir.path().join("tmp")).unwrap();
        let path = resolve_uds_path(None).unwrap();
        assert_eq!(path, dir.path().join("tmp").join("observation.sock"));
    }

    #[test]
    fn rejects_system_tmp() {
        let err = resolve_uds_path(Some(Path::new("/tmp/observation.sock"))).unwrap_err();
        assert!(matches!(err, ObservationError::InvalidUdsPath { .. }));
    }
}
```

- [ ] **Step 2: Run tests — expect FAIL** (module missing)

```bash
cargo test -p observation --lib uds_path::tests
```

- [ ] **Step 3: Implement `uds_path` + `ObservationError::InvalidUdsPath` + export from `lib.rs`**

- [ ] **Step 4: Add `/tmp/` to `.gitignore`** (repo-root cwd tmp for sockets; do not ignore unrelated paths elsewhere)

- [ ] **Step 5: Re-run tests — expect PASS**

```bash
cargo test -p observation --lib uds_path::tests
```

---

### Task 2: Live wire message types (hello + event)

**Files:**
- Create: `crates/observation/src/live/mod.rs`
- Create: `crates/observation/src/live/message.rs`

**Interfaces:**
- Produces:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LiveMessage {
    Hello {
        schema_version: u32,
        vehicle: VehicleIdentityV1, // or inline { identity: String } matching spec
    },
    Event {
        stream: LiveStream,
        record: LiveRecordDto,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LiveStream { Diagnostic, Ledger }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum LiveRecordDto {
    Diagnostic(StreamEnvelopeV1<DiagnosticPayloadV1>),
    Ledger(StreamEnvelopeV1<LedgerPayloadV1>),
}
```

Prefer a tagged `record` that matches stream (validate `stream` ↔ payload kind on decode). Keep JSON examples from the spec.

- [ ] **Step 1: Round-trip unit test** — serialize `Hello` and a diagnostic `Event` to one JSON line each; deserialize back equal.

- [ ] **Step 2: Run — expect FAIL**

```bash
cargo test -p observation --lib live::
```

- [ ] **Step 3: Implement types + `to_json_line` / `from_json_line` helpers**

- [ ] **Step 4: Run — expect PASS**

---

### Task 3: `LiveSink` / `LiveSource` traits + memory doubles

**Files:**
- Create: `crates/observation/src/live/traits.rs`
- Create: `crates/observation/src/live/memory.rs`
- Modify: `crates/observation/src/live/mod.rs`

**Interfaces:**
- Produces (async traits via `async_trait` **or** concrete async fns on structs — prefer small concrete APIs if avoiding new deps):

```rust
pub trait LiveSink: Send {
    fn emit(&mut self, message: &LiveMessage) -> Result<(), ObservationError>;
    fn finish(&mut self) -> Result<(), ObservationError>;
}

pub trait LiveSource: Send {
    fn recv_blocking(&mut self) -> Result<Option<LiveMessage>, ObservationError>;
}
```

For Tokio call sites, also provide async wrappers on UDS types in Task 6. Memory double: `MemoryLiveLink::pair() -> (MemoryLiveSink, MemoryLiveSource)` using `std::sync::mpsc` or `tokio::sync::mpsc`.

- [ ] **Step 1: Test** — sink emits hello + event; source receives same order.

- [ ] **Step 2: Run — expect FAIL**

- [ ] **Step 3: Implement traits + memory pair**

- [ ] **Step 4: Run — expect PASS**

Add `tokio` to `observation` `Cargo.toml` only if memory uses tokio channels; otherwise defer tokio to Task 6.

---

### Task 4: `DTO → live` reverse projection

**Files:**
- Modify: `crates/observation/src/schema/v1.rs`
- Modify: `crates/observation/tests/round_trip.rs` (or new `tests/live_to_from.rs`)

**Interfaces:**
- Produces: `pub fn diagnostic_from_envelope(env: &StreamEnvelopeV1<DiagnosticPayloadV1>) -> Result<DiagnosticRecord, ObservationError>`
- Produces: `pub fn ledger_from_envelope(env: &StreamEnvelopeV1<LedgerPayloadV1>) -> Result<PublishedTransitionRecord, ObservationError>`
- Note: `DiagnosticRecord.source` is `&'static str`. Implement `fn intern_source(s: &str) -> &'static str` using a process-global `Mutex<HashSet>` + `Box::leak` for unknown strings (document as Phase 6 pragmatic; twin still uses static literals).

- [ ] **Step 1: Failing test** — `sample_diagnostic` → `diagnostic_envelope` → `diagnostic_from_envelope` equals original on level/kind/timestamps (source string equal by value).

- [ ] **Step 2: Run — expect FAIL**

```bash
cargo test -p observation --test round_trip
```

(or the new test file)

- [ ] **Step 3: Implement reverse projectors for all `DiagnosticKindV1` / FSM / context variants used by schema v2**

- [ ] **Step 4: Run — expect PASS**; also `cargo test -p observation`

---

### Task 5: `ObservationTee`

**Files:**
- Create: `crates/observation/src/tee.rs`
- Modify: `crates/observation/src/lib.rs`
- Create: `crates/observation/tests/tee_file_and_sink.rs`

**Interfaces:**
- Produces:

```rust
pub struct ObservationTee<S: LiveSink> {
    writer: RunWriter,
    sink: Option<S>, // None = headless file-only
}

impl<S: LiveSink> ObservationTee<S> {
    pub fn new(writer: RunWriter, sink: Option<S>) -> Self;
    pub fn record_diagnostic(&mut self, record: &DiagnosticRecord) -> Result<(), ObservationError>;
    pub fn record_ledger(&mut self, record: &PublishedTransitionRecord) -> Result<(), ObservationError>;
    pub fn finish(self) -> Result<(), ObservationError>;
}
```

Each `record_*`: build envelope via existing `diagnostic_envelope` / `ledger_envelope` **once**; append via writer (refactor `RunWriter` to accept envelope **or** keep calling `record_*` which converts again — **prefer extract shared write-envelope path so conversion is single**). Emit `LiveMessage::Event { stream, record }` on sink when `Some`.

Minimal acceptable approach if refactor is large: call `RunWriter::record_*` (internal convert) **and** build the same envelope again for the sink in tee — document as temporary duplication only if extracting proves noisy; **prefer single convert**.

- [ ] **Step 1: Test** — tee one diagnostic + one ledger into temp `RunWriter` + `MemoryLiveSink`; assert files readable by `RunReader` and sink got two events with matching payloads.

- [ ] **Step 2: Run — expect FAIL**

- [ ] **Step 3: Implement tee (+ small `RunWriter` helper if needed)**

- [ ] **Step 4: Run — expect PASS**

---

### Task 6: UDS live sink/source

**Files:**
- Create: `crates/observation/src/live/uds.rs`
- Modify: `crates/observation/Cargo.toml` — add  
  `tokio = { version = "=1.48.0", features = ["net", "io-util", "macros", "rt", "rt-multi-thread", "sync", "time"] }`  
  (pin aligned with gateway)
- Create: `crates/observation/tests/live_uds_roundtrip.rs`

**Interfaces:**
- Produces: `UdsLiveSink::bind_and_accept(path, connect_timeout) -> Result<(Self, HelloAlreadySent), ObservationError>`  
  — creates parent `tmp`, removes stale sock, listens, accepts **one** client, writes `hello`, returns sink for further `emit`.
- Produces: `UdsLiveSource::connect(path) -> Result<Self, ObservationError>`
- Produces: async `recv(&mut self) -> Result<Option<LiveMessage>, ObservationError>` reading newline-delimited JSON
- On client disconnect: further `emit` returns `Ok(())` and becomes no-op (or distinct `Disconnected` ignored by tee)

- [ ] **Step 1: Integration test** using `tempdir` as cwd (`set_current_dir`), resolve default UDS path, spawn accept task + connect client, assert hello then echo event.

- [ ] **Step 2: Run — expect FAIL**

```bash
cargo test -p observation --test live_uds_roundtrip
```

- [ ] **Step 3: Implement UDS types**

- [ ] **Step 4: Run — expect PASS**

---

### Task 7: Gateway CLI + main wiring (capture + optional UDS)

**Files:**
- Create: `crates/gateway/src/cli.rs`
- Modify: `crates/gateway/src/main.rs`
- Modify: `crates/gateway/src/lib.rs` (export cli if tested from lib; else keep cli in binary module tree)
- Modify: `crates/gateway/Cargo.toml` — add `observation`, `serde`, `serde_json` as needed
- Create: `crates/gateway/tests/observation_capture_headless.rs` (file-only tee without UDS)

**Interfaces:**
- Produces: `GatewayArgs { uds: Option<PathBuf>, observation_dir: PathBuf, connect_timeout: Duration }`
- Default observation dir: `observations`
- Default connect timeout: `60s`
- `--uds` optional; when present, path passed through `observation::resolve_uds_path`

**Gateway main flow (with `--uds`):**
1. resolve/bind/accept/hello via `UdsLiveSink`
2. create MPSC diag/ledger channels; attach to `TwinRuntimeBuilder`
3. `install_controller`
4. wait boot on `diagnostic_rx` (timeout)
5. `RunWriter::create` + eprintln run dir
6. `ObservationTee::new(writer, Some(sink))`; record boot
7. `spawn_runtime`
8. loop `select!` both receivers → tee; on ctrl/exit finish tee

**Without `--uds`:** skip bind; tee with `sink: None` after boot; keep existing stdout observer flags working.

- [ ] **Step 1: CLI unit tests** — default headless; `--uds observation.sock` resolves under tmp; rejects `/tmp/foo.sock`

- [ ] **Step 2: Headless capture test** — install twin (existing test patterns / auto_power_on false), emit or wait boot, assert run dir has `manifest.json` + `diagnostic.jsonl`

- [ ] **Step 3: Implement CLI + main refactor**

- [ ] **Step 4:**

```bash
cargo test -p gateway
cargo test -p observation
```

Expected: PASS

---

### Task 8: Dashboard — remove in-process twin; consume `LiveSource`

**Files:**
- Modify: `crates/tui_dashboard/src/cli.rs`
- Modify: `crates/tui_dashboard/src/main.rs`
- Modify: `crates/tui_dashboard/Cargo.toml` — remove `gateway` dependency
- Modify/create tests in `tui_dashboard` for mock source → state

**Interfaces:**
- Produces: `DashboardArgs { uds: PathBuf }` — `--uds` required (default `./tmp/observation.sock` via `resolve_uds_path(None)` is OK if flag omitted **or** require explicit flag; prefer **default path** when no args so smoke is short: `tui_dashboard` ≡ `tui_dashboard --uds ./tmp/observation.sock`)
- Remove `--observation-dir` from live Dashboard
- Remove `TwinRuntimeBuilder`, `RunWriter`, `RecordCapture` persist path
- Add `ConnectionStatus { Connected { uds: PathBuf }, Disconnected }` on `DashboardState`
- Feed: `UdsLiveSource::connect` → read hello → set Connected → wait boot diagnostic event (`kind` boot) → apply-before-display loop using `diagnostic_from_envelope` / `ledger_from_envelope`
- `q` quits TUI only

- [ ] **Step 1: Rewrite CLI tests** for `--uds` / default / reject `/tmp/...`

- [ ] **Step 2: Unit test** — mock `MemoryLiveSource` with hello + boot diagnostic event → `DashboardState.latest_diagnostic` set; `connection` is Connected (extract apply helpers for testability)

- [ ] **Step 3: Implement CLI + main split** (keep `view/` modules unchanged)

- [ ] **Step 4:**

```bash
cargo test -p tui_dashboard
cargo test --workspace
```

Expected: PASS (fix any broken tests that assumed in-process twin)

---

### Task 9: Footer shows connected / disconnected

**Files:**
- Modify: `crates/tui_dashboard/src/main.rs` (keys/footer render)
- Modify: view helpers if footer is centralized

**Interfaces:**
- Bottom pane / keys footer includes connection text, e.g.  
  `Connected to twin via ./tmp/observation.sock | Keys: 'q' quit`  
  and on socket EOF:  
  `Disconnected from twin | Keys: 'q' quit`

- [ ] **Step 1: Test** — render footer string (pure fn) for Connected vs Disconnected

- [ ] **Step 2: Implement footer composition**

- [ ] **Step 3:** `cargo test -p tui_dashboard` PASS

---

### Task 10: Gateway UDS integration + two-process smoke

**Files:**
- Create: `crates/gateway/tests/observation_tee_uds.rs` (or extend Task 7)
- Create: `scripts/smoke-phase6-two-process.sh`
- Ensure script is executable

**Gateway integration test:**
- temp cwd with `tmp/` + `observations/`
- spawn: bind accept in test **or** drive `ObservationTee` + `UdsLiveSink` with a fake boot record + one ledger after connecting a client
- Prefer testing the public composition without full `vcan0` if CI lacks it; if existing gateway e2e uses vcan, follow that pattern only when available
- Assert: client received `hello` + diagnostic event; run dir has matching diagnostic line

**Smoke script (documented run order):**

```bash
#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
mkdir -p tmp observations
SOCK="$ROOT/tmp/observation.sock"
rm -f "$SOCK"
cargo build -p gateway -p tui_dashboard -p emulator
# start gateway with --uds, then dashboard, then emulator --readings 1 (or minimal)
# assert observations/* contains a run; kill dashboard; assert gateway still alive briefly
```

- [ ] **Step 1: Write gateway UDS integration test; run until PASS**

- [ ] **Step 2: Write smoke script; run manually once on a machine with `vcan0` if required — document prerequisite**

- [ ] **Step 3:** `cargo test --workspace` PASS

---

### Task 11: Docs and roadmap close-out

**Files:**
- Modify: `docs/PHASES.md` — Phase 6 status Done; check tests; note UDS under `./tmp`
- Modify: `docs/ARCHITECTURE-OVERVIEW.md` — transitional table: twin in Gateway; G5 closed; observation ownership Gateway
- Modify: `TODO-connect-to-twin.md` — point to Phase 6 Done + Zenoh Phase 9; remove stale Phase 5/8 wording
- Modify: `DESIGN.md` §16.5 “Today” row if still claiming in-process twin
- Modify: `README.md` only if it documents process run order

- [ ] **Step 1: Apply doc edits matching acceptance in the spec**

- [ ] **Step 2: Grep for stale “twin until Phase 5” / “Phase 5 process split” / Zenoh-as-Phase-8 claims; fix to Phase 6 / Phase 9**

```bash
rg -n "Phase 5.*split|TODO-connect-to-twin|Zenoh.*Phase 8|in-process.*twin" docs DESIGN.md README.md TODO-connect-to-twin.md || true
```

- [ ] **Step 3: Final verification**

```bash
cargo test --workspace
cargo fmt --check
cargo clippy -p observation -p gateway -p tui_dashboard --all-targets --no-deps -- -D warnings
```

Expected: all green.

---

## Spec coverage checklist (plan self-review)

| Spec requirement | Task |
|------------------|------|
| Gateway sole twin owner | 7, 8 |
| Dashboard no `TwinRuntimeBuilder` | 8 |
| UDS live link | 6, 7, 8 |
| Schema v2 JSONL DTOs on wire | 2, 4, 5 |
| Tee file + UDS | 5, 7 |
| Single client, no reconnect | 6, 7, 8 |
| Connect-gated install when `--uds` | 7 |
| Headless without `--uds` | 7 |
| UDS under `<cwd>/tmp` | 1, 7, 8 |
| No `run_started` | 2, 7, 8 |
| Footer connected status | 9 |
| Detachable `LiveSink`/`LiveSource` | 3, 6 |
| Convert once on Gateway | 5 |
| `DTO → live` on Dashboard | 4, 8 |
| Gateway integration test | 10 |
| Dashboard mock stream test | 8 |
| Two-process smoke | 10 |
| Docs / G5 / stub | 11 |
| Channel ownership unchanged | 7 (builder API untouched) |

## Placeholder / consistency notes

- Default socket filename is `observation.sock` everywhere.
- Tokio pin `=1.48.0` matches gateway.
- Reverse projection must cover full schema v2 vocabulary used in goldens.
- Phase 8 replay explicitly out of scope; `DTO → live` is the reusable seam.
