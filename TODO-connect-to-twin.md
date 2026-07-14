# Plan: Connect TUI Dashboard to the Live Digital Twin

## Architecture Decision

The TUI Dashboard has its own `main()`. The Gateway has its own `main()`.
They should become **independent executables** that communicate via Zenoh/UDS.

---

## Design Discussion: Builder Pattern vs. Functional Composition

The previous iteration proposed eliminating `run()` entirely and having each
`main()` call 10 independent functions directly. The user asks: why not a
**Builder pattern** that assembles the runtime, then calls `run()`?

### Option A: Pure functional composition (previous plan)

```
Gateway main()          Dashboard main()
    │                        │
    ├─ create_diag_ch()      ├─ create_diag_ch()
    ├─ create_trans_ch()     ├─ create_trans_ch()
    ├─ create_act_ch()       ├─ create_act_ch()
    ├─ make_runtime_opts()   ├─ make_runtime_opts()
    ├─ install_controller()  ├─ install_controller()
    ├─ spawn_publishers()    ├─ spawn_publishers()
    ├─ spawn_timer()         ├─ spawn_timer()
    ├─ spawn_can_reader()    ├─ spawn_can_reader()
    └─ dispatch_loop()       └─ dispatch_loop() [bg]
                                  └─ ui_loop()
```

**Pro**: Fully transparent — every step is visible at the call site.
**Con**: Each `main()` must spell out the full sequence. Repetitive.

### Option B: Builder pattern (selected)

```
Gateway main()          Dashboard main()
    │                        │
    ├─ create channels       ├─ create channels
    ├─ attach observers      ├─ (no observers)
    │                        │
    └─ builder.run()         ├─ builder.install_controller()
                              ├─ builder.spawn_runtime()
                              └─ ui_loop(diag_rx, trans_rx)
```

---

## Channel Ownership: Why callers create channels, not the builder

**Decision: The builder accepts SEND ends only. Callers create the full channel
pairs and retain the RECV ends.**

### Option 1 (rejected): Builder creates channels, caller takes receivers

```rust
let mut builder = TwinRuntimeBuilder::new()
    .with_diagnostic_channel()       // builder creates (tx, rx) internally
    .with_transition_channel(256);

let diag_rx = builder.take_diagnostic_receiver().unwrap();  // 😕
let trans_rx = builder.take_transition_receiver().unwrap(); // 😕
```

**Problems:**
- `take_*()` is an anti-pattern — building something just to rip it apart
- Footgun: if caller forgets `take_*()` before `run()`, `run()` may attach its own
  observer and the UI gets nothing
- Ambiguous semantics: does `.with_diagnostic_channel()` mean "create" or "use"?
- Builder becomes a temporary holder for receivers, violating single responsibility

### Option 2 (selected): Caller creates channels, passes senders to builder

```rust
let (diag_tx, diag_rx) = mpsc::unbounded_channel();
let (trans_tx, trans_rx) = mpsc::channel(256);
let (act_tx, act_rx) = mpsc::channel(64);          // act_rx unused in Dashboard

let mut builder = TwinRuntimeBuilder::new()
    .with_car_identity("My-Opel-Corsa-1.4-GSi")
    .with_can_interface("vcan0")
    .with_diagnostic_channel(diag_tx)               // sender only
    .with_transition_channel(trans_tx)              // sender only
    .with_actuation_channel(act_tx)                 // sender only
    .with_headlamp_policy();

builder.install_controller().await?;
let _handle = builder.spawn_runtime().await?;
run_dashboard_ui(diag_rx, trans_rx).await;          // rx already owned by caller
```

