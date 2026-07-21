# Phase 9 Live Observation Transport (UDS | Zenoh) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Operators pick an explicit live observation carrier — UDS or peer Zenoh — via mutually exclusive CLI flags on Gateway and Dashboard; same schema-v2 `LiveMessage` wire format and Gateway file tee; vehicle bus stays CAN.

**Architecture:** Keep Phase 6 `LiveSink` / `LiveSource` / `ObservationTee` contracts. Add `ZenohLiveSink` / `ZenohLiveSource` in `observation` (peer sessions, one keyexpr, NDJSON payloads). Gateway waits for first Zenoh subscriber (via publisher `matching_listener`) before twin install — same connect-gate shape as UDS accept. Composition roots select impl from CLI; no silent defaults.

**Tech Stack:** Rust, Tokio `=1.48.0`, `zenoh = "1.9"` (peer `Config::default()`), existing `observation` live framing, manual CLI parsers (not clap).

**Spec:** [`docs/superpowers/specs/2026-07-20-phase-9-zenoh-observation-design.md`](../specs/2026-07-20-phase-9-zenoh-observation-design.md)

## Global Constraints

- Live modes are **mutually exclusive** and **required** — no default UDS path, no omit-means-headless.
- Gateway: exactly one of `--uds <path>` | `--zenoh` + `--keyexpr <expr>` | `--no-live`.
- Dashboard: exactly one of `--uds <path>` | `--zenoh` + `--keyexpr <expr>` (reject `--no-live`).
- UDS paths still resolve under **`<cwd>/tmp/`** via `resolve_uds_path`.
- Zenoh day one: **peer sessions**, **one** keyexpr, multiplexed `hello` + `event` (schema v2). No `zenohd` required.
- Install gate: UDS accept **or** Zenoh first matching subscriber; shared `--connect-timeout` (default 60s).
- File archive via `ObservationTee` → `RunWriter` always when twin runs (independent of live mode).
- Vehicle bus remains **CAN**. No uProtocol. No vehicle-bus Zenoh. No Phase 8 replay. No Phase 7.
- Do not reopen Phase 5 presentation work beyond footer connection text.
- **Do not push.** Commit only when the user explicitly asks; otherwise leave a clean reviewable working tree at task boundaries.
- Every task’s requirements implicitly include this section.

## File structure

| Path | Responsibility |
|------|----------------|
| `crates/observation/Cargo.toml` | Add `zenoh = "1.9"` |
| `crates/observation/src/error.rs` | `ObservationError::Zenoh { message }` |
| `crates/observation/src/live/zenoh.rs` | `ZenohLiveSink` / `ZenohLiveSource` |
| `crates/observation/src/live/any.rs` | `AnyLiveSink` / `AnyLiveSource` enums implementing traits |
| `crates/observation/src/live/mod.rs` | Export zenoh + any |
| `crates/observation/src/lib.rs` | Re-export new types |
| `crates/observation/tests/live_zenoh_roundtrip.rs` | Peer hello + event + timeout |
| `crates/gateway/src/cli.rs` | `GatewayLiveMode`; required exclusive flags; `-h` examples |
| `crates/gateway/src/main.rs` | Select sink by mode; connect-gate; tee |
| `crates/gateway/tests/observation_tee_zenoh.rs` | Boot reaches files + Zenoh client |
| `crates/tui_dashboard/src/cli.rs` | `DashboardLiveMode`; no default; `-h` examples |
| `crates/tui_dashboard/src/main.rs` | Source by mode; footer `uds:` / `zenoh:` |
| `scripts/smoke-phase6-two-process.sh` | Update to required `--uds` (already has it) |
| `scripts/smoke-phase9-zenoh-peer.sh` | Zenoh peer smoke (optional but preferred) |
| `docs/PHASES.md`, `docs/ARCHITECTURE-OVERVIEW.md`, `README.md` | Phase 9 Done / commands when impl lands |

---

### Task 1: Gateway CLI — required exclusive live mode

**Files:**
- Modify: `crates/gateway/src/cli.rs`
- Modify: `crates/gateway/src/main.rs` (only if it still reads `args.uds` after this task — prefer finishing CLI types here and adapting `main` in Task 5; for compile green, update `main` to match new fields minimally)

