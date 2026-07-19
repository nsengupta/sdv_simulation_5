use super::{MISSING, fit_line};
use common::DiagnosticRecord;
use common::facade::{
    DiagnosticKind, DiagnosticLevel, PublishedHeadlampState, PublishedTransitionRecord,
};
use common::fsm::FrontHeadlampIncompleteCause;
use common::vehicle_physics::format_speed_bar;

pub struct DriverPane {
    pub lines: Vec<String>,
}

/// Whether an Observer Notice should adopt this diagnostic as the latest displayed notice.
///
/// The diagnostic stream is wider than the Notice surface — e.g. [`DiagnosticKind::TimerTick`]
/// stays in the stream/capture but must not overwrite the driver Notice.
pub fn should_update_notice(kind: &DiagnosticKind) -> bool {
    !matches!(kind, DiagnosticKind::TimerTick)
}

pub fn driver_pane(
    diagnostic: Option<&DiagnosticRecord>,
    ledger: Option<&PublishedTransitionRecord>,
    width: usize,
) -> DriverPane {
    if ledger.is_none() {
        return DriverPane {
            lines: [
                "Twin installed.",
                "Waiting for PowerOn on CAN.",
                "Ledger and diagnostics appear after lifecycle starts.",
            ]
            .into_iter()
            .map(|line| fit_line(line, width))
            .collect(),
        };
    }

    let mut lines = Vec::with_capacity(4);
    lines.push(fit_line(&format_notice(diagnostic), width));
    lines.push(fit_line(&format_speed_line(ledger, width), width));
    lines.push(fit_line(&format_visibility_line(ledger), width));
    // TODO(phase-5-follow-up): Twin rain / wiper presentation fields.
    lines.push(fit_line(&format_weather_line(), width));
    DriverPane { lines }
}

fn format_notice(diagnostic: Option<&DiagnosticRecord>) -> String {
    let Some(d) = diagnostic else {
        return "Notice: (no notice yet)".to_owned();
    };
    match &d.kind {
        DiagnosticKind::TimerTick => "Notice: (no notice yet)".to_owned(),
        DiagnosticKind::Boot => {
            format!("Notice: {} — Twin booting", format_level(d.level))
        }
        DiagnosticKind::HeadlampActuationUnconfirmed { on, cause } => {
            let dir = if *on { "ON" } else { "OFF" };
            let why = match cause {
                FrontHeadlampIncompleteCause::TimedOut => "timeout",
                FrontHeadlampIncompleteCause::NegativeAck => "NACK",
                _ => "unconfirmed",
            };
            format!(
                "Notice: {} — Headlamp {dir} not confirmed ({why})",
                format_level(d.level)
            )
        }
        DiagnosticKind::RainChanged { raining } => format!(
            "Notice: {} — Rain {}",
            format_level(d.level),
            if *raining { "detected" } else { "cleared" }
        ),
        DiagnosticKind::WiperMotionChanged { wiping } => format!(
            "Notice: {} — Wipers {}",
            format_level(d.level),
            if *wiping { "active" } else { "stopped" }
        ),
        DiagnosticKind::ActuationFailure { action, error } => format!(
            "Notice: {} — Actuation failure ({action}: {error})",
            format_level(d.level)
        ),
        DiagnosticKind::TransitionSinkFull => {
            format!(
                "Notice: {} — Transition sink full",
                format_level(d.level)
            )
        }
        DiagnosticKind::TransitionSinkClosed => {
            format!(
                "Notice: {} — Transition sink closed",
                format_level(d.level)
            )
        }
        DiagnosticKind::Text { text } => {
            let msg = driver_facing_text(text);
            if msg == "Must be IDLE before POWER-OFF" {
                format!("Notice: {msg}")
            } else {
                format!("Notice: {} — {msg}", format_level(d.level))
            }
        }
    }
}

/// Strip twin/engineer jargon from free-form [`DiagnosticKind::Text`]; identity is on the Session bar.
fn driver_facing_text(raw: &str) -> String {
    let mut s = raw.replace('\n', " ");
    if let Some(rest) = s.strip_prefix('[') {
        if let Some(idx) = rest.find("]: ") {
            s = rest[idx + 3..].to_owned();
        }
    }
    if s.contains("must be Idle before PowerOff") {
        return "Must be IDLE before POWER-OFF".to_owned();
    }
    if let Some(rest) = s.strip_prefix("[REJECTED]: ") {
        return rest.to_owned();
    }
    s
}

