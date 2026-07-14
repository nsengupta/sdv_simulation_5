//! `TwinRuntimeBuilder` — assembles the live Digital Twin runtime.
//!
//! Gateway and Dashboard both use this builder to wire up:
//!   - `VehicleController` (actor tree)
//!   - Diagnostic channel (unbounded) — caller creates, passes sender
//!   - Transition channel (bounded)   — caller creates, passes sender
//!   - Actuation channel              — created internally (CAN egress detail)
//!   - Timer tick loop
//!   - CAN reader thread
//!   - Actuation command publishers
//!   - Ingress dispatch loop
//!
//! # Lifecycle
//! 1. `TwinRuntimeBuilder::new()` — minimal defaults
//! 2. `.with_car_identity(...)`, `.with_can_interface(...)`, etc.
//! 3. `.install_controller().await` — spawns actor tree, creates actuation channel
//! 4. `.spawn_runtime()` — spawns timer, CAN reader, publishers, returns `JoinHandle`
//! 5. (Gateway) `.run()` = `install_controller()` + `spawn_runtime()` + await dispatch

use anyhow::Result;
use common::facade::{
    ActuationCommand, PhysicalCarVocabulary, PublishedTransitionRecord, VehicleController,
    VehicleControllerRuntimeOptions, VehicleEvent, VssSignal, spawn_stdout_diagnostic_observer,
};
use common::DiagnosticRecord;
use socketcan::{CanSocket, Socket};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use vehicle_device_bus::devices::front_headlamp::can::{
    decode_payload_from_can_frame, encode_command_frame,
};
use vehicle_device_bus::devices::front_headlamp::policy::{
    FrontHeadlampPolicy, FrontHeadlampPolicyDecision,
};
use vehicle_device_bus::devices::wiper::can::encode_wiper_command_frame;

use crate::ingress;
use crate::transition_log;

/// Default SocketCAN interface (matches emulator and front_headlamp_actuator).
pub const DEFAULT_CAN_INTERFACE: &str = "vcan0";

const TIMER_TICK_MS: u64 = 100;
const ACTUATION_COMMAND_CHANNEL_CAPACITY: usize = 64;
/// Bound on the off-task ingress-log queue. Logging is best-effort: a frozen console
/// (Ctrl-S / XOFF) must not stall the CAN ingress dispatch loop (which also delivers ACKs to the
/// twin), so lines are dropped once this fills.
const INGRESS_LOG_CHANNEL_CAPACITY: usize = 512;

/// Messages forwarded from the dedicated CAN reader thread into the async dispatch loop.
enum CanIngressEnvelope {
    Physical(PhysicalCarVocabulary),
    ActuationResponse {
        physical: PhysicalCarVocabulary,
        session: u16,
        sequence: u32,
    },
}

// ---------------------------------------------------------------------------
// Builder
// ---------------------------------------------------------------------------

/// Assembles and runs the live Digital Twin runtime.
///
/// Gateway creates channels, attaches stdout observers, calls [`run()`](Self::run).
/// Dashboard creates channels, calls [`install_controller()`](Self::install_controller)
/// + [`spawn_runtime()`](Self::spawn_runtime) separately, then runs its own UI loop
/// while holding the receivers.
pub struct TwinRuntimeBuilder {
    car_identity: Option<String>,
    can_interface: String,
    log_timer_tick: bool,
    trace_actuation_ingress: bool,
    diagnostic_tx: Option<mpsc::UnboundedSender<DiagnosticRecord>>,
    transition_tx: Option<mpsc::Sender<PublishedTransitionRecord>>,
    headlamp_policy: Arc<Mutex<FrontHeadlampPolicy>>,
    /// Created internally by [`install_controller`]; consumed by [`spawn_runtime`].
    actuation_cmd_rx: Option<mpsc::Receiver<ActuationCommand>>,
}

