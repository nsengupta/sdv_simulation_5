//! Single-application entry: **Digital Twin** (via gateway runtime APIs) + **Dashboard**.
//!
//! `main()` wires observation channels in setup, installs the twin through
//! [`TwinRuntimeBuilder`], then runs the dashboard as a passive display of twin emissions.
//! Session elapsed time is derived from twin records ([`SessionClock`]), not a local clock.
//!
//! Only **`q`** / Esc quit the dashboard. Lifecycle (PowerOn/PowerOff) is CAN / emulator driven.
//! Vehicle operation (RPM, park, lighting, …) is CAN / emulator driven — not the dashboard.

use std::time::Duration;

use anyhow::Result;
use common::observation_records::diagnostic::elapsed_since_session;
use crossterm::ExecutableCommand;
use crossterm::event::{self, Event, KeyCode};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use gateway::gateway_runtime::TwinRuntimeBuilder;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::{Frame, Terminal};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use common::DiagnosticRecord;
use common::PublishedDomainAction;
use common::facade::{PublishedFsmEvent, PublishedFsmState, PublishedTransitionRecord};

const VIRTUAL_CAR_IDENTITY: &str = "My-Opel-Corsa-1.4-GSi";
const BOOT_DIAGNOSTIC_WAIT: Duration = Duration::from_millis(500);
const KEYS_FOOTER: &str = "Keys: 'q' quit";
const MAX_PANEL_LINE_CHARS: usize = 72;

/// Channels and runtime handles wired in `main()` before the dashboard loop runs.
struct DigitalTwinRuntime {
    diagnostic_rx: mpsc::UnboundedReceiver<DiagnosticRecord>,
    transition_rx: mpsc::Receiver<PublishedTransitionRecord>,
    /// Keeps CAN ingress and actuation workers alive for the session.
    _runtime_handle: JoinHandle<Result<()>>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let mut twin = install_digital_twin().await?;
    run_dashboard(&mut twin).await
}

/// Setup call-tree: create channels, install twin via gateway runtime APIs, spawn ingress.
async fn install_digital_twin() -> Result<DigitalTwinRuntime> {
    let (diag_tx, diagnostic_rx) = mpsc::unbounded_channel::<DiagnosticRecord>();
    let (trans_tx, transition_rx) = mpsc::channel::<PublishedTransitionRecord>(256);

    let mut builder = TwinRuntimeBuilder::new()
        .with_car_identity(VIRTUAL_CAR_IDENTITY)
        .with_can_interface(gateway::gateway_runtime::DEFAULT_CAN_INTERFACE)
        .with_auto_power_on(false)
        .with_diagnostic_channel(diag_tx)
        .with_transition_channel(trans_tx);

    let (controller, _opts) = builder.install_controller().await?;
    let runtime_handle = builder.spawn_runtime(controller)?;

    Ok(DigitalTwinRuntime {
        diagnostic_rx,
        transition_rx,
        _runtime_handle: runtime_handle,
    })
}

async fn run_dashboard(twin: &mut DigitalTwinRuntime) -> Result<()> {
    let mut latest_diagnostic =
        await_boot_diagnostic(&mut twin.diagnostic_rx, BOOT_DIAGNOSTIC_WAIT).await;
    let mut latest_transition: Option<PublishedTransitionRecord> = None;

    enable_raw_mode()?;
    let mut stderr = std::io::stderr();
    crossterm::execute!(stderr, EnterAlternateScreen)?;
    let mut terminal = Terminal::new(ratatui::backend::CrosstermBackend::new(stderr))?;

    let res = run_ui_loop(
        &mut terminal,
        twin,
        &mut latest_diagnostic,
        &mut latest_transition,
    )
    .await;

    disable_raw_mode()?;
    terminal.backend_mut().execute(LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    // Convenience (see DESIGN §16.6): quitting exits the process and drops the runtime handle,
    // which stops CAN ingress to the twin. A future version may disband explicitly (TL-6).
    if let Err(e) = res {
        eprintln!("Dashboard error: {e:?}");
    }
    Ok(())
}

async fn await_boot_diagnostic(
    rx: &mut mpsc::UnboundedReceiver<DiagnosticRecord>,
    timeout: Duration,
) -> Option<DiagnosticRecord> {
    match tokio::time::timeout(timeout, rx.recv()).await {
        Ok(Some(record)) => Some(record),
        Ok(None) | Err(_) => None,
    }
}

async fn run_ui_loop(
    terminal: &mut Terminal<ratatui::backend::CrosstermBackend<std::io::Stderr>>,
    twin: &mut DigitalTwinRuntime,
    latest_diagnostic: &mut Option<DiagnosticRecord>,
    latest_transition: &mut Option<PublishedTransitionRecord>,
) -> Result<()> {
    loop {
        drain_twin_emissions(
            &mut twin.diagnostic_rx,
            &mut twin.transition_rx,
            latest_diagnostic,
            latest_transition,
        );

        terminal.draw(|f| {
            render_frame(f, latest_diagnostic, latest_transition);
        })?;

        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => break,
                    _ => {}
                }
            }
        }
    }
    Ok(())
}