**Interfaces:**
- Produces:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GatewayLiveMode {
    Uds(PathBuf),
    Zenoh { keyexpr: String },
    NoLive,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayArgs {
    pub live: GatewayLiveMode,
    pub observation_dir: PathBuf,
    pub connect_timeout: Duration,
    pub print_transitions_only: bool,
    pub trace_actuation_ingress: bool,
}
```

- Help text (on `-h` / `--help`) must include the three example pairs from the spec (UDS, Zenoh, `--no-live`).
- `--zenoh` without `--keyexpr` (or empty keyexpr) → usage error.
- Combining modes (e.g. `--uds` + `--no-live`) → usage error.
- Empty argv → usage error (no longer headless-by-default).

- [ ] **Step 1: Replace CLI unit tests** in `cli.rs` — delete `defaults_are_headless_file_capture`; add:

```rust
#[test]
fn requires_exactly_one_live_mode() {
    assert!(parse_args(std::iter::empty::<&str>()).is_err());
    assert!(parse_args(["--uds", "observation.sock", "--no-live"]).is_err());
    assert!(parse_args(["--zenoh"]).is_err());
    assert!(parse_args(["--zenoh", "--keyexpr", ""]).is_err());
}

#[test]
fn accepts_no_live() {
    let args = parse_args(["--no-live"]).unwrap();
    assert_eq!(args.live, GatewayLiveMode::NoLive);
}

#[test]
fn accepts_zenoh_with_keyexpr() {
    let args = parse_args(["--zenoh", "--keyexpr", "sdv/twin/observation"]).unwrap();
    assert_eq!(
        args.live,
        GatewayLiveMode::Zenoh {
            keyexpr: "sdv/twin/observation".into()
        }
    );
}

#[test]
fn help_flag_prints_examples() {
    let err = parse_args(["--help"]).unwrap_err();
    let text = err.to_string();
    assert!(text.contains("--uds"));
    assert!(text.contains("--zenoh"));
    assert!(text.contains("--no-live"));
    assert!(text.contains("sdv/twin/observation"));
}

#[test]
fn uds_bare_filename_resolves_under_cwd_tmp() {
    let dir = tempdir().unwrap();
    let _guard = CwdGuard::enter(dir.path());
    let args = parse_args(["--uds", "observation.sock"]).unwrap();
    assert_eq!(
        args.live,
        GatewayLiveMode::Uds(dir.path().join("tmp").join("observation.sock"))
    );
}
```

Keep `rejects_system_tmp_uds` adapted to `GatewayLiveMode::Uds`.

- [ ] **Step 2: Run tests — expect FAIL**

```bash
cargo test -p gateway --lib cli::tests
```

Expected: FAIL (types / behaviour not yet matching).

- [ ] **Step 3: Implement parser**

Parse flags into optional slots (`uds`, `zenoh`, `no_live`, `keyexpr`, …). After the loop:

```rust
let live = match (uds, zenoh, no_live) {
    (Some(path), false, false) => GatewayLiveMode::Uds(
        resolve_uds_path(Some(&path)).map_err(|e| anyhow::anyhow!(e))?,
    ),
    (None, true, false) => {
        let Some(keyexpr) = keyexpr.filter(|k| !k.is_empty()) else {
            bail!("{USAGE}");
        };
        GatewayLiveMode::Zenoh { keyexpr }
    }
    (None, false, true) => GatewayLiveMode::NoLive,
    _ => bail!("{USAGE}"),
};
```

On `-h` / `--help`, `bail!("{USAGE}")` where `USAGE` is a multi-line string with examples:

```text
usage: gateway --uds <path> | --zenoh --keyexpr <expr> | --no-live
       [--observation-dir <dir>] [--connect-timeout <secs>] ...

examples:
  cargo run -p gateway -- --uds observation.sock --connect-timeout 60
  cargo run -p gateway -- --zenoh --keyexpr sdv/twin/observation --connect-timeout 60
  cargo run -p gateway -- --no-live --observation-dir observations
```

- [ ] **Step 4: Keep `main.rs` compiling** — temporary match:

```rust
let live_sink = match &args.live {
    GatewayLiveMode::Uds(path) => { /* existing bind_and_accept */ Some(...) }
    GatewayLiveMode::NoLive => None,
    GatewayLiveMode::Zenoh { .. } => bail!("Zenoh live mode not wired yet (Task 5)"),
};
```

- [ ] **Step 5: Run tests — expect PASS**

```bash
cargo test -p gateway --lib cli::tests
```

---

### Task 2: Dashboard CLI — required exclusive live mode

**Files:**
- Modify: `crates/tui_dashboard/src/cli.rs`
- Modify: `crates/tui_dashboard/src/main.rs` (minimal compile bridge until Task 6)

**Interfaces:**
- Produces:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DashboardLiveMode {
    Uds(PathBuf),
    Zenoh { keyexpr: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DashboardArgs {
    pub live: DashboardLiveMode,
    pub connect_timeout: Duration, // optional; default 60 if you parse it; else keep boot timeouts as today
}
```

Dashboard may omit `--connect-timeout` (boot waits stay as local constants). Include `--connect-timeout` only if you want parity; **not required** by spec for Dashboard. Spec requires mode flags + `-h` examples.

- [ ] **Step 1: Replace CLI tests**

```rust
#[test]
fn requires_live_mode_no_default() {
    assert!(parse_args(std::iter::empty::<&str>()).is_err());
}

#[test]
fn rejects_no_live() {
    assert!(parse_args(["--no-live"]).is_err());
}

#[test]
fn accepts_uds() {
    let dir = tempdir().unwrap();
    let _guard = CwdGuard::enter(dir.path());
    let args = parse_args(["--uds", "observation.sock"]).unwrap();
    assert_eq!(
        args.live,
        DashboardLiveMode::Uds(dir.path().join("tmp").join("observation.sock"))
    );
}

#[test]
fn accepts_zenoh() {
    let args = parse_args(["--zenoh", "--keyexpr", "sdv/twin/observation"]).unwrap();
    assert_eq!(
        args.live,
        DashboardLiveMode::Zenoh {
            keyexpr: "sdv/twin/observation".into()
        }
    );
}

#[test]
fn help_includes_examples() {
    let err = parse_args(["-h"]).unwrap_err();
    let text = err.to_string();
    assert!(text.contains("--uds"));
    assert!(text.contains("--zenoh"));
    assert!(text.contains("sdv/twin/observation"));
}
```

Delete `default_uds_is_under_cwd_tmp`. Keep system-tmp rejection and malformed-arg tests updated.

- [ ] **Step 2: Run — expect FAIL**

```bash
cargo test -p tui_dashboard --lib cli::tests
```

- [ ] **Step 3: Implement parser** (same exclusive-mode pattern as Gateway; no `--no-live` arm).

- [ ] **Step 4: Bridge `main.rs`**

```rust
let mut source = match &args.live {
    DashboardLiveMode::Uds(path) => UdsLiveSource::connect(path).await?,
    DashboardLiveMode::Zenoh { .. } => anyhow::bail!("Zenoh live mode not wired yet (Task 6)"),
};
```

Footer still uses UDS path for the UDS arm (`ConnectionStatus::Connected { detail: path.display().to_string() }` — see Task 6 for the string shape).

- [ ] **Step 5: Run — expect PASS**

```bash
cargo test -p tui_dashboard --lib cli::tests
```

---

### Task 3: `ObservationError::Zenoh` + dependency

**Files:**
- Modify: `crates/observation/Cargo.toml`
- Modify: `crates/observation/src/error.rs`

**Interfaces:**
- Produces: `ObservationError::Zenoh { message: String }` with `#[error("zenoh: {message}")]`
- Dependency: `zenoh = "1.9"` (no optional feature unless build forces it)

- [ ] **Step 1: Add error variant**

```rust
#[error("zenoh: {message}")]
Zenoh { message: String },
```

- [ ] **Step 2: Add dependency**

```toml
zenoh = "1.9"
```

- [ ] **Step 3: Verify workspace still builds**

```bash
cargo check -p observation
```

Expected: PASS (zenoh unused until Task 4). If `zenoh` pulls an incompatible Tokio, pin / feature-gate per compiler errors — keep workspace Tokio at `=1.48.0` for other crates; do not bump Tokio past 1.50 without an explicit decision.

---

### Task 4: `ZenohLiveSink` — peer publisher + subscriber wait

**Files:**
- Create: `crates/observation/src/live/zenoh.rs`
- Modify: `crates/observation/src/live/mod.rs`
- Modify: `crates/observation/src/lib.rs`
- Create: `crates/observation/tests/live_zenoh_roundtrip.rs`

**Interfaces:**
- Produces:

```rust
pub struct ZenohLiveSink { /* session, publisher, keyexpr, maybe matching listener handle */ }

impl ZenohLiveSink {
    /// Open peer session, declare publisher on `keyexpr`, wait until matching
    /// subscribers ≥ 1 (or timeout), put `hello`, return sink ready for `emit`.
    pub async fn open_and_wait_subscriber(
        keyexpr: impl Into<String>,
        connect_timeout: Duration,
        hello: LiveMessage,
    ) -> Result<Self, ObservationError>;

    pub fn keyexpr(&self) -> &str;
}

impl LiveSink for ZenohLiveSink {
    fn emit(&mut self, message: &LiveMessage) -> Result<(), ObservationError>;
    fn finish(&mut self) -> Result<(), ObservationError>;
}
```

**Subscriber-wait algorithm (mandatory behaviour):**

```rust
let session = zenoh::open(zenoh::Config::default())
    .await
    .map_err(|e| ObservationError::Zenoh { message: e.to_string() })?;
let key = keyexpr.into();
let publisher = session
    .declare_publisher(key.clone())
    .await
    .map_err(|e| ObservationError::Zenoh { message: e.to_string() })?;

// Prefer matching_listener; fall back to polling matching_status if API differs slightly.
let mut listener = publisher
    .matching_listener()
    .await
    .map_err(|e| ObservationError::Zenoh { message: e.to_string() })?;

timeout(connect_timeout, async {
    loop {
        let status = listener.recv_async().await.map_err(|e| {
            ObservationError::Zenoh { message: e.to_string() }
        })?;
        if status.matching() {
            break Ok(());
        }
    }
})
.await
.map_err(|_| ObservationError::Zenoh {
    message: format!(
        "timed out waiting for subscriber on {key} after {connect_timeout:?}"
    ),
})??;

let line = hello.to_json_line()?;
publisher
    .put(line)
    .await
    .map_err(|e| ObservationError::Zenoh { message: e.to_string() })?;
```

`emit`: `to_json_line` + `publisher.put(...)`. On put failure after start, clear publisher/session handles and return `Ok(())` on later emits (mirror UDS silent disconnect) **or** map to `Zenoh` once then no-op — pick one and keep consistent with UDS (`emit` returns `Ok` after disconnect).

`finish`: undeclare / drop session.

- [ ] **Step 1: Write failing integration test** `live_zenoh_roundtrip.rs`:

```rust
#[tokio::test(flavor = "multi_thread")]
async fn zenoh_waits_for_subscriber_then_hello_and_event() {
    let key = format!("sdv/test/obs/{}", uuid::Uuid::new_v4());
    let hello = LiveMessage::hello("test-vehicle");

    let sink_task = tokio::spawn({
        let key = key.clone();
        let hello = hello.clone();
        async move {
            ZenohLiveSink::open_and_wait_subscriber(
                key,
                Duration::from_secs(10),
                hello,
            )
            .await
        }
    });

    tokio::time::sleep(Duration::from_millis(200)).await;
    let mut source = ZenohLiveSource::subscribe(key.clone()).await.expect("subscribe");

    let mut sink = sink_task.await.expect("join").expect("open sink");
    let first = source.recv().await.expect("recv").expect("hello msg");
    assert!(matches!(first, LiveMessage::Hello { .. }));

    // Build a minimal diagnostic event (reuse helper from live_uds_roundtrip / envelope helpers)
    let event = /* LiveMessage::diagnostic_event(...) */;
    LiveSink::emit(&mut sink, &event).unwrap();
    let second = source.recv().await.expect("recv").expect("event");
    assert!(matches!(second, LiveMessage::Event { .. }));

    LiveSink::finish(&mut sink).unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn zenoh_sink_times_out_without_subscriber() {
    let key = format!("sdv/test/obs-timeout/{}", uuid::Uuid::new_v4());
    let err = ZenohLiveSink::open_and_wait_subscriber(
        key,
        Duration::from_millis(300),
        LiveMessage::hello("test-vehicle"),
    )
    .await
    .expect_err("must timeout");
    assert!(matches!(err, ObservationError::Zenoh { .. }));
}
```

Copy the diagnostic envelope construction from `crates/observation/tests/live_uds_roundtrip.rs` rather than inventing a new shape.

- [ ] **Step 2: Run — expect FAIL** (types missing)

```bash
cargo test -p observation --test live_zenoh_roundtrip
```

- [ ] **Step 3: Implement `ZenohLiveSource` stub in same file** (enough for the roundtrip test), then full `ZenohLiveSink`. Export from `live/mod.rs` and `lib.rs`.

`ZenohLiveSource` shape for this task (finish in same file):

```rust
pub struct ZenohLiveSource { /* session, subscriber sample stream, keyexpr */ }

impl ZenohLiveSource {
    pub async fn subscribe(keyexpr: impl Into<String>) -> Result<Self, ObservationError>;
    pub async fn recv(&mut self) -> Result<Option<LiveMessage>, ObservationError>;
    pub fn keyexpr(&self) -> &str;
}

impl LiveSource for ZenohLiveSource {
    fn recv_blocking(&mut self) -> Result<Option<LiveMessage>, ObservationError> {
        tokio::runtime::Handle::current().block_on(self.recv())
    }
}
```

Payload decode: UTF-8 sample bytes → `LiveMessage::from_json_line`. EOF / undeclare → `Ok(None)`.

- [ ] **Step 4: Run — expect PASS**

```bash
cargo test -p observation --test live_zenoh_roundtrip
cargo test -p observation --test live_uds_roundtrip
```

---

### Task 5: `AnyLiveSink` + Gateway composition wiring

**Files:**
- Create: `crates/observation/src/live/any.rs`
- Modify: `crates/observation/src/live/mod.rs`, `lib.rs`
- Modify: `crates/gateway/src/main.rs`
- Create: `crates/gateway/tests/observation_tee_zenoh.rs`

**Interfaces:**
- Produces:

```rust
pub enum AnyLiveSink {
    Uds(UdsLiveSink),
    Zenoh(ZenohLiveSink),
}

impl LiveSink for AnyLiveSink {
    fn emit(&mut self, message: &LiveMessage) -> Result<(), ObservationError> {
        match self {
            Self::Uds(s) => s.emit(message),
            Self::Zenoh(s) => s.emit(message),
        }
    }
    fn finish(&mut self) -> Result<(), ObservationError> {
        match self {
            Self::Uds(s) => s.finish(),
            Self::Zenoh(s) => s.finish(),
        }
    }
}
```

(`AnyLiveSource` optional here; Dashboard can match on mode instead — add if it simplifies Task 6.)

- [ ] **Step 1: Add `AnyLiveSink` + exports**

- [ ] **Step 2: Wire `run_with_capture` in `main.rs`**

```rust
let live_sink: Option<AnyLiveSink> = match &args.live {
    GatewayLiveMode::Uds(path) => {
        eprintln!(
            "[gateway] waiting for Dashboard on {} (timeout {:?})",
            path.display(),
            args.connect_timeout
        );
        Some(AnyLiveSink::Uds(
            UdsLiveSink::bind_and_accept(
                path.clone(),
                args.connect_timeout,
                LiveMessage::hello(VIRTUAL_CAR_IDENTITY),
            )
            .await
            .context("UDS accept / hello")?,
        ))
    }
    GatewayLiveMode::Zenoh { keyexpr } => {
        eprintln!(
            "[gateway] waiting for Zenoh subscriber on {keyexpr} (timeout {:?})",
            args.connect_timeout
        );
        Some(AnyLiveSink::Zenoh(
            ZenohLiveSink::open_and_wait_subscriber(
                keyexpr.clone(),
                args.connect_timeout,
                LiveMessage::hello(VIRTUAL_CAR_IDENTITY),
            )
            .await
            .context("Zenoh subscriber wait / hello")?,
        ))
    }
    GatewayLiveMode::NoLive => None,
};

// then unchanged: install_controller → boot → RunWriter → ObservationTee::new(writer, live_sink)
```

- [ ] **Step 3: Write `observation_tee_zenoh.rs`** mirroring `observation_tee_uds.rs`:

Pattern:
1. Spawn `ZenohLiveSink::open_and_wait_subscriber(key, …)`
2. `ZenohLiveSource::subscribe(key)`
3. Recv hello
4. Install twin / boot / `ObservationTee::new(writer, Some(AnyLiveSink::Zenoh(sink)))`
5. Assert boot event on Zenoh **and** files via `RunReader`

- [ ] **Step 4: Confirm headless test still passes** (`observation_capture_headless` does not use CLI). Confirm UDS tee test still passes.

```bash
cargo test -p gateway --test observation_tee_uds
cargo test -p gateway --test observation_tee_zenoh
cargo test -p gateway --test observation_capture_headless
```

Expected: PASS

---

### Task 6: Dashboard composition + footer

**Files:**
- Modify: `crates/tui_dashboard/src/main.rs`

**Interfaces:**
- Change footer detail to a display string:

```rust
enum ConnectionStatus {
    Connected { detail: String },
    Disconnected,
}

fn format_keys_footer(connection: &ConnectionStatus) -> String {
    match connection {
        ConnectionStatus::Connected { detail } => {
            format!("Connected to twin via {detail} | Keys: 'q' quit")
        }
        ConnectionStatus::Disconnected => {
            "Disconnected from twin | Keys: 'q' quit".to_string()
        }
    }
}
```

- UDS detail: path display (as today).
- Zenoh detail: `format!("zenoh:{keyexpr}")`.

- [ ] **Step 1: Update footer unit test**

```rust
#[test]
fn keys_footer_shows_connected_and_disconnected() {
    let uds = format_keys_footer(&ConnectionStatus::Connected {
        detail: "/tmp/x".into(), // string only; real paths validated elsewhere
    });
    assert!(uds.contains("Connected to twin via /tmp/x"));
    let zenoh = format_keys_footer(&ConnectionStatus::Connected {
        detail: "zenoh:sdv/twin/observation".into(),
    });
    assert!(zenoh.contains("zenoh:sdv/twin/observation"));
    let disconnected = format_keys_footer(&ConnectionStatus::Disconnected);
    assert!(disconnected.contains("Disconnected from twin"));
}
```

- [ ] **Step 2: Genericize live forward path** (avoid duplicating apply logic):

```rust
async fn open_live_source(live: &DashboardLiveMode) -> anyhow::Result<AnyLiveSource> {
    match live {
        DashboardLiveMode::Uds(path) => Ok(AnyLiveSource::Uds(
            UdsLiveSource::connect(path).await?,
        )),
        DashboardLiveMode::Zenoh { keyexpr } => Ok(AnyLiveSource::Zenoh(
            ZenohLiveSource::subscribe(keyexpr.clone()).await?,
        )),
    }
}
```

Add `AnyLiveSource` in `observation/src/live/any.rs` with `recv` async helper **or** implement `LiveSource` only and use `recv_blocking` from async via `spawn_blocking` — prefer async `recv` methods on the enum:

```rust
impl AnyLiveSource {
    pub async fn recv(&mut self) -> Result<Option<LiveMessage>, ObservationError> {
        match self {
            Self::Uds(s) => s.recv().await,
            Self::Zenoh(s) => s.recv().await,
        }
    }
}
```

Refactor `forward_live_source` / `require_boot_from_source` to take `&mut AnyLiveSource` (or generic `S` with async recv — enum is enough).

- [ ] **Step 3: `main()` flow**

```rust
let args = cli::parse_args(...)?;
let detail = match &args.live {
    DashboardLiveMode::Uds(p) => p.display().to_string(),
    DashboardLiveMode::Zenoh { keyexpr } => format!("zenoh:{keyexpr}"),
};
let mut source = open_live_source(&args.live).await?;
// existing hello timeout + boot require + apply_diagnostic(boot) before TUI
state.connection = ConnectionStatus::Connected { detail: detail.clone() };
// spawn forward_live_source(source, ...)
```

- [ ] **Step 4: Run Dashboard unit tests + workspace**

```bash
cargo test -p tui_dashboard
cargo test -p observation --test live_zenoh_roundtrip
```

Expected: PASS

---

### Task 7: Docs, smoke script, Phase 9 Done checklist

**Files:**
- Create: `scripts/smoke-phase9-zenoh-peer.sh`
- Modify: `scripts/smoke-phase6-two-process.sh` only if flags drifted (it already passes `--uds`)
- Modify: `docs/PHASES.md` — Phase 9 Status → Done; roadmap box; G10 note
- Modify: `docs/ARCHITECTURE-OVERVIEW.md` — transitional table “Today” includes Zenoh; close G10
- Modify: `README.md` — document both live pairs + `--no-live`

- [ ] **Step 1: Smoke script** `scripts/smoke-phase9-zenoh-peer.sh`:

```bash
#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
KEY="sdv/twin/observation/smoke"
mkdir -p observations

cargo build -p gateway -p tui_dashboard -p emulator

# Start gateway (waits for subscriber)
cargo run -p gateway -- --zenoh --keyexpr "$KEY" --observation-dir observations --connect-timeout 60 &
GW_PID=$!
trap 'kill $GW_PID 2>/dev/null || true' EXIT

# Dashboard briefly: connect then quit — or a small Rust/python subscriber.
# Minimal: run a one-shot cargo test binary is heavy; prefer:
#   timeout 8 cargo run -p tui_dashboard -- --zenoh --keyexpr "$KEY" </dev/null || true
# Better for CI-less smoke: use `cargo test -p observation --test live_zenoh_roundtrip`
# and a short emulator session against a headless gateway is insufficient for Zenoh.
#
# Practical smoke:
# 1) background gateway
# 2) background `cargo run -p tui_dashboard -- --zenoh --keyexpr "$KEY"` with a helper that sends 'q'
# 3) cargo run -p emulator -- --readings 1
# 4) assert observations/* exists and gateway survived

sleep 1
# Use scripted TUI quit if available; otherwise document manual operator check and
# rely on `live_zenoh_roundtrip` + `observation_tee_zenoh` as automated proof.
```

Implement a **reliable** automated path: if driving Ratatui quit is awkward, smoke script may:

1. Start Gateway `--zenoh --keyexpr …`
2. Run a tiny inline `cargo run` / `#[tokio::main]` helper **or** reuse observation test binary pattern: spawn `ZenohLiveSource::subscribe`, read hello, sleep, exit (like Phase 6 Python UDS client).

Preferred: embed a few lines of Rust in `scripts/` is awkward — add `crates/observation/examples/zenoh_hold_subscriber.rs`:

```rust
// holds a subscriber for N seconds so Gateway can install + tee
```

Then smoke:

```bash
cargo run -p observation --example zenoh_hold_subscriber -- --keyexpr "$KEY" --hold-secs 15 &
SUB_PID=$!
cargo run -p gateway -- --zenoh --keyexpr "$KEY" --observation-dir observations --connect-timeout 60 &
GW_PID=$!
sleep 2
cargo run -p emulator -- --readings 1
kill $SUB_PID || true
test "$(find observations -mindepth 1 -maxdepth 1 -type d | wc -l)" -ge 1
```

- [ ] **Step 2: Update docs** with example commands from the design; mark Phase 9 Done; close G10.

- [ ] **Step 3: Full verification**

```bash
cargo test --workspace
```

Expected: PASS

- [ ] **Step 4: Manual acceptance (operator)** — not automated in CI:

```bash
# Terminal A
cargo run -p gateway -- --zenoh --keyexpr sdv/twin/observation --connect-timeout 60

# Terminal B
cargo run -p tui_dashboard -- --zenoh --keyexpr sdv/twin/observation

# Terminal C
cargo run -p emulator -- --readings 20
```

Confirm footer shows `zenoh:sdv/twin/observation`, boot/driver panes update, `q` leaves Gateway running with files under `observations/`.

---

## Self-review (plan vs spec)

| Spec requirement | Task |
|------------------|------|
| Approach 1 Zenoh sink/source behind traits | 4 |
| Mutually exclusive Gateway flags + `--no-live` | 1, 5 |
| Mutually exclusive Dashboard flags (no `--no-live`) | 2, 6 |
| Required `--keyexpr` with `--zenoh` | 1, 2 |
| `-h` examples | 1, 2 |
| Peer Zenoh, one keyexpr, schema-v2 lines | 4 |
| Subscriber-wait install gate + timeout | 4, 5 |
| File tee unchanged | 5 (reuse `ObservationTee`) |
| Footer shows transport | 6 |
| UDS regression | 4 (keep UDS tests), 5 |
| Zenoh timeout without subscriber | 4 |
| Docs + smoke | 7 |
| CAN / uProtocol / zenohd / replay out of scope | Global Constraints |

**Placeholder scan:** none intentional — Zenoh API snippet uses `matching_listener` / `matching()`; if 1.9 method names differ slightly, adjust to compile while preserving behaviour.

**Type consistency:** `GatewayLiveMode` / `DashboardLiveMode` / `ZenohLiveSink::open_and_wait_subscriber` / `ZenohLiveSource::subscribe` / `AnyLiveSink` used consistently across tasks.

---

## Execution handoff

Plan complete and saved to `docs/superpowers/plans/2026-07-20-phase-9-zenoh-observation.md`.

**Two execution options:**

1. **Subagent-Driven (recommended)** — fresh subagent per task, review between tasks  
2. **Inline Execution** — execute tasks in this session with checkpoints  

Which approach?