impl TwinRuntimeBuilder {
    /// Create a new builder with safe defaults.
    pub fn new() -> Self {
        Self {
            car_identity: None,
            can_interface: DEFAULT_CAN_INTERFACE.to_string(),
            log_timer_tick: false,
            trace_actuation_ingress: false,
            diagnostic_tx: None,
            transition_tx: None,
            headlamp_policy: Arc::new(Mutex::new(FrontHeadlampPolicy::default())),
            actuation_cmd_rx: None,
        }
    }

    /// Set the car identity string (e.g. `"My-Opel-Corsa-1.4-GSi"`).
    pub fn with_car_identity(mut self, identity: impl Into<String>) -> Self {
        self.car_identity = Some(identity.into());
        self
    }

    /// Set the SocketCAN interface name (default: `vcan0`).
    pub fn with_can_interface(mut self, iface: impl Into<String>) -> Self {
        self.can_interface = iface.into();
        self
    }

    /// Enable TimerTick heartbeat logging.
    pub fn with_timer_tick_logging(mut self) -> Self {
        self.log_timer_tick = true;
        self
    }

    /// Log ignored CAN ingress frames (headlamp echoes).
    pub fn with_actuation_ingress_trace(mut self) -> Self {
        self.trace_actuation_ingress = true;
        self
    }

    /// Provide a sender for diagnostic records. Caller retains the receiver.
    pub fn with_diagnostic_channel(mut self, tx: mpsc::UnboundedSender<DiagnosticRecord>) -> Self {
        self.diagnostic_tx = Some(tx);
        self
    }

    /// Provide a sender for transition records. Caller retains the receiver.
    pub fn with_transition_channel(mut self, tx: mpsc::Sender<PublishedTransitionRecord>) -> Self {
        self.transition_tx = Some(tx);
        self
    }

    /// Attach a stdout diagnostic observer for the given receiver.
    /// Convenience: wires the observer and returns the `JoinHandle`.
    pub fn with_stdout_diagnostic_observer(
        &self,
        rx: mpsc::UnboundedReceiver<DiagnosticRecord>,
    ) -> JoinHandle<()> {
        spawn_stdout_diagnostic_observer(rx)
    }

    /// Spawn a transition log task for the given receiver.
    /// Returns the `JoinHandle` so the caller can keep it alive.
    pub fn with_transition_log_task(
        &self,
        rx: mpsc::Receiver<PublishedTransitionRecord>,
        color: bool,
    ) -> JoinHandle<()> {
        transition_log::spawn_transition_log_task(rx, color)
    }

    /// Install the controller actor tree.
    ///
    /// This creates the actuation channel internally (a CAN-egress implementation detail)
    /// and spawns the `VehicleController` with all configured channels.
    ///
    /// Must be called before [`spawn_runtime()`](Self::spawn_runtime).
    pub async fn install_controller(&mut self) -> Result<(VehicleController, VehicleControllerRuntimeOptions)> {
        let identity = self
            .car_identity
            .clone()
            .ok_or_else(|| anyhow::anyhow!("car_identity must be set before install_controller"))?;

        let (actuation_cmd_tx, actuation_cmd_rx) =
            mpsc::channel(ACTUATION_COMMAND_CHANNEL_CAPACITY);

        let runtime_options = VehicleControllerRuntimeOptions {
            log_timer_tick: self.log_timer_tick,
            actuation_command_tx: Some(actuation_cmd_tx),
            diagnostic_tx: self.diagnostic_tx.clone(),
            transition_tx: self.transition_tx.clone(),
            ..VehicleControllerRuntimeOptions::default()
        };

        let (controller, _join) = VehicleController::install_and_start_with_options(
            identity,
            runtime_options.clone(),
        )
        .await
        .map_err(|e| anyhow::anyhow!("install controller: {e}"))?;

        // Store actuation receiver so spawn_runtime can find it.
        self.actuation_cmd_rx = Some(actuation_cmd_rx);

        Ok((controller, runtime_options))
    }

