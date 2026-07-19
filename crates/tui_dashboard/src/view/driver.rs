use super::{MISSING, LineRole, PaneLine, Segment, SegmentContent, SegmentStyle};
use common::DiagnosticRecord;
use common::facade::{
    DiagnosticKind, DiagnosticLevel, PublishedHeadlampState, PublishedTransitionRecord,
};
use common::fsm::FrontHeadlampIncompleteCause;
use common::vehicle_physics::{
    SPEED_EXTREME_OPERATION_THRESHOLD_KPH, speed_band, speed_bar_cells,
};
use unicode_width::UnicodeWidthStr;

pub struct DriverPane {
    pub lines: Vec<PaneLine>,
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
            .map(|line| PaneLine::plain_fitted(LineRole::Standby, line, width))
            .collect(),
        };
    }

    // TODO(phase-5-follow-up): Twin rain / wiper presentation fields (+ Icon segments).
    let lines = vec![
        PaneLine::plain_fitted(LineRole::Notice, &format_notice(diagnostic), width),
        speed_pane_line(ledger, width),
        PaneLine::plain_fitted(LineRole::Visibility, &format_visibility_line(ledger), width),
        PaneLine::plain_fitted(LineRole::Weather, &format_weather_line(), width),
    ];
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

fn speed_pane_line(ledger: Option<&PublishedTransitionRecord>, width: usize) -> PaneLine {
    let Some(row) = ledger else {
        return PaneLine::plain_fitted(LineRole::Speed, &format!("Speed: {MISSING}"), width);
    };
    let speed = row.current_ctx.powertrain.speed_kph;
    let band = speed_band(speed);
    let suffix_body = format!("{speed}/{SPEED_EXTREME_OPERATION_THRESHOLD_KPH} km/h");
    let prefix = "Speed: [";
    let mid = "] ";
    let overhead = prefix.width() + mid.width() + suffix_body.width();
    let bar_width = width.saturating_sub(overhead);
    let cells = speed_bar_cells(speed, bar_width);

    let line = PaneLine {
        role: LineRole::Speed,
        segments: vec![
            Segment {
                style: SegmentStyle::Default,
                content: SegmentContent::Text(prefix.to_owned()),
            },
            Segment {
                style: SegmentStyle::Default,
                content: SegmentContent::SpeedBar { cells },
            },
            Segment {
                style: SegmentStyle::Default,
                content: SegmentContent::Text(mid.to_owned()),
            },
            Segment {
                style: SegmentStyle::from_speed_band(band),
                content: SegmentContent::Text(suffix_body),
            },
        ],
    };
    line.pad_to_width(width)
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
    use common::vehicle_physics::SpeedBand;
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
        assert_eq!(pane.lines[0].role, LineRole::Notice);
        assert!(pane.lines[0].text().contains("Notice: Warning"));
        assert!(pane.lines[0].text().contains("tunnel ahead"));
        let speed_line = pane.lines[1].text();
        assert!(speed_line.starts_with("Speed: ["));
        assert!(speed_line.contains("80/160 km/h"));
        assert!(speed_line.contains('|'));
        assert_eq!(pane.lines[1].role, LineRole::Speed);
    }

    #[test]
    fn speed_line_zones_and_numeric_band_style() {
        let ledger = sample_ledger(155, 0, PublishedHeadlampState::Off);
        let pane = driver_pane(None, Some(&ledger), 64);
        let speed = &pane.lines[1];
        let bar = speed
            .segments
            .iter()
            .find_map(|s| match &s.content {
                SegmentContent::SpeedBar { cells } => Some(cells),
                _ => None,
            })
            .expect("speed bar segment");
        assert!(bar.iter().any(|c| c.band == SpeedBand::Green));
        assert!(bar.iter().any(|c| c.band == SpeedBand::Yellow));
        assert!(bar.iter().any(|c| c.band == SpeedBand::Red));
        let numeric = speed
            .segments
            .iter()
            .find(|s| matches!(&s.content, SegmentContent::Text(t) if t.contains("km/h")))
            .expect("numeric suffix");
        assert_eq!(numeric.style, SegmentStyle::ZoneRed);
        assert!(
            speed
                .segments
                .iter()
                .any(|s| matches!(&s.content, SegmentContent::Text(t) if t.starts_with("Speed:"))
                    && s.style == SegmentStyle::Default)
        );
    }

    #[test]
    fn notice_formats_unconfirmed_without_icons() {
        let diag = sample_diag(DiagnosticKind::HeadlampActuationUnconfirmed {
            on: true,
            cause: FrontHeadlampIncompleteCause::TimedOut,
        });
        let ledger = sample_ledger(0, 100, PublishedHeadlampState::Ready);
        let pane = driver_pane(Some(&diag), Some(&ledger), 80);
        let notice = pane.lines[0].text();
        let notice = notice.trim_end();
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
        assert!(
            pane.lines[0]
                .text()
                .starts_with("Notice: Must be IDLE before POWER-OFF")
        );
        assert!(!pane.lines[0].text().contains("My-Opel"));
        assert!(!pane.lines[0].text().contains("REJECTED"));
    }

    #[test]
    fn driver_visibility_and_headlamps_share_a_line() {
        let ledger = sample_ledger(10, 150, PublishedHeadlampState::On);
        let pane = driver_pane(None, Some(&ledger), 64);
        let line = pane
            .lines
            .iter()
            .find(|l| l.text().contains("Visibility:"))
            .expect("visibility line");
        assert!(line.text().contains("Visibility: (150 lux)"));
        assert!(line.text().contains("Headlamps: On"));
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
            .find(|l| l.text().contains("Rain:"))
            .expect("weather line");
        assert!(line.text().contains("Rain: —"));
        assert!(line.text().contains("Wipers: —"));
    }

    #[test]
    fn driver_lines_never_exceed_width_or_wrap() {
        let diag = sample_diag(DiagnosticKind::Text {
            text: "x".repeat(200),
        });
        let ledger = sample_ledger(40, 10, PublishedHeadlampState::OnRequested);
        let pane = driver_pane(Some(&diag), Some(&ledger), 32);
        for line in &pane.lines {
            assert_eq!(line.display_width(), 32, "{:?}", line.text());
            assert!(!line.text().contains('\n'));
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
        let count = |line: &PaneLine| line.text().chars().filter(|c| *c == '|').count();
        assert!(count(&high.lines[1]) > count(&low.lines[1]));
    }
}