**Advantages:**
- No `take_*()` — caller owns receivers from creation, naturally
- Clear ownership: builder gets SEND ends, caller keeps RECV ends
- No ambiguity — `with_diagnostic_channel(sender)` clearly means "here's the channel to use"
- `run()` cannot accidentally consume a receiver the caller needs
- Builder is a pure "configure twin's outgoing channels" abstraction
- Caller can conditionally create channels (Gateway's `--print-transitions-only`)
- Testable: caller can pass test-doubles as senders

---

## TwinRuntimeBuilder — Public API

```rust
pub struct TwinRuntimeBuilder { .. }

impl TwinRuntimeBuilder {
    // --- Create ---
    pub fn new() -> Self;

    // --- Configuration ---
    pub fn with_car_identity(mut self, identity: impl Into<String>) -> Self;
    pub fn with_can_interface(mut self, iface: impl Into<String>) -> Self;
    pub fn with_timer_tick_logging(mut self, enabled: bool) -> Self;
    pub fn with_trace_actuation_ingress(mut self, enabled: bool) -> Self;

    // --- Channels (accept SEND ends only) ---
    pub fn with_diagnostic_channel(
        mut self,
        tx: mpsc::UnboundedSender<DiagnosticRecord>,
    ) -> Self;
    pub fn with_transition_channel(
        mut self,
        tx: mpsc::Sender<PublishedTransitionRecord>,
    ) -> Self;
    pub fn with_actuation_channel(
        mut self,
        rx: mpsc::Receiver<ActuationCommand>,
    ) -> Self;
    pub fn with_headlamp_policy(mut self) -> Self;

    // --- Observer attachments (Gateway-specific) ---
    // These spawn background tasks that consume from the RECV ends.
    // Dashboard does NOT call these — it passes receivers to the UI directly.
    pub fn with_stdout_diagnostic_observer(
        mut self,
        rx: mpsc::UnboundedReceiver<DiagnosticRecord>,
    ) -> Self;
    pub fn with_transition_log_task(
        mut self,
        rx: mpsc::Receiver<PublishedTransitionRecord>,
        color: bool,
    ) -> Self;
    pub fn with_ingress_logger(mut self) -> Self;

    // --- Lifecycle ---
    pub async fn install_controller(&mut self) -> Result<&mut Self>;
    pub async fn spawn_runtime(&mut self) -> Result<JoinHandle<()>>;
    pub async fn run(self) -> Result<()>;  // install + spawn + await dispatch
}
```

---

## How Gateway's main() uses the builder

Gateway always runs headless. It creates channels, attaches stdout observers
to the RECV ends, and passes SEND ends to the builder.

```rust
#[tokio::main]
async fn main() -> Result<()> {
    let args = parse_cli_args();

    // --- Create channels (conditionally based on CLI flags) ---
    let (diag_tx, diag_rx) = if args.print_transitions_only {
        (None, None)
    } else {
        let (tx, rx) = mpsc::unbounded_channel();
        (Some(tx), Some(rx))
    };
    let (trans_tx, trans_rx) = {
        let (tx, rx) = mpsc::channel(256);
        (Some(tx), Some(rx))
    };
    let (act_tx, act_rx) = mpsc::channel(64);

    // --- Attach stdout observers to RECV ends ---
    if let Some(rx) = diag_rx {
        spawn_stdout_diagnostic_observer(rx);
    }
    if let Some(rx) = trans_rx {
        spawn_transition_log_task(rx, stdout_is_tty());
    }

    // --- Build and run ---
    let mut builder = TwinRuntimeBuilder::new()
        .with_car_identity(VIRTUAL_CAR_IDENTITY)
        .with_can_interface("vcan0")
        .with_headlamp_policy()
        .with_timer_tick_logging(args.print_timer_tick);

    if let Some(tx) = diag_tx { builder = builder.with_diagnostic_channel(tx); }
    if let Some(tx) = trans_tx { builder = builder.with_transition_channel(tx); }
    builder = builder.with_actuation_channel(act_tx);

    builder.run().await
}
```

## How Dashboard's main() uses the builder

Dashboard always creates both diagnostic and transition channels. It passes SEND
ends to the builder and retains RECV ends for the UI.

```rust
#[tokio::main]
async fn main() -> Result<()> {
    // --- Create channels (always both) ---
    let (diag_tx, diag_rx) = mpsc::unbounded_channel();
    let (trans_tx, trans_rx) = mpsc::channel(256);
    let (act_tx, _act_rx) = mpsc::channel(64);      // no actuation listener in UI

    // --- Build ---
    let mut builder = TwinRuntimeBuilder::new()
        .with_car_identity("My-Opel-Corsa-1.4-GSi")
        .with_can_interface("vcan0")
        .with_diagnostic_channel(diag_tx)
        .with_transition_channel(trans_tx)
        .with_actuation_channel(act_tx)
        .with_headlamp_policy();

    // --- Install & spawn (UI runs concurrently) ---
    builder.install_controller().await?;
    let _runtime_handle = builder.spawn_runtime().await?;

    // --- UI owns RECV sides ---
    run_dashboard_ui(diag_rx, trans_rx).await;
}
```

---

## What `run()` does

`run()` is a convenience method for Gateway's headless mode. It combines
install + spawn + await dispatch, and assumes stdout observers have already
been attached by the caller.

```rust
pub async fn run(mut self) -> Result<()> {
    self.install_controller().await?;
    let _handle = self.spawn_runtime().await?;
    // Dispatch loop runs forever (or until controller stops)
    // The JoinHandle is stored in self, so it lives until run() returns
    _handle.await.unwrap();
    Ok(())
}
```

Dashboard does NOT call `run()` — it calls `install_controller()` and
`spawn_runtime()` separately so the UI loop can run concurrently.

---

## Step plan

| Step | What changes | File(s) |
|------|-------------|---------|
| **Step 1** | Implement `TwinRuntimeBuilder` in `gateway_runtime.rs`. Extract helpers as `pub fn`. Delete old `run()`. | `crates/gateway/src/gateway_runtime.rs` |
| **Step 2** | Create `crates/gateway/src/lib.rs` with `pub mod gateway_runtime;` | `crates/gateway/src/lib.rs` (new) |
| **Step 3** | Rewrite Gateway `main.rs` to use the builder | `crates/gateway/src/main.rs` |
| **Step 4** | Add `gateway` dependency to TUI Dashboard | `crates/tui_dashboard/Cargo.toml` |
| **Step 5** | Rewrite Dashboard `main.rs` to use the builder + UI loop | `crates/tui_dashboard/src/main.rs` |
| **Step 6** | Wire left panel to `DiagnosticRecord` | `crates/tui_dashboard/src/` |
| **Step 7** | Wire right panel to `PublishedTransitionRecord` | `crates/tui_dashboard/src/` |
| **Step 8** | Remove mock data (`generate_mock_ledger()`, `LedgerTick`, `ReplayEngine`) | `crates/tui_dashboard/src/` |
| **Step 9** | (Later) Zenoh for independent Gateway/Dashboard/Emulator processes | — |