    /// Spawn background workers (timer tick, CAN reader, actuation publishers).
    ///
    /// Call [`install_controller()`](Self::install_controller) first.
    /// Returns a `JoinHandle` that resolves when the ingress dispatch loop exits.
    pub fn spawn_runtime(
        &mut self,
        controller: VehicleController,
    ) -> Result<JoinHandle<Result<()>>> {
        let can_interface = self.can_interface.clone();
        let headlamp_policy = self.headlamp_policy.clone();
        let trace_actuation_ingress = self.trace_actuation_ingress;
        let actuation_cmd_rx = self
            .actuation_cmd_rx
            .take()
            .ok_or_else(|| anyhow::anyhow!("install_controller must be called before spawn_runtime"))?;

        // Off-hot-path ingress logger: a frozen console must not block ACK delivery to the twin.
        let ingress_log_tx = {
            let (tx, mut rx) = mpsc::channel::<String>(INGRESS_LOG_CHANNEL_CAPACITY);
            tokio::spawn(async move {
                while let Some(line) = rx.recv().await {
                    println!("{line}");
                }
            });
            tx
        };

        // Timer tick loop
        spawn_timer_tick_loop(controller.clone());

        // Actuation command publishers (fan-out to headlamp + wiper)
        spawn_actuation_command_publishers(
            actuation_cmd_rx,
            can_interface.clone(),
            headlamp_policy.clone(),
        );

        // CAN reader thread (blocking I/O)
        let (can_tx, can_rx) = mpsc::unbounded_channel();
        spawn_can_reader_thread(
            can_interface.clone(),
            headlamp_policy,
            trace_actuation_ingress,
            can_tx,
        )?;

        // Print startup banners
        println!(
            "⚡ Gateway on {can_interface} — CAN → VehicleEvent → PhysicalCarVocabulary → VehicleController"
        );
        println!(
            "[gateway] front-headlamp + wiper CMD egress on CAN; \
             run `cargo run -p front_headlamp_actuator` and `cargo run -p wiper_actuator`"
        );

        // We need to send PowerOn asynchronously — do it in a spawned task.
        let c = controller.clone();
        tokio::spawn(async move {
            if let Err(e) = c.send_power_on().await {
                eprintln!("[gateway] PowerOn failed: {e:?}");
            }
        });

        // Spawn ingress dispatch loop and return its JoinHandle.
        let dispatch = tokio::spawn(run_can_ingress_dispatch_loop(
            controller,
            can_rx,
            ingress_log_tx,
        ));

        Ok(dispatch)
    }

    /// Convenience: install controller, spawn runtime, and await the dispatch loop.
    ///
    /// Suitable for Gateway (headless). Dashboard calls
    /// [`install_controller()`](Self::install_controller) +
    /// [`spawn_runtime()`](Self::spawn_runtime) separately.
    pub async fn run(&mut self) -> Result<()> {
        let (controller, _) = self.install_controller().await?;
        let handle = self.spawn_runtime(controller)?;
        match handle.await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => Err(e),
            Err(e) => Err(anyhow::anyhow!("dispatch loop panicked: {e:?}")),
        }
    }
}

impl Default for TwinRuntimeBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

fn spawn_timer_tick_loop(controller: VehicleController) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_millis(TIMER_TICK_MS)).await;
            let physical = ingress::vehicle_event_to_physical_vocabulary(VehicleEvent::TimerTick);
            let _ = controller.submit_physical_car_event(physical).await;
        }
    });
}

