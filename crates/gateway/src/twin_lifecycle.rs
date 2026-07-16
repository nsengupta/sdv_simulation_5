//! Twin install / start / stop / disband lifecycle.
//!
//! Dashboard-owned operator flow; gateway may opt into auto-start for CI.
//! Design: `DESIGN.md` §16. Implementation checklist: `docs/TODO-twin-lifecycle.md`.

use std::time::Duration;

use anyhow::Result;
use common::facade::VehicleController;

/// High-level lifecycle phase for UI and shutdown coordination.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TwinLifecyclePhase {
    /// Actor tree installed; `SessionClock` running; FSM `Off`; Start not yet sent.
    Installed,
    /// `PowerOn` sent; FSM in powered operation (`PreparingToStart` … `Off` not yet reached on stop).
    Started,
    /// `PowerOff` sent; waiting for `PreparingToStop` → `Off` and disband.
    Stopping,
    /// Brain torn down; session ended.
    Disbanded,
}

impl TwinLifecyclePhase {
    pub fn may_send_power_on(self) -> bool {
        matches!(self, Self::Installed)
    }

    pub fn may_send_power_off(self) -> bool {
        matches!(self, Self::Started)
    }

    pub fn is_operational(self) -> bool {
        matches!(self, Self::Started | Self::Stopping)
    }
}

/// Tracks Dashboard lifecycle phase (compile-ready shell; wired in TL-3+).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TwinLifecycleCoordinator {
    phase: TwinLifecyclePhase,
}

impl TwinLifecycleCoordinator {
    pub fn after_install() -> Self {
        Self {
            phase: TwinLifecyclePhase::Installed,
        }
    }

    pub fn phase(&self) -> TwinLifecyclePhase {
        self.phase
    }

    pub fn mark_started(&mut self) {
        debug_assert!(self.phase.may_send_power_on());
        self.phase = TwinLifecyclePhase::Started;
    }

    pub fn mark_stopping(&mut self) {
        debug_assert!(self.phase.may_send_power_off());
        self.phase = TwinLifecyclePhase::Stopping;
    }

    pub fn mark_disbanded(&mut self) {
        self.phase = TwinLifecyclePhase::Disbanded;
    }
}

impl Default for TwinLifecycleCoordinator {
    fn default() -> Self {
        Self::after_install()
    }
}

/// Quit path: ensure Stop + disband before process exit (TL-6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShutdownCoordinator {
    stop_timeout: Duration,
}

impl ShutdownCoordinator {
    pub fn new(stop_timeout: Duration) -> Self {
        Self { stop_timeout }
    }

    /// If lifecycle not disbanded, run Stop then tear down. Stub: returns `Ok` until TL-6.
    pub async fn ensure_stopped_before_exit(
        &self,
        _lifecycle: &mut TwinLifecycleCoordinator,
        _controller: &VehicleController,
    ) -> Result<()> {
        let _ = self.stop_timeout;
        // TL-6: send_power_off when Idle, await FsmState::Off, stop actor + ingress.
        Ok(())
    }
}

impl Default for ShutdownCoordinator {
    fn default() -> Self {
        Self::new(Duration::from_secs(30))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_then_start_transitions_phase() {
        let mut lc = TwinLifecycleCoordinator::after_install();
        assert!(lc.phase().may_send_power_on());
        lc.mark_started();
        assert_eq!(lc.phase(), TwinLifecyclePhase::Started);
        assert!(lc.phase().may_send_power_off());
    }
}
