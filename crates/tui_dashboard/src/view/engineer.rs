use super::{MISSING, LineRole, PaneLine};
use common::facade::{
    PublishedFsmEvent, PublishedFsmState, PublishedHeadlampState, PublishedTransitionRecord,
};

pub struct EngineerPane {
    pub lines: Vec<PaneLine>,
}

pub fn engineer_pane(ledger: Option<&PublishedTransitionRecord>, width: usize) -> EngineerPane {
    if ledger.is_none() {
        return EngineerPane {
            lines: [
                "Twin installed.",
                "Waiting for PowerOn on CAN.",
                "Ledger and diagnostics appear after lifecycle starts.",
            ]
            .into_iter()
            .map(|line| PaneLine::plain_fitted(LineRole::Standby, line, width))
            .collect(),
        };
    }

    let row = ledger.expect("checked above");
    let lines = vec![
        PaneLine::plain_fitted(
            LineRole::EngineerState,
            &format!("Current state: {}", format_state(&row.next_state)),
            width,
        ),
        PaneLine::plain_fitted(
            LineRole::EngineerEvent,
            &format!("Last event: {}", format_event(&row.event)),
            width,
        ),
        // TODO(phase-5-follow-up): Twin ROB depth emission.
        PaneLine::plain_fitted(
            LineRole::EngineerRob,
            &format!("Active ROB turns: {MISSING}"),
            width,
        ),
        PaneLine::plain_fitted(LineRole::EngineerHeading, "Sub-assemblies:", width),
        PaneLine::plain_fitted(
            LineRole::EngineerAssembly,
            &format!(
                "  Headlamp: {}",
                format_headlamp(row.current_ctx.headlamp.state)
            ),
            width,
        ),
        // TODO(phase-5-follow-up): Twin wiper actor status.
        PaneLine::plain_fitted(
            LineRole::EngineerAssembly,
            &format!("  Wiper: {MISSING}"),
            width,
        ),
    ];
    EngineerPane { lines }
}

fn format_state(state: &PublishedFsmState) -> String {
    match state {
        PublishedFsmState::ExtremeOperationWarning { .. } => {
            "Extreme operation warning".to_owned()
        }
        other => format!("{other:?}"),
    }
}

fn format_event(event: &PublishedFsmEvent) -> String {
    match event {
        PublishedFsmEvent::UpdateRpm(rpm) => format!("UpdateRpm({rpm})"),
        PublishedFsmEvent::UpdateAmbientLux(lux) => format!("UpdateAmbientLux({lux})"),
        PublishedFsmEvent::FrontHeadlampActuationIncomplete { direction, cause } => {
            format!("HeadlampIncomplete({direction:?},{cause:?})")
        }
        PublishedFsmEvent::Internal(op) => format!("Internal({op:?})"),
        other => format!("{other:?}"),
    }
}

fn format_headlamp(state: PublishedHeadlampState) -> &'static str {
    match state {
        PublishedHeadlampState::Off => "Off",
        PublishedHeadlampState::Ready => "Ready",
        PublishedHeadlampState::OnRequested => "On requested",
        PublishedHeadlampState::On => "On",
        PublishedHeadlampState::OffRequested => "Off requested",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::facade::{
        PublishedHeadlampContext, PublishedHealthContext, PublishedPowertrainContext,
        PublishedVehicleContext, PublishedVisibilityContext, PublishedWheelRpm, UnixTimestamp,
    };
    use std::time::Duration;

    fn sample_ledger() -> PublishedTransitionRecord {
        PublishedTransitionRecord {
            car_identity: "x".into(),
            session_started_at: UnixTimestamp::from_duration_since_epoch(Duration::from_secs(1)),
            record_seq: 7,
            recorded_at: UnixTimestamp::from_duration_since_epoch(Duration::from_secs(2)),
            event: PublishedFsmEvent::UpdateAmbientLux(120),
            old_state: PublishedFsmState::Idle,
            next_state: PublishedFsmState::Driving,
            old_ctx: empty_ctx(),
            current_ctx: empty_ctx(),
            actions: vec![],
        }
    }

    fn empty_ctx() -> PublishedVehicleContext {
        PublishedVehicleContext {
            powertrain: PublishedPowertrainContext {
                wheel_rpm: PublishedWheelRpm {
                    front_left: 0,
                    front_right: 0,
                    rear_left: 0,
                    rear_right: 0,
                },
                speed_kph: 0,
            },
            health: PublishedHealthContext {
                fuel_level_pct: 100,
                oil_pressure_kpa: 100,
                tyre_pressure_ok: true,
            },
            visibility: PublishedVisibilityContext { ambient_lux: 0 },
            headlamp: PublishedHeadlampContext {
                state: PublishedHeadlampState::On,
                ack_pending_since: None,
            },
        }
    }

    #[test]
    fn engineer_fills_state_and_event_rob_placeholder() {
        let pane = engineer_pane(Some(&sample_ledger()), 48);
        assert!(pane.lines[0].text().contains("Current state: Driving"));
        assert!(
            pane.lines[1]
                .text()
                .contains("Last event: UpdateAmbientLux(120)")
        );
        assert!(
            pane.lines
                .iter()
                .any(|l| l.text().contains("Active ROB turns: —"))
        );
        assert!(pane.lines.iter().any(|l| l.text().contains("Headlamp: On")));
        assert!(pane.lines.iter().any(|l| l.text().contains("Wiper: —")));
    }
}