/// Dashboard trusts the twin: display only what arrives on the observation channels.
fn drain_twin_emissions(
    diag_rx: &mut mpsc::UnboundedReceiver<DiagnosticRecord>,
    trans_rx: &mut mpsc::Receiver<PublishedTransitionRecord>,
    latest_diagnostic: &mut Option<DiagnosticRecord>,
    latest_transition: &mut Option<PublishedTransitionRecord>,
) {
    while let Ok(record) = diag_rx.try_recv() {
        *latest_diagnostic = Some(record);
    }
    while let Ok(record) = trans_rx.try_recv() {
        *latest_transition = Some(record);
    }
}

fn render_frame(
    f: &mut Frame,
    latest_diagnostic: &Option<DiagnosticRecord>,
    latest_transition: &Option<PublishedTransitionRecord>,
) {
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(3),
        ])
        .split(f.size());

    let status = format_status_line(latest_diagnostic, latest_transition);
    let session_block = Block::default()
        .title(" Session ")
        .borders(Borders::ALL)
        .style(Style::default().fg(Color::Yellow));
    f.render_widget(
        Paragraph::new(Line::from(Span::raw(status))).block(session_block),
        outer[0],
    );

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(outer[1]);

    let panel_width = chunks[0].width.saturating_sub(4) as usize;
    let line_limit = panel_width.max(24).min(MAX_PANEL_LINE_CHARS);

    let pre_power = twin_pre_power_on(latest_transition);

    let diag_text = if pre_power {
        standby_panel_lines()
    } else if let Some(d) = latest_diagnostic {
        vec![
            Line::from(Span::raw(format_field(
                "Level",
                &format!("{:?}", d.level),
                line_limit,
            ))),
            Line::from(Span::raw(format_field("Source", &d.source, line_limit))),
            Line::from(Span::raw(format_field(
                "Message",
                &truncate_line(&d.message),
                line_limit,
            ))),
            Line::from(Span::raw(format_field(
                "T+ since session",
                &format_elapsed(d.elapsed_since_session()),
                line_limit,
            ))),
        ]
    } else {
        vec![Line::from(Span::raw("(no diagnostic from twin yet)"))]
    };
    let diag_block = Block::default()
        .title(" Diagnostic ")
        .borders(Borders::ALL)
        .style(Style::default().fg(Color::Cyan));
    f.render_widget(Paragraph::new(diag_text).block(diag_block), chunks[0]);

    let trans_text = if pre_power {
        standby_panel_lines()
    } else if let Some(t) = latest_transition {
        vec![
            Line::from(Span::raw(format_field(
                "Seq",
                &t.record_seq.to_string(),
                line_limit,
            ))),
            Line::from(Span::raw(format_field(
                "Event",
                &format_published_event(&t.event),
                line_limit,
            ))),
            Line::from(Span::raw(format_field(
                "Old state",
                &format_published_state(&t.old_state),
                line_limit,
            ))),
            Line::from(Span::raw(format_field(
                "Next state",
                &format_published_state(&t.next_state),
                line_limit,
            ))),
            Line::from(Span::raw(format_field(
                "Actions",
                &format_actions_summary(&t.actions),
                line_limit,
            ))),
            Line::from(Span::raw(format_field(
                "T+ since session",
                &format_elapsed(elapsed_since_session(
                    t.recorded_at_unix,
                    t.session_start_unix_nanos,
                )),
                line_limit,
            ))),
        ]
    } else {
        vec![Line::from(Span::raw("(no ledger row from twin yet)"))]
    };
    let trans_block = Block::default()
        .title(" Transition ")
        .borders(Borders::ALL)
        .style(Style::default().fg(Color::Green));
    f.render_widget(Paragraph::new(trans_text).block(trans_block), chunks[1]);

    let keys_block = Block::default()
        .borders(Borders::ALL)
        .style(Style::default().fg(Color::DarkGray));
    f.render_widget(
        Paragraph::new(Line::from(Span::raw(KEYS_FOOTER))).block(keys_block),
        outer[2],
    );
}

