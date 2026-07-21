//! Wiper zone (L1): alphabet + context + behavior.
//!
//! Three-state assembly, **no actuation-ACK protocol**. Transitions are immediate; the twinlet
//! always tell-backs `ZoneReady` (when not silent). Motor CMDs are fire-and-forget.
//!
//! **State transition (ASCII):**
//! ```text
//!                    BecomeOn
//!          Off ──────────────────► Ready ─────── Start ───────► Running
//!           ▲                        │  ▲                          │
//!           │                        │  └──────── Stop ────────────┘
//!           │                        │              (RainsStopped)
//!           │                        │
//!           └──── BecomeOff ─────────┴────── BecomeOff ────────────┘
//!                 (any → Off)              (any → Off)
//!
//! Operational: Start / Stop emit StartWiping / StopWiping (→ CAN CMD).
//! Lifecycle:   BecomeOff jumps straight to Off from Ready or Running and does
//!              **not** emit StopWiping — same deliberate incompleteness as Headlamp
//!              BecomeOff (no RequestOff). Physical stop on shutdown is TBD for both.
//! ```
//!
//! `BecomeOn`: `Off → Ready`. `BecomeOff`: any → `Off`. `Start`/`Stop` ignored while `Off`.

// ── L1 alphabet ───────────────────────────────────────────────────────────────

/// Snapshot — what the wiper zone **IS**.
///
/// - `Off` — assembly not started; ignores `Start`/`Stop` events.
/// - `Ready` — assembly active; no rain detected.
/// - `Running` — actively wiping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WiperState {
    #[default]
    Off,
    Ready,
    Running,
}

/// Inputs — brain tells these to the wiper assembly.
///
/// - `BecomeOn` — lifecycle: start the assembly (`Off → Ready`).
/// - `BecomeOff` — lifecycle: stop the assembly (any → `Off`; no `StopWiping`).
/// - `Start` — rain detected (`Ready → Running`).
/// - `Stop` — rain ceased (`Running → Ready`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WiperMessage {
    BecomeOn,
    BecomeOff,
    Start,
    Stop,
}

/// Zone twinlet reply after one [`WiperMessage`].
#[derive(Debug, Clone, PartialEq)]
pub struct WiperZoneReply {
    pub ctx: WiperContext,
    pub outcomes: Vec<WiperOutcome>,
}

/// Zone-local egress — L4 maps to actuation / diagnostics.
///
/// - `StartWiping` — wiper motor should be activated.
/// - `StopWiping` — wiper motor should be deactivated.
/// - `LogWarning` — observability signal; routed to diagnostic sink, never actuated.
#[derive(Debug, Clone, PartialEq)]
pub enum WiperOutcome {
    StartWiping,
    StopWiping,
    /// Emitted by the synthetic unresponsive reply when the twinlet tell-back times out.
    LogWarning(String),
}

// ── Context ───────────────────────────────────────────────────────────────────

/// Wiper zone snapshot.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WiperContext {
    pub state: WiperState,
}

impl WiperContext {
    /// Pure L1 handler: snapshot in, [`WiperZoneReply`] out.
    ///
    /// Follows the same pattern as `HeadlampContext::on_receiving_message`.
    pub fn on_receiving_message(&self, msg: WiperMessage) -> WiperZoneReply {
        let mut next = self.clone();
        let outcomes = next.apply_in_place(msg);
        WiperZoneReply {
            ctx: next,
            outcomes,
        }
    }

    fn apply_in_place(&mut self, msg: WiperMessage) -> Vec<WiperOutcome> {
        match msg {
            WiperMessage::BecomeOn => {
                if self.state == WiperState::Off {
                    self.state = WiperState::Ready;
                }
                vec![]
            }
            WiperMessage::BecomeOff => {
                self.state = WiperState::Off;
                vec![]
            }
            WiperMessage::Start => match self.state {
                WiperState::Ready => {
                    self.state = WiperState::Running;
                    vec![WiperOutcome::StartWiping]
                }
                WiperState::Running | WiperState::Off => vec![],
            },
            WiperMessage::Stop => match self.state {
                WiperState::Running => {
                    self.state = WiperState::Ready;
                    vec![WiperOutcome::StopWiping]
                }
                WiperState::Ready | WiperState::Off => vec![],
            },
        }
    }
}
