//! Single-application entry: **Digital Twin** (via gateway runtime APIs) + **Dashboard**.
//!
//! `main()` wires observation channels in setup, installs the twin through
//! [`TwinRuntimeBuilder`], then runs the dashboard as a passive display of twin emissions.
//! Session elapsed time is derived from twin records ([`SessionClock`]), not a local clock.
//!
//! Only **`q`** / Esc quit the dashboard. Lifecycle (PowerOn/PowerOff) is CAN / emulator driven.
//! Vehicle operation (RPM, park, lighting, …) is CAN / emulator driven — not the dashboard.

mod cli;

use std::time::Duration;

use anyhow::Result;
use common::observation_records::diagnostic::elapsed_since_session;
use crossterm::event::{self, Event, KeyCode};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use gateway::gateway_runtime::TwinRuntimeBuilder;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::{Frame, Terminal};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use common::facade::{
    PublishedFsmEvent, PublishedFsmState, PublishedTransitionRecord, UnixTimestamp,
};
use common::DiagnosticRecord;
use common::PublishedDomainAction;
use observation::{RunId, RunMetadata, RunWriter, UnixTimestampV1};

const VIRTUAL_CAR_IDENTITY: &str = "My-Opel-Corsa-1.4-GSi";
const BOOT_DIAGNOSTIC_WAIT: Duration = Duration::from_millis(500);
const KEYS_FOOTER: &str = "Keys: 'q' quit";
const MAX_PANEL_LINE_CHARS: usize = 72;

type DashboardTerminal = Terminal<ratatui::backend::CrosstermBackend<std::io::Stderr>>;

/// Channels and runtime handles wired in `main()` before the dashboard loop runs.
struct DigitalTwinRuntime {
    diagnostic_rx: mpsc::UnboundedReceiver<DiagnosticRecord>,
    transition_rx: mpsc::Receiver<PublishedTransitionRecord>,
    /// Keeps CAN ingress and actuation workers alive for the session.
    _runtime_handle: JoinHandle<Result<()>>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = cli::parse_args(std::env::args_os().skip(1))?;
    let mut twin = install_digital_twin().await?;
    let boot = require_boot_diagnostic(&mut twin.diagnostic_rx, BOOT_DIAGNOSTIC_WAIT).await?;
    let metadata = RunMetadata::now(
        RunId::new_v4(),
        UnixTimestampV1::from_live(boot.session_started_at),
        VIRTUAL_CAR_IDENTITY,
        None,
    )?;
    let capture = RunWriter::create(&args.observation_dir, metadata)?;
    eprintln!("Observation run: {}", capture.run_dir().display());
    run_dashboard(&mut twin, capture, boot).await
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

async fn run_dashboard(
    twin: &mut DigitalTwinRuntime,
    mut capture: RunWriter,
    boot: DiagnosticRecord,
) -> Result<()> {
    let mut state = DashboardState::default();

    // The boot diagnostic is persisted before any terminal setup so a capture failure here
    // propagates cleanly without leaving the terminal in raw mode.
    capture = handle_boot_before_terminal(boot, capture, &mut state)?;

    let mut terminal = match setup_terminal() {
        Ok(terminal) => terminal,
        Err(setup_error) => {
            let finish_result = capture.finish_capture();
            return preserve_primary_result(Err(setup_error), finish_result);
        }
    };

    let loop_result = run_ui_loop(&mut terminal, twin, &mut capture, &mut state).await;
    let final_drain_result = final_drain_twin_emissions(
        &mut twin.diagnostic_rx,
        &mut twin.transition_rx,
        &mut capture,
        &mut state,
    );
    let operation_result = preserve_primary_result(loop_result, final_drain_result);

    let restoration_result = restore_terminal(&mut terminal);
    let finish_result = capture.finish_capture();

    // All work above is attempted before results are combined: terminal restoration precedes
    // finish, and finish runs on both successful and failed loop/final-drain paths. The earliest
    // operation error remains primary if restoration or finish also fail.
    let result = preserve_primary_result(operation_result, restoration_result);
    preserve_primary_result(result, finish_result)
}

fn setup_terminal() -> Result<DashboardTerminal> {
    enable_raw_mode()?;
    let mut stderr = std::io::stderr();

    if let Err(error) = crossterm::execute!(stderr, EnterAlternateScreen) {
        best_effort_restore_after_setup_failure();
        return Err(error.into());
    }

    match Terminal::new(ratatui::backend::CrosstermBackend::new(stderr)) {
        Ok(terminal) => Ok(terminal),
        Err(error) => {
            best_effort_restore_after_setup_failure();
            Err(error.into())
        }
    }
}

fn best_effort_restore_after_setup_failure() {
    let mut stderr = std::io::stderr();
    let _ = stderr.execute(LeaveAlternateScreen);
    let _ = disable_raw_mode();
}

fn restore_terminal(terminal: &mut DashboardTerminal) -> Result<()> {
    let raw_mode_result = disable_raw_mode().map_err(anyhow::Error::from);
    let alternate_screen_result = terminal
        .backend_mut()
        .execute(LeaveAlternateScreen)
        .map(|_| ())
        .map_err(anyhow::Error::from);
    let cursor_result = terminal.show_cursor().map_err(anyhow::Error::from);

    let result = preserve_primary_result(raw_mode_result, alternate_screen_result);
    preserve_primary_result(result, cursor_result)
}

fn preserve_primary_result(primary: Result<()>, secondary: Result<()>) -> Result<()> {
    match primary {
        Err(error) => Err(error),
        Ok(()) => secondary,
    }
}

/// Application boundary for durable capture. Keeping it narrow lets the UI loop be driven by a
/// fake in tests while `RunWriter` provides the production implementation.
trait RecordCapture {
    fn record_diagnostic(&mut self, record: &DiagnosticRecord) -> Result<()>;
    fn record_ledger(&mut self, record: &PublishedTransitionRecord) -> Result<()>;
}

trait CaptureFinalizer {
    fn finish_capture(self) -> Result<()>;
}

impl RecordCapture for RunWriter {
    fn record_diagnostic(&mut self, record: &DiagnosticRecord) -> Result<()> {
        RunWriter::record_diagnostic(self, record)?;
        Ok(())
    }

