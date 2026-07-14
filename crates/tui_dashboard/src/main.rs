//! TUI Dashboard — connects to the live Digital Twin via [`TwinRuntimeBuilder`].
//!
//! Creates diagnostic and transition channels, wires them through the builder,
//! then renders live data in a split-pane terminal UI.

use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::ExecutableCommand;
use gateway::gateway_runtime::TwinRuntimeBuilder;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::{Frame, Terminal};
use tokio::sync::mpsc;

use common::DiagnosticRecord;
use common::facade::PublishedTransitionRecord;

const VIRTUAL_CAR_IDENTITY: &str = "My-Opel-Corsa-1.4-GSi";

#[tokio::main]
async fn main() -> Result<()> {
    // Create channels — caller keeps receivers, builder gets senders.
    let (diag_tx, mut diag_rx) = mpsc::unbounded_channel::<DiagnosticRecord>();
    let (trans_tx, mut trans_rx) = mpsc::channel::<PublishedTransitionRecord>(256);

    // Build and install the twin runtime.
    let mut builder = TwinRuntimeBuilder::new()
        .with_car_identity(VIRTUAL_CAR_IDENTITY)
        .with_can_interface(gateway::gateway_runtime::DEFAULT_CAN_INTERFACE)
        .with_diagnostic_channel(diag_tx)
        .with_transition_channel(trans_tx);

    let (controller, _opts) = builder.install_controller().await?;
    let _runtime_handle = builder.spawn_runtime(controller)?;

    // Track latest records for display.
    let mut latest_diagnostic: Option<DiagnosticRecord> = None;
    let mut latest_transition: Option<PublishedTransitionRecord> = None;

    // Terminal setup.
    enable_raw_mode()?;
    let mut stderr = std::io::stderr(); // ratatui writes to stderr, leaving stdout free
    crossterm::execute!(stderr, EnterAlternateScreen)?;
    let mut terminal = Terminal::new(ratatui::backend::CrosstermBackend::new(stderr))?;

    // UI event loop.
    let res = run_ui_loop(&mut terminal, &mut diag_rx, &mut trans_rx, &mut latest_diagnostic, &mut latest_transition).await;

    // Cleanup.
    disable_raw_mode()?;
    terminal.backend_mut().execute(LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    if let Err(e) = res {
        eprintln!("Dashboard error: {e:?}");
    }
    Ok(())
}

async fn run_ui_loop(
    terminal: &mut Terminal<ratatui::backend::CrosstermBackend<std::io::Stderr>>,
    diag_rx: &mut mpsc::UnboundedReceiver<DiagnosticRecord>,
    trans_rx: &mut mpsc::Receiver<PublishedTransitionRecord>,
    latest_diagnostic: &mut Option<DiagnosticRecord>,
    latest_transition: &mut Option<PublishedTransitionRecord>,
) -> Result<()> {
    loop {
        // Drain channels (non-blocking).
        while let Ok(record) = diag_rx.try_recv() {
            *latest_diagnostic = Some(record);
        }
        while let Ok(record) = trans_rx.try_recv() {
            *latest_transition = Some(record);
        }

        // Render frame.
        terminal.draw(|f| render_frame(f, latest_diagnostic, latest_transition))?;

        // Check for quit key (non-blocking, 50 ms timeout).
        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                if key.code == KeyCode::Char('q') || key.code == KeyCode::Esc {
                    break;
                }
            }
        }
    }
    Ok(())
}

fn render_frame(
    f: &mut Frame,
    latest_diagnostic: &Option<DiagnosticRecord>,
    latest_transition: &Option<PublishedTransitionRecord>,
) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(f.size());

    // Left panel: diagnostic record.
    let diag_text = if let Some(d) = latest_diagnostic {
        vec![
            Line::from(Span::raw(format!("Level:    {:?}", d.level))),
            Line::from(Span::raw(format!("Source:   {}", d.source))),
            Line::from(Span::raw(format!("Message:  {}", d.message))),
            Line::from(Span::raw(format!(
                "Timestamp: {}",
                d.timestamp_utc_nanos
            ))),
        ]
    } else {
        vec![Line::from(Span::raw("(awaiting diagnostic record...)"))]
    };
    let diag_block = Block::default()
        .title(" Diagnostic ")
        .borders(Borders::ALL)
        .style(Style::default().fg(Color::Cyan));
    let diag_para = Paragraph::new(diag_text)
        .block(diag_block)
        .wrap(Wrap { trim: false });
    f.render_widget(diag_para, chunks[0]);

    // Right panel: transition record.
    let trans_text = if let Some(t) = latest_transition {
        vec![
            Line::from(Span::raw(format!("Seq:        {}", t.record_seq))),
            Line::from(Span::raw(format!("Event:      {:?}", t.event))),
            Line::from(Span::raw(format!("Old state:  {:?}", t.old_state))),
            Line::from(Span::raw(format!("Next state: {:?}", t.next_state))),
            Line::from(Span::raw(format!("Actions:    {}", t.actions.len()))),
        ]
    } else {
        vec![Line::from(Span::raw("(awaiting transition record...)"))]
    };
    let trans_block = Block::default()
        .title(" Transition ")
        .borders(Borders::ALL)
        .style(Style::default().fg(Color::Green));
    let trans_para = Paragraph::new(trans_text)
        .block(trans_block)
        .wrap(Wrap { trim: false });
    f.render_widget(trans_para, chunks[1]);
}