/// Dedicated OS thread for blocking `read_frame()` loop.
fn spawn_can_reader_thread(
    can_interface: String,
    front_headlamp_policy: Arc<Mutex<FrontHeadlampPolicy>>,
    trace_actuation_ingress: bool,
    tx: mpsc::UnboundedSender<CanIngressEnvelope>,
) -> Result<std::thread::JoinHandle<()>> {
    let socket = CanSocket::open(&can_interface)?;
    let thread_name = format!("gateway-can-reader-{can_interface}");
    let handle = std::thread::Builder::new()
        .name(thread_name)
        .spawn(move || {
            loop {
                let frame = match socket.read_frame() {
                    Ok(frame) => frame,
                    Err(e) => {
                        eprintln!("[gateway-can-reader]: read_frame failed: {e:?}");
                        continue;
                    }
                };
                if let Some(sig) = VssSignal::from_can_frame(&frame) {
                    if matches!(sig, VssSignal::VehicleSpeed(_)) {
                        continue;
                    }
                    let ev = VehicleEvent::TelemetryUpdate(sig);
                    let physical = ingress::vehicle_event_to_physical_vocabulary(ev);
                    if tx.send(CanIngressEnvelope::Physical(physical)).is_err() {
                        break;
                    }
                    continue;
                }
                if let Some(payload) = decode_payload_from_can_frame(&frame) {
                    let decision = {
                        let mut policy = front_headlamp_policy
                            .lock()
                            .expect("front-headlamp policy lock");
                        policy.on_response(payload)
                    };
                    match decision {
                        FrontHeadlampPolicyDecision::Accept {
                            physical,
                            session,
                            sequence,
                        } => {
                            if tx
                                .send(CanIngressEnvelope::ActuationResponse {
                                    physical,
                                    session,
                                    sequence,
                                })
                                .is_err()
                            {
                                break;
                            }
                        }
                        FrontHeadlampPolicyDecision::Ignore(reason) => {
                            if trace_actuation_ingress {
                                eprintln!(
                                    "[actuation-can-ingress trace ignored]: reason={reason} session={} seq={}",
                                    payload.session_id, payload.sequence_no
                                );
                            }
                        }
                    }
                }
            }
        })?;
    Ok(handle)
}

/// Fan-out: one actuation channel -> headlamp publisher + wiper publisher.
fn spawn_actuation_command_publishers(
    mut actuation_cmd_rx: mpsc::Receiver<ActuationCommand>,
    can_interface: String,
    front_headlamp_policy: Arc<Mutex<FrontHeadlampPolicy>>,
) {
    let (headlamp_tx, headlamp_rx) = mpsc::channel(ACTUATION_COMMAND_CHANNEL_CAPACITY);
    let (wiper_tx, wiper_rx) = mpsc::channel(ACTUATION_COMMAND_CHANNEL_CAPACITY);

    tokio::spawn(async move {
        while let Some(cmd) = actuation_cmd_rx.recv().await {
            let tx = match &cmd {
                ActuationCommand::StartWiper | ActuationCommand::StopWiper => &wiper_tx,
                _ => &headlamp_tx,
            };
            if tx.send(cmd).await.is_err() {
                break;
            }
        }
    });

    spawn_front_headlamp_command_publisher(headlamp_rx, can_interface.clone(), front_headlamp_policy);
    spawn_wiper_command_publisher(wiper_rx, can_interface);
}

/// Egress: twin actuation intent -> policy pending state -> CMD frame on CAN.
fn spawn_front_headlamp_command_publisher(
    mut actuation_cmd_rx: mpsc::Receiver<ActuationCommand>,
    can_interface: String,
    front_headlamp_policy: Arc<Mutex<FrontHeadlampPolicy>>,
) {
    tokio::spawn(async move {
        let socket = match CanSocket::open(&can_interface) {
            Ok(s) => s,
            Err(e) => {
                eprintln!(
                    "[gateway]: cannot open CAN {can_interface} for front-headlamp CMD TX: {e}"
                );
                return;
            }
        };
        while let Some(cmd) = actuation_cmd_rx.recv().await {
            {
                let mut policy = front_headlamp_policy
                    .lock()
                    .expect("front-headlamp policy lock");
                policy.on_command_sent(&cmd);
            }
            match encode_command_frame(&cmd) {
                Ok(frame) => {
                    if let Err(e) = socket.write_frame(&frame) {
                        eprintln!("[gateway]: front-headlamp CMD write_frame failed: {e:?}");
                    }
                }
                Err(e) => eprintln!("[gateway]: encode front-headlamp CMD failed: {e:?}"),
            }
        }
    });
}