    fn record_ledger(&mut self, record: &PublishedTransitionRecord) -> Result<()> {
        RunWriter::record_ledger(self, record)?;
        Ok(())
    }
}

impl CaptureFinalizer for RunWriter {
    fn finish_capture(self) -> Result<()> {
        self.finish()?;
        Ok(())
    }
}

/// Latest twin emissions retained purely for rendering; every record is captured first.
#[derive(Default)]
struct DashboardState {
    latest_diagnostic: Option<DiagnosticRecord>,
    latest_transition: Option<PublishedTransitionRecord>,
}

fn handle_diagnostic(
    record: DiagnosticRecord,
    capture: &mut impl RecordCapture,
    state: &mut DashboardState,
) -> Result<()> {
    capture.record_diagnostic(&record)?;
    state.latest_diagnostic = Some(record);
    Ok(())
}

fn handle_ledger(
    record: PublishedTransitionRecord,
    capture: &mut impl RecordCapture,
    state: &mut DashboardState,
) -> Result<()> {
    capture.record_ledger(&record)?;
    state.latest_transition = Some(record);
    Ok(())
}

fn handle_boot_before_terminal<C>(
    record: DiagnosticRecord,
    mut capture: C,
    state: &mut DashboardState,
) -> Result<C>
where
    C: RecordCapture + CaptureFinalizer,
{
    match handle_diagnostic(record, &mut capture, state) {
        Ok(()) => Ok(capture),
        Err(boot_error) => {
            let result = preserve_primary_result(Err(boot_error), capture.finish_capture());
            match result {
                Err(error) => Err(error),
                Ok(()) => unreachable!("a boot capture error is always primary"),
            }
        }
    }
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

/// Require the Twin boot diagnostic before creating a run directory.
async fn require_boot_diagnostic(
    rx: &mut mpsc::UnboundedReceiver<DiagnosticRecord>,
    timeout: Duration,
) -> Result<DiagnosticRecord> {
    await_boot_diagnostic(rx, timeout)
        .await
        .ok_or_else(|| anyhow::anyhow!("timed out waiting for Twin boot diagnostic"))
}

async fn run_ui_loop(
    terminal: &mut DashboardTerminal,
    twin: &mut DigitalTwinRuntime,
    capture: &mut impl RecordCapture,
    state: &mut DashboardState,
) -> Result<()> {
    loop {
        drain_twin_emissions(
            &mut twin.diagnostic_rx,
            &mut twin.transition_rx,
            capture,
            state,
        )?;

        terminal.draw(|f| {
            render_frame(f, &state.latest_diagnostic, &state.latest_transition);
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

/// Dashboard trusts the twin: capture every emission durably before retaining it for display.
fn drain_twin_emissions(
    diag_rx: &mut mpsc::UnboundedReceiver<DiagnosticRecord>,
    trans_rx: &mut mpsc::Receiver<PublishedTransitionRecord>,
    capture: &mut impl RecordCapture,
    state: &mut DashboardState,
) -> Result<()> {
    while let Ok(record) = diag_rx.try_recv() {
        handle_diagnostic(record, capture, state)?;
    }
    while let Ok(record) = trans_rx.try_recv() {
        handle_ledger(record, capture, state)?;
    }
    Ok(())
}

fn final_drain_twin_emissions(
    diag_rx: &mut mpsc::UnboundedReceiver<DiagnosticRecord>,
    trans_rx: &mut mpsc::Receiver<PublishedTransitionRecord>,
    capture: &mut impl RecordCapture,
    state: &mut DashboardState,
) -> Result<()> {
    drain_twin_emissions(diag_rx, trans_rx, capture, state)
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
    let line_limit = panel_width.clamp(24, MAX_PANEL_LINE_CHARS);

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
            Line::from(Span::raw(format_field("Source", d.source, line_limit))),
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
                &format_elapsed(elapsed_since_session(t.recorded_at, t.session_started_at)),
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
        PublishedFsmState::ExtremeOperationWarning { entered_at } => {
            format!(
                "ExtremeOpWarn@{}ms",
                entered_at.duration_since_epoch().as_millis()
            )
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
    let session_started_at = latest_transition
        .as_ref()
        .map(|t| t.session_started_at)
        .or_else(|| latest_diagnostic.as_ref().map(|d| d.session_started_at));

    let session_label = session_started_at
        .map(format_unix_timestamp_short)
        .unwrap_or_else(|| "awaiting twin…".to_string());

    let twin_elapsed = latest_twin_elapsed(latest_diagnostic, latest_transition)
        .map(format_elapsed)
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
        (Some(d), Some(t)) => Some(
            d.elapsed_since_session()
                .max(elapsed_since_session(t.recorded_at, t.session_started_at)),
        ),
        (Some(d), None) => Some(d.elapsed_since_session()),
        (None, Some(t)) => Some(elapsed_since_session(t.recorded_at, t.session_started_at)),
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

fn format_unix_timestamp_short(timestamp: UnixTimestamp) -> String {
    let secs = timestamp.unix_seconds();
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
    use observation::RunReader;
    use std::cell::Cell;
    use std::rc::Rc;

    fn sample_boot_diagnostic() -> DiagnosticRecord {
        DiagnosticRecord {
            level: DiagnosticLevel::Info,
            source: "VirtualCarActor",
            message: "initializing".into(),
            session_started_at: UnixTimestamp::from_duration_since_epoch(Duration::new(
                1_700_000_000,
                0,
            )),
            recorded_at: UnixTimestamp::from_duration_since_epoch(Duration::new(
                1_700_000_000,
                50_000_000,
            )),
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
            session_started_at: UnixTimestamp::from_duration_since_epoch(Duration::from_nanos(1)),
            record_seq: 2,
            recorded_at: UnixTimestamp::from_duration_since_epoch(Duration::ZERO),
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

    /// Records how the handlers drove capture and lets a test force a failure.
    #[derive(Default)]
    struct FakeCapture {
        diagnostic_calls: usize,
        ledger_calls: usize,
        fail_diagnostic: bool,
        fail_ledger: bool,
    }

    impl RecordCapture for FakeCapture {
        fn record_diagnostic(&mut self, _record: &DiagnosticRecord) -> Result<()> {
            self.diagnostic_calls += 1;
            if self.fail_diagnostic {
                anyhow::bail!("forced diagnostic capture failure");
            }
            Ok(())
        }

        fn record_ledger(&mut self, _record: &PublishedTransitionRecord) -> Result<()> {
            self.ledger_calls += 1;
            if self.fail_ledger {
                anyhow::bail!("forced ledger capture failure");
            }
            Ok(())
        }
    }

    #[test]
    fn handle_diagnostic_captures_once_then_updates_state() {
        let mut capture = FakeCapture::default();
        let mut state = DashboardState::default();
        let record = sample_boot_diagnostic();

        handle_diagnostic(record.clone(), &mut capture, &mut state).unwrap();

        assert_eq!(capture.diagnostic_calls, 1);
        let retained = state
            .latest_diagnostic
            .expect("state updated after capture");
        assert_eq!(retained.message, record.message);
        assert_eq!(retained.recorded_at, record.recorded_at);
    }

    #[test]
    fn handle_ledger_captures_once_then_updates_state() {
        let mut capture = FakeCapture::default();
        let mut state = DashboardState::default();
        let record = sample_ledger_row();

        handle_ledger(record.clone(), &mut capture, &mut state).unwrap();

        assert_eq!(capture.ledger_calls, 1);
        assert_eq!(state.latest_transition, Some(record));
    }

    #[test]
    fn handle_diagnostic_error_leaves_previous_latest_unchanged() {
        let previous = sample_boot_diagnostic();
        let mut state = DashboardState {
            latest_diagnostic: Some(previous.clone()),
            latest_transition: None,
        };
        let mut capture = FakeCapture {
            fail_diagnostic: true,
            ..FakeCapture::default()
        };
        let mut newer = sample_boot_diagnostic();
        newer.message = "newer diagnostic that must not be retained".into();

        let result = handle_diagnostic(newer, &mut capture, &mut state);

        assert!(result.is_err());
        assert_eq!(capture.diagnostic_calls, 1);
        let retained = state
            .latest_diagnostic
            .expect("previous diagnostic must be retained on failure");
        assert_eq!(retained.message, previous.message);
    }

    #[test]
    fn handle_ledger_error_leaves_previous_latest_unchanged() {
        let previous = sample_ledger_row();
        let mut state = DashboardState {
            latest_diagnostic: None,
            latest_transition: Some(previous.clone()),
        };
        let mut capture = FakeCapture {
            fail_ledger: true,
            ..FakeCapture::default()
        };
        let mut newer = sample_ledger_row();
        newer.record_seq = 999;

        let result = handle_ledger(newer, &mut capture, &mut state);

        assert!(result.is_err());
        assert_eq!(capture.ledger_calls, 1);
        assert_eq!(state.latest_transition, Some(previous));
    }

    #[tokio::test]
    async fn boot_diagnostic_is_routed_through_handle_diagnostic() {
        let (tx, mut rx) = mpsc::unbounded_channel::<DiagnosticRecord>();
        let boot = sample_boot_diagnostic();
        tx.send(boot.clone()).unwrap();

        let received = await_boot_diagnostic(&mut rx, BOOT_DIAGNOSTIC_WAIT)
            .await
            .expect("boot diagnostic should be received");

        let mut capture = FakeCapture::default();
        let mut state = DashboardState::default();
        handle_diagnostic(received, &mut capture, &mut state).unwrap();

        assert_eq!(capture.diagnostic_calls, 1);
        let retained = state
            .latest_diagnostic
            .expect("boot diagnostic captured then retained");
        assert_eq!(retained.message, boot.message);
        assert_eq!(retained.source, boot.source);
    }

    #[derive(Debug)]
    struct FailingBootCapture {
        finish_calls: Rc<Cell<usize>>,
    }

    impl RecordCapture for FailingBootCapture {
        fn record_diagnostic(&mut self, _record: &DiagnosticRecord) -> Result<()> {
            anyhow::bail!("boot capture failed")
        }

        fn record_ledger(&mut self, _record: &PublishedTransitionRecord) -> Result<()> {
            Ok(())
        }
    }

    impl CaptureFinalizer for FailingBootCapture {
        fn finish_capture(self) -> Result<()> {
            self.finish_calls.set(self.finish_calls.get() + 1);
            anyhow::bail!("finish failed")
        }
    }

    #[test]
    fn final_drain_captures_records_queued_after_an_earlier_drain() {
        let (diag_tx, mut diag_rx) = mpsc::unbounded_channel::<DiagnosticRecord>();
        let (ledger_tx, mut ledger_rx) = mpsc::channel::<PublishedTransitionRecord>(4);
        let mut capture = FakeCapture::default();
        let mut state = DashboardState::default();

        drain_twin_emissions(&mut diag_rx, &mut ledger_rx, &mut capture, &mut state).unwrap();
        diag_tx.send(sample_boot_diagnostic()).unwrap();
        ledger_tx.try_send(sample_ledger_row()).unwrap();

        final_drain_twin_emissions(&mut diag_rx, &mut ledger_rx, &mut capture, &mut state).unwrap();

        assert_eq!(capture.diagnostic_calls, 1);
        assert_eq!(capture.ledger_calls, 1);
        assert!(state.latest_diagnostic.is_some());
        assert!(state.latest_transition.is_some());
    }

    #[test]
    fn production_drain_persists_records_readable_by_run_reader() {
        let temp = tempfile::tempdir().unwrap();
        let run_id = RunId::parse("00000000-0000-4000-8000-000000000006").unwrap();
        let boot = sample_boot_diagnostic();
        let metadata = RunMetadata::new(
            run_id.clone(),
            UnixTimestampV1::from_live(boot.recorded_at),
            UnixTimestampV1::from_live(boot.session_started_at),
            "x",
            None,
        );
        let mut capture = RunWriter::create(temp.path(), metadata).unwrap();
        let run_dir = capture.run_dir().to_path_buf();
        let (diag_tx, mut diag_rx) = mpsc::unbounded_channel::<DiagnosticRecord>();
        let (ledger_tx, mut ledger_rx) = mpsc::channel::<PublishedTransitionRecord>(4);
        diag_tx.send(boot).unwrap();
        ledger_tx.try_send(sample_ledger_row()).unwrap();
        let mut state = DashboardState::default();

        drain_twin_emissions(&mut diag_rx, &mut ledger_rx, &mut capture, &mut state).unwrap();
        capture.finish().unwrap();

        let stored = RunReader::open(run_dir).unwrap().load().unwrap();
        assert_eq!(stored.diagnostics.len(), 1);
        assert_eq!(stored.ledger.len(), 1);
        assert_eq!(stored.diagnostics[0].payload.message, "initializing");
        assert_eq!(stored.ledger[0].payload.record_seq, 1);
        assert_eq!(
            stored.manifest.session_started_at,
            stored.diagnostics[0].payload.session_started_at
        );
    }

    #[tokio::test]
    async fn boot_timeout_creates_no_run_directory() {
        let temp = tempfile::tempdir().unwrap();
        let (_tx, mut rx) = mpsc::unbounded_channel::<DiagnosticRecord>();
        let error = require_boot_diagnostic(&mut rx, Duration::from_millis(20))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("timed out"), "{error}");
        assert!(
            temp.path().read_dir().unwrap().next().is_none(),
            "observation parent must remain empty when boot times out"
        );
    }

    #[test]
    fn capture_finalization_preserves_boot_primary_and_surfaces_lone_finish_error() {
        let finish_calls = Rc::new(Cell::new(0));
        let capture = FailingBootCapture {
            finish_calls: Rc::clone(&finish_calls),
        };
        let mut state = DashboardState::default();

        let error =
            handle_boot_before_terminal(sample_boot_diagnostic(), capture, &mut state).unwrap_err();

        assert_eq!(finish_calls.get(), 1);
        assert_eq!(error.to_string(), "boot capture failed");
        assert!(state.latest_diagnostic.is_none());

        let combined = preserve_primary_result(Ok(()), Err(anyhow::anyhow!("finish failed")));
        assert_eq!(combined.unwrap_err().to_string(), "finish failed");
    }

    fn sample_ledger_row() -> PublishedTransitionRecord {
        PublishedTransitionRecord {
            car_identity: "x".into(),
            session_started_at: UnixTimestamp::from_duration_since_epoch(Duration::new(
                1_700_000_000,
                0,
            )),
            record_seq: 1,
            recorded_at: UnixTimestamp::from_duration_since_epoch(Duration::new(
                1_700_000_000,
                100_000_000,
            )),
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
                ack_pending_since: None,
            },
        }
    }
}