fn format_level(level: DiagnosticLevel) -> &'static str {
    match level {
        DiagnosticLevel::Info => "Info",
        DiagnosticLevel::Action => "Action",
        DiagnosticLevel::Alert => "Alert",
        DiagnosticLevel::Warning => "Warning",
        DiagnosticLevel::Error => "Error",
    }
}

fn format_speed_line(ledger: Option<&PublishedTransitionRecord>, width: usize) -> String {
    use common::vehicle_physics::SPEED_EXTREME_OPERATION_THRESHOLD_KPH;
    use unicode_width::UnicodeWidthStr;

    let Some(row) = ledger else {
        return format!("Speed: {MISSING}");
    };
    let speed = row.current_ctx.powertrain.speed_kph;
    let prefix = "Speed: [";
    let suffix = format!("] {speed}/{SPEED_EXTREME_OPERATION_THRESHOLD_KPH} km/h");
    let overhead = prefix.width() + suffix.width();
    let bar_width = width.saturating_sub(overhead);
    let bar = format_speed_bar(speed, bar_width);
    format!("{prefix}{bar}{suffix}")
}

fn format_visibility_line(ledger: Option<&PublishedTransitionRecord>) -> String {
    let Some(row) = ledger else {
        return format!("Visibility: ({MISSING})  Headlamps: {MISSING}");
    };
    let lux = row.current_ctx.visibility.ambient_lux;
    let mut headlamps = format_headlamp_state(row.current_ctx.headlamp.state).to_owned();
    if row.current_ctx.headlamp.ack_pending_since.is_some() {
        headlamps.push_str("; waiting for reply");
    }
    format!("Visibility: ({lux} lux)  Headlamps: {headlamps}")
}

fn format_weather_line() -> String {
    format!("Rain: {MISSING}  Wipers: {MISSING}")
}