/// Egress: twin wiper actuation intent -> CMD frame on CAN (fire-and-forget).
fn spawn_wiper_command_publisher(
    mut actuation_cmd_rx: mpsc::Receiver<ActuationCommand>,
    can_interface: String,
) {
    tokio::spawn(async move {
        let socket = match CanSocket::open(&can_interface) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[gateway]: cannot open CAN {can_interface} for wiper CMD TX: {e}");
                return;
            }
        };
        while let Some(cmd) = actuation_cmd_rx.recv().await {
            if !matches!(cmd, ActuationCommand::StartWiper | ActuationCommand::StopWiper) {
                continue;
            }
            match encode_wiper_command_frame(&cmd) {
                Ok(frame) => {
                    if let Err(e) = socket.write_frame(&frame) {
                        eprintln!("[gateway]: wiper CMD write_frame failed: {e:?}");
                    }
                }
                Err(e) => eprintln!("[gateway]: wiper CMD encode failed: {e}"),
            }
        }
    });
}

/// Format the wire-level ingress line for a headlamp ACK/NACK, or `None` for other events.
fn format_front_headlamp_ingress(
    session: u16,
    sequence: u32,
    physical: &PhysicalCarVocabulary,
) -> Option<String> {
    let (icon, msg) = match physical {
        PhysicalCarVocabulary::FrontHeadlampCommandConfirmed { on_command: true } => {
            ("✓", "ACK_ON")
        }
        PhysicalCarVocabulary::FrontHeadlampCommandConfirmed { on_command: false } => {
            ("✓", "ACK_OFF")
        }
        PhysicalCarVocabulary::FrontHeadlampCommandRejected { on_command: true } => {
            ("✗", "NACK_ON")
        }
        PhysicalCarVocabulary::FrontHeadlampCommandRejected { on_command: false } => {
            ("✗", "NACK_OFF")
        }
        _ => return None,
    };
    Some(format!(
        "[actuation-can-ingress session={session} seq={sequence}]: {icon} {msg}"
    ))
}

async fn run_can_ingress_dispatch_loop(
    controller: VehicleController,
    mut rx: mpsc::UnboundedReceiver<CanIngressEnvelope>,
    ingress_log_tx: mpsc::Sender<String>,
) -> Result<()> {
    while let Some(msg) = rx.recv().await {
        match msg {
            CanIngressEnvelope::Physical(physical) => {
                controller
                    .submit_physical_car_event(physical)
                    .await
                    .map_err(|e| anyhow::anyhow!("submit physical car event: {e:?}"))?;
            }
            CanIngressEnvelope::ActuationResponse {
                physical,
                session,
                sequence,
            } => {
                let line = format_front_headlamp_ingress(session, sequence, &physical);
                controller
                    .submit_physical_car_event(physical)
                    .await
                    .map_err(|e| anyhow::anyhow!("submit physical car event: {e:?}"))?;
                if let Some(line) = line {
                    let _ = ingress_log_tx.try_send(line);
                }
            }
        }
    }
    Err(anyhow::anyhow!(
        "CAN ingress channel closed: reader thread exited"
    ))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn builder_defaults_do_not_panic() {
        let builder = TwinRuntimeBuilder::new();
        assert!(builder.car_identity.is_none());
        assert_eq!(builder.can_interface, DEFAULT_CAN_INTERFACE);
    }

    #[tokio::test]
    async fn builder_accepts_channels() {
        let (diag_tx, _diag_rx) = mpsc::unbounded_channel();
        let (trans_tx, _trans_rx) = mpsc::channel(256);

        let builder = TwinRuntimeBuilder::new()
            .with_car_identity("test-car")
            .with_diagnostic_channel(diag_tx)
            .with_transition_channel(trans_tx);

        assert!(builder.diagnostic_tx.is_some());
        assert!(builder.transition_tx.is_some());
    }

    #[tokio::test]
    async fn builder_install_controller_needs_identity() {
        let mut builder = TwinRuntimeBuilder::new();
        let result = builder.install_controller().await;
        assert!(result.is_err());
        assert!(
            format!("{:?}", result).contains("car_identity"),
            "expected error about missing car_identity, got: {result:?}"
        );
    }
}