/// Before the first ledger row (PowerOn), twin is installed but not yet powered for observation panes.
fn twin_pre_power_on(latest_transition: &Option<PublishedTransitionRecord>) -> bool {
    latest_transition.is_none()
}

fn format_field(label: &str, value: &str, max_chars: usize) -> String {
    let value = truncate_to(value, max_chars.saturating_sub(label.len() + 2));
    format!("{label}: {value}")
}

fn truncate_line(s: &str) -> String {
    truncate_to(s, MAX_PANEL_LINE_CHARS)
}

fn truncate_to(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_owned();
    }
    let end = s
        .char_indices()
        .nth(max_chars.saturating_sub(1))
        .map(|(idx, _)| idx)
        .unwrap_or(s.len());
    format!("{}…", &s[..end])
}

fn format_published_event(event: &PublishedFsmEvent) -> String {
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

fn format_published_state(state: &PublishedFsmState) -> String {
    match state {
        PublishedFsmState::ExtremeOperationWarning { entered_at_unix } => {
            format!("ExtremeOpWarn@{}ms", entered_at_unix.as_millis())
        }
        other => format!("{other:?}"),
    }
}

fn format_actions_summary(actions: &[PublishedDomainAction]) -> String {
    if actions.is_empty() {
        return "—".to_string();
    }
    actions
        .iter()
        .map(|action| match action {
            PublishedDomainAction::LogWarning(msg) => {
                format!("LogWarning({})", truncate_line(msg))
            }
            other => format!("{other:?}"),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn standby_panel_lines() -> Vec<Line<'static>> {
    vec![Line::from(Span::raw(
        "Twin installed; waiting for PowerOn on CAN — ledger and diagnostics appear after lifecycle starts.",
    ))]
}

fn format_status_line(
    latest_diagnostic: &Option<DiagnosticRecord>,
    latest_transition: &Option<PublishedTransitionRecord>,
) -> String {
    let session_nanos = latest_transition
        .as_ref()
        .map(|t| t.session_start_unix_nanos)
        .or_else(|| {
            latest_diagnostic
                .as_ref()
                .map(|d| d.session_start_unix_nanos)
        });

    let session_label = session_nanos
        .map(format_unix_nanos_short)
        .unwrap_or_else(|| "awaiting twin…".to_string());

    let twin_elapsed = latest_twin_elapsed(latest_diagnostic, latest_transition)
        .map(|d| format_elapsed(d))
        .unwrap_or_else(|| "—".to_string());

    let last_ledger = latest_transition
        .as_ref()
        .map(|t| format!("seq {}", t.record_seq))
        .unwrap_or_else(|| "—".to_string());

    let fsm_label = twin_fsm_status_label(latest_transition);

    format!(
        "Car: {VIRTUAL_CAR_IDENTITY}  │  Twin T+: {twin_elapsed}  │  Session start: {session_label}  │  FSM: {fsm_label}  │  Last ledger: {last_ledger}"
    )
}

fn twin_fsm_status_label(latest_transition: &Option<PublishedTransitionRecord>) -> String {
    match latest_transition {
        None => "standby (no ledger yet)".to_string(),
        Some(row) => format_published_state(&row.next_state),
    }
}

fn latest_twin_elapsed(
    latest_diagnostic: &Option<DiagnosticRecord>,
    latest_transition: &Option<PublishedTransitionRecord>,
) -> Option<Duration> {
    match (latest_diagnostic, latest_transition) {
        (Some(d), Some(t)) => Some(d.elapsed_since_session().max(elapsed_since_session(
            t.recorded_at_unix,
            t.session_start_unix_nanos,
        ))),
        (Some(d), None) => Some(d.elapsed_since_session()),
        (None, Some(t)) => Some(elapsed_since_session(
            t.recorded_at_unix,
            t.session_start_unix_nanos,
        )),
        (None, None) => None,
    }
}

fn format_elapsed(d: Duration) -> String {
    let secs = d.as_secs();
    let hours = secs / 3600;
    let mins = (secs % 3600) / 60;
    let secs = secs % 60;
    if hours > 0 {
        format!("{hours}h {mins:02}m {secs:02}s")
    } else if mins > 0 {
        format!("{mins}m {secs:02}s")
    } else {
        format!("{secs}s")
    }
}

fn format_unix_nanos_short(nanos: u128) -> String {
    let secs = (nanos / 1_000_000_000) as u64;
    format!(
        "{:02}:{:02}:{:02} UTC",
        (secs / 3600) % 24,
        (secs % 3600) / 60,
        secs % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::DiagnosticLevel;

    fn sample_boot_diagnostic() -> DiagnosticRecord {
        DiagnosticRecord {
            level: DiagnosticLevel::Info,
            source: "VirtualCarActor",
            message: "initializing".into(),
            session_start_unix_nanos: 1_700_000_000_000_000_000,
            recorded_at_unix: Duration::from_nanos(1_700_000_000_050_000_000),
        }
    }

    #[test]
    fn status_line_uses_boot_diagnostic_without_ledger() {
        let diag = sample_boot_diagnostic();
        let line = format_status_line(&Some(diag.clone()), &None);
        assert!(line.contains("standby (no ledger yet)"));
        assert!(line.contains("Twin T+:"));
        assert!(!line.contains("awaiting twin"));
        assert!(line.contains("Last ledger: —"));
        assert_eq!(
            latest_twin_elapsed(&Some(diag), &None),
            Some(Duration::from_millis(50))
        );
    }

    #[test]
    fn status_line_fsm_from_latest_ledger_row() {
        let row = PublishedTransitionRecord {
            car_identity: "x".into(),
            session_start_unix_nanos: 1,
            record_seq: 2,
            recorded_at_unix: Duration::ZERO,
            event: PublishedFsmEvent::UpdateRpm(1500),
            old_state: PublishedFsmState::Idle,
            next_state: PublishedFsmState::Driving,
            old_ctx: empty_published_ctx(),
            current_ctx: empty_published_ctx(),
            actions: vec![],
        };
        let line = format_status_line(&None, &Some(row));
        assert!(line.contains("FSM: Driving"));
    }

    #[test]
    fn pre_power_panels_until_first_ledger_row() {
        assert!(twin_pre_power_on(&None));
        assert!(!twin_pre_power_on(&Some(sample_ledger_row())));
    }

    #[test]
    fn standby_panels_show_can_lifecycle_message() {
        let lines = standby_panel_lines();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].spans[0].content.contains("PowerOn"));
        assert!(lines[0].spans[0].content.contains("CAN"));
    }

    #[test]
    fn keys_footer_lists_quit_only() {
        assert_eq!(KEYS_FOOTER, "Keys: 'q' quit");
    }

    #[test]
    fn status_line_prefixes_car_identity() {
        let line = format_status_line(&None, &None);
        assert!(line.starts_with("Car: My-Opel-Corsa"));
    }

    #[test]
    fn truncate_line_shortens_long_rejection_messages() {
        let long = "[REJECTED]: vehicle must be Idle before PowerOff; current state is DrivingDangerously with extra detail";
        let truncated = truncate_line(long);
        assert!(truncated.chars().count() <= MAX_PANEL_LINE_CHARS);
        assert!(truncated.ends_with('…'));
    }

    #[test]
    fn format_actions_summary_truncates_log_warning() {
        let summary = format_actions_summary(&[PublishedDomainAction::LogWarning(
            "[REJECTED]: vehicle must be Idle before PowerOff; current state is Driving".into(),
        )]);
        assert!(summary.starts_with("LogWarning("));
        assert!(summary.len() < 120);
    }

    fn sample_ledger_row() -> PublishedTransitionRecord {
        PublishedTransitionRecord {
            car_identity: "x".into(),
            session_start_unix_nanos: 1,
            record_seq: 1,
            recorded_at_unix: Duration::ZERO,
            event: PublishedFsmEvent::PowerOn,
            old_state: PublishedFsmState::Off,
            next_state: PublishedFsmState::PreparingToStart,
            old_ctx: empty_published_ctx(),
            current_ctx: empty_published_ctx(),
            actions: vec![],
        }
    }

    fn empty_published_ctx() -> common::facade::PublishedVehicleContext {
        use common::facade::{
            PublishedHeadlampContext, PublishedHeadlampState, PublishedHealthContext,
            PublishedPowertrainContext, PublishedVehicleContext, PublishedVisibilityContext,
            PublishedWheelRpm,
        };
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
                state: PublishedHeadlampState::Off,
                ack_pending_since_at_unix: None,
            },
        }
    }
}