fn format_headlamp_state(state: PublishedHeadlampState) -> &'static str {
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
        PublishedFsmEvent, PublishedFsmState, PublishedHeadlampContext, PublishedHealthContext,
        PublishedPowertrainContext, PublishedVehicleContext, PublishedVisibilityContext,
        PublishedWheelRpm, UnixTimestamp,
    };
    use std::time::Duration;

    fn sample_diag(kind: DiagnosticKind) -> DiagnosticRecord {
        DiagnosticRecord {
            level: DiagnosticLevel::Warning,
            source: "test",
            kind,
            session_started_at: UnixTimestamp::from_duration_since_epoch(Duration::from_secs(1)),
            recorded_at: UnixTimestamp::from_duration_since_epoch(Duration::from_secs(2)),
        }
    }

    fn sample_ledger(
        speed: u16,
        lux: u16,
        headlamp: PublishedHeadlampState,
    ) -> PublishedTransitionRecord {
        PublishedTransitionRecord {
            car_identity: "x".into(),
            session_started_at: UnixTimestamp::from_duration_since_epoch(Duration::from_secs(1)),
            record_seq: 1,
            recorded_at: UnixTimestamp::from_duration_since_epoch(Duration::from_secs(2)),
            event: PublishedFsmEvent::UpdateRpm(1500),
            old_state: PublishedFsmState::Idle,
            next_state: PublishedFsmState::Driving,
            old_ctx: ctx(0, 0, PublishedHeadlampState::Off),
            current_ctx: ctx(speed, lux, headlamp),
            actions: vec![],
        }
    }

    fn ctx(speed: u16, lux: u16, headlamp: PublishedHeadlampState) -> PublishedVehicleContext {
        PublishedVehicleContext {
            powertrain: PublishedPowertrainContext {
                wheel_rpm: PublishedWheelRpm {
                    front_left: 0,
                    front_right: 0,
                    rear_left: 0,
                    rear_right: 0,
                },
                speed_kph: speed,
            },
            health: PublishedHealthContext {
                fuel_level_pct: 100,
                oil_pressure_kpa: 100,
                tyre_pressure_ok: true,
            },
            visibility: PublishedVisibilityContext {
                ambient_lux: lux,
            },
            headlamp: PublishedHeadlampContext {
                state: headlamp,
                ack_pending_since: None,
            },
        }
    }

    #[test]
    fn driver_shows_notice_and_speed_bar() {
        let diag = sample_diag(DiagnosticKind::Text {
            text: "tunnel ahead".into(),
        });
        let ledger = sample_ledger(80, 150, PublishedHeadlampState::On);
        let pane = driver_pane(Some(&diag), Some(&ledger), 48);
        assert!(pane.lines[0].contains("Notice: Warning"));
        assert!(pane.lines[0].contains("tunnel ahead"));
        let speed_line = &pane.lines[1];
        assert!(speed_line.starts_with("Speed: ["));
        assert!(speed_line.contains("80/160 km/h"));
        assert!(speed_line.contains('|'));
    }

    #[test]
    fn notice_formats_unconfirmed_without_icons() {
        let diag = sample_diag(DiagnosticKind::HeadlampActuationUnconfirmed {
            on: true,
            cause: FrontHeadlampIncompleteCause::TimedOut,
        });
        let ledger = sample_ledger(0, 100, PublishedHeadlampState::Ready);
        let pane = driver_pane(Some(&diag), Some(&ledger), 80);
        let notice = pane.lines[0].trim_end();
        assert!(notice.contains("not confirmed"));
        assert!(notice.contains("timeout"));
        assert!(!notice.contains('✅'));
        assert!(!notice.contains('✓'));
    }

    #[test]
    fn timer_tick_does_not_update_notice() {
        assert!(!should_update_notice(&DiagnosticKind::TimerTick));
        assert!(should_update_notice(&DiagnosticKind::Boot));
        assert!(should_update_notice(
            &DiagnosticKind::HeadlampActuationUnconfirmed {
                on: false,
                cause: FrontHeadlampIncompleteCause::NegativeAck,
            }
        ));
    }

    #[test]
    fn driver_notice_strips_car_id_and_rejection_jargon() {
        let diag = sample_diag(DiagnosticKind::Text {
            text: "[My-Opel-Corsa-1.4-GSi]: [REJECTED]: vehicle must be Idle before PowerOff; current state is Driving".into(),
        });
        let ledger = sample_ledger(0, 100, PublishedHeadlampState::Off);
        let pane = driver_pane(Some(&diag), Some(&ledger), 60);
        assert!(pane.lines[0].starts_with("Notice: Must be IDLE before POWER-OFF"));
        assert!(!pane.lines[0].contains("My-Opel"));
        assert!(!pane.lines[0].contains("REJECTED"));
    }

    #[test]
    fn driver_visibility_and_headlamps_share_a_line() {
        let ledger = sample_ledger(10, 150, PublishedHeadlampState::On);
        let pane = driver_pane(None, Some(&ledger), 64);
        let line = pane
            .lines
            .iter()
            .find(|l| l.contains("Visibility:"))
            .expect("visibility line");
        assert!(line.contains("Visibility: (150 lux)"));
        assert!(line.contains("Headlamps: On"));
    }

    #[test]
    fn driver_rain_and_wipers_share_a_placeholder_line() {
        let diag = sample_diag(DiagnosticKind::Text {
            text: "ok".into(),
        });
        let ledger = sample_ledger(10, 800, PublishedHeadlampState::Off);
        let pane = driver_pane(Some(&diag), Some(&ledger), 40);
        let line = pane
            .lines
            .iter()
            .find(|l| l.contains("Rain:"))
            .expect("weather line");
        assert!(line.contains("Rain: —"));
        assert!(line.contains("Wipers: —"));
    }

    #[test]
    fn driver_lines_never_exceed_width_or_wrap() {
        use unicode_width::UnicodeWidthStr;
        let diag = sample_diag(DiagnosticKind::Text {
            text: "x".repeat(200),
        });
        let ledger = sample_ledger(40, 10, PublishedHeadlampState::OnRequested);
        let pane = driver_pane(Some(&diag), Some(&ledger), 32);
        for line in &pane.lines {
            assert_eq!(line.width(), 32, "{line:?}");
            assert!(!line.contains('\n'));
        }
    }

    #[test]
    fn higher_speed_fills_more_bar_cells() {
        let width = 48;
        let low = driver_pane(
            None,
            Some(&sample_ledger(40, 0, PublishedHeadlampState::Off)),
            width,
        );
        let high = driver_pane(
            None,
            Some(&sample_ledger(120, 0, PublishedHeadlampState::Off)),
            width,
        );
        let count = |line: &str| line.chars().filter(|c| *c == '|').count();
        assert!(count(&high.lines[1]) > count(&low.lines[1]));
    }
}
