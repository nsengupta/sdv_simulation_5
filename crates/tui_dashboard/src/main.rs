use crossterm::{
    event::{Event, EventStream, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use futures_util::StreamExt;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Terminal,
};
use serde::{Deserialize, Serialize};
use std::{io, time::Duration};
use tokio::time::interval;

/// Replay media player state machine
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackState {
    Start,
    Paused,
    Stopped,
}

/// The serialization structure matching your Digital Twin runtime ledger.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LedgerTick {
    pub tick: u64,
    pub source: String,
    pub headlamp_state: String,
    pub wiper_state: String,
    pub rob_pending_count: usize,
    pub barrier_queue_depth: usize,
    pub message_context: String,
}

/// Simulation of the internal state history buffer parsed from the log file
pub struct ReplayEngine {
    pub ticks: Vec<LedgerTick>,
    pub current_index: usize,
    pub playback_state: PlaybackState,
}

impl ReplayEngine {
    pub fn new(mock_data: Vec<LedgerTick>) -> Self {
        Self {
            ticks: mock_data,
            current_index: 0,
            playback_state: PlaybackState::Paused,
        }
    }

    pub fn next(&mut self) {
        if self.playback_state != PlaybackState::Stopped && self.current_index + 1 < self.ticks.len() {
            self.current_index += 1;
        }
    }

    pub fn prev(&mut self) {
        if self.playback_state != PlaybackState::Stopped && self.current_index > 0 {
            self.current_index -= 1;
        }
    }

    pub fn play(&mut self) {
        self.playback_state = PlaybackState::Start;
    }

    pub fn pause(&mut self) {
        self.playback_state = PlaybackState::Paused;
    }

    pub fn stop(&mut self) {
        self.playback_state = PlaybackState::Stopped;
        self.current_index = 0;
    }

    pub fn get_current(&self) -> Option<&LedgerTick> {
        if self.playback_state == PlaybackState::Stopped {
            None
        } else {
            self.ticks.get(self.current_index)
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Initialize Terminal UI environment
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // 2. Generate deterministic mockup ledger trace data
    let mock_ledger = generate_mock_ledger();
    let mut engine = ReplayEngine::new(mock_ledger);

    // 3. Configure event streams and async playback cadences
    let mut reader = EventStream::new();
    let mut ticker = interval(Duration::from_millis(200)); // 5Hz automatic replay rate

    loop {
        // Draw layout to screen
        terminal.draw(|f| ui_render(f, &engine))?;

        tokio::select! {
            // Handle cross-cutting terminal inputs asynchronously
            Some(Ok(event)) = reader.next() => {
                if let Event::Key(key) = event {
                    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                        break;
                    }
                    match key.code {
                        KeyCode::Char('q') => break,
                        KeyCode::Char(' ') => { // Space toggles Play/Pause
                            match engine.playback_state {
                                PlaybackState::Start => engine.pause(),
                                PlaybackState::Paused | PlaybackState::Stopped => engine.play(),
                            }
                        }
                        KeyCode::Right | KeyCode::Char('n') => engine.next(),
                        KeyCode::Left | KeyCode::Char('p') => engine.prev(),
                        KeyCode::Char('s') => engine.stop(),
                        _ => {}
                    }
                }
            }
            // Auto-advance cadence timer step when active
            _ = ticker.tick() => {
                if engine.playback_state == PlaybackState::Start {
                    engine.next();
                }
            }
        }
    }

    // 4. Graceful Cleanup and Restoration
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    Ok(())
}

fn ui_render(f: &mut ratatui::Frame, engine: &ReplayEngine) {
    // Partition interface into Title, Body Split, and Status Footer
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(10),
            Constraint::Length(3),
        ])
        .split(f.size());

    // --- Pane 1: Global Media Controls Header ---
    let state_str = format!("{:?}", engine.playback_state).to_uppercase();
    let header_text = format!(
        " SDV Replay Dashboard | Mode: {} | Frame: {}/{}",
        state_str,
        if engine.playback_state == PlaybackState::Stopped { 0 } else { engine.current_index + 1 },
        engine.ticks.len()
    );
    let header_block = Block::default()
        .borders(Borders::ALL)
        .title(" System Core Status ")
        .style(Style::default().fg(Color::Cyan));
    let header = Paragraph::new(header_text)
        .block(header_block)
        .style(Style::default().add_modifier(Modifier::BOLD));
    f.render_widget(header, chunks[0]);

    // --- Pane 2: Dual Split Columns (Telemetry and Ledger Auditing) ---
    let body_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(chunks[1]);

    if let Some(tick) = engine.get_current() {
        // Left Panel: Twinlet Actuation Status
        let twinlet_info = vec![
            Line::from(vec![
                Span::styled("Digital Twin Tick: ", Style::default().fg(Color::DarkGray)),
                Span::styled(tick.tick.to_string(), Style::default().fg(Color::Yellow)),
            ]),
            Line::from(vec![
                Span::styled("Headlamp Actor State: ", Style::default().fg(Color::White)),
                Span::styled(&tick.headlamp_state, Style::default().fg(if tick.headlamp_state.contains("ACK") { Color::Green } else { Color::LightRed })),
            ]),
            Line::from(vec![
                Span::styled("Wiper Actor State:    ", Style::default().fg(Color::White)),
                Span::styled(&tick.wiper_state, Style::default().fg(Color::Magenta)),
            ]),
        ];
        let twinlet_block = Block::default().borders(Borders::ALL).title(" Actuation & Twins State ");
        f.render_widget(Paragraph::new(twinlet_info).block(twinlet_block), body_chunks[0]);

        // Right Panel: Distributed Ledger & TurnBarrier Reorder Buffer Metrics
        let ledger_info = vec![
            Line::from(vec![
                Span::styled("Update Origin Source: ", Style::default().fg(Color::White)),
                Span::styled(&tick.source, Style::default().fg(Color::Blue)),
            ]),
            Line::from(vec![
                Span::styled("ROB Pending Count:    ", Style::default().fg(Color::White)),
                Span::styled(tick.rob_pending_count.to_string(), Style::default().fg(if tick.rob_pending_count > 0 { Color::LightRed } else { Color::Green })),
            ]),
            Line::from(vec![
                Span::styled("Barrier Queue Depth:  ", Style::default().fg(Color::White)),
                Span::styled(tick.barrier_queue_depth.to_string(), Style::default().fg(Color::LightCyan)),
            ]),
            Line::from(""),
            Line::from(Span::styled("Causal Context Log:", Style::default().add_modifier(Modifier::UNDERLINED))),
            Line::from(Span::styled(&tick.message_context, Style::default().fg(Color::LightYellow))),
        ];
        let ledger_block = Block::default().borders(Borders::ALL).title(" Causal Consistency Audit Ledger ");
        f.render_widget(Paragraph::new(ledger_info).block(ledger_block).wrap(Wrap { trim: true }), body_chunks[1]);
    } else {
        // Render Empty/Stopped Viewport
        let empty_block = Block::default().borders(Borders::ALL).title(" System Offline ");
        f.render_widget(Paragraph::new("Playback stopped. Press [Space] to start simulation replay loop.").block(empty_block), chunks[1]);
    }

    // --- Pane 3: System Legend / Key Bindings ---
    let footer_text = "[Space] Play/Pause  |  [n/Right] Next Frame  |  [p/Left] Prev Frame  |  [s] Stop  |  [q] Quit";
    let footer_block = Block::default().borders(Borders::ALL).title(" Interaction Keys ");
    let footer = Paragraph::new(footer_text)
        .block(footer_block)
        .style(Style::default().fg(Color::DarkGray));
    f.render_widget(footer, chunks[2]);
}

/// Helper generating deterministic JSONL trace payloads mapping state execution changes.
fn generate_mock_ledger() -> Vec<LedgerTick> {
    vec![
        LedgerTick {
            tick: 100,
            source: "WiperActor".into(),
            headlamp_state: "CAN_OFF_IDLE".into(),
            wiper_state: "SLOW_SWEEP".into(),
            rob_pending_count: 0,
            barrier_queue_depth: 0,
            message_context: "Immediate transition for WiperActor. Reorder Buffer bypassed cleanly.".into(),
        },
        LedgerTick {
            tick: 101,
            source: "HeadlampActor".into(),
            headlamp_state: "TX_CAN_HIGH_BEAM_REQ".into(),
            wiper_state: "SLOW_SWEEP".into(),
            rob_pending_count: 1,
            barrier_queue_depth: 1,
            message_context: "Out-of-order Tell-Back captured. Headlamp state locked waiting for explicit hardware CAN ACK loop.".into(),
        },
        LedgerTick {
            tick: 102,
            source: "HeadlampActor".into(),
            headlamp_state: "ACK_RECEIVED_HIGH_BEAM".into(),
            wiper_state: "SLOW_SWEEP".into(),
            rob_pending_count: 0,
            barrier_queue_depth: 0,
            message_context: "Hardware ACK match resolved in TurnBarrier queue. Causal sequence completely committed to centralized ledger.".into(),
        },
        LedgerTick {
            tick: 103,
            source: "WiperActor".into(),
            headlamp_state: "ACK_RECEIVED_HIGH_BEAM".into(),
            wiper_state: "OFF".into(),
            rob_pending_count: 0,
            barrier_queue_depth: 0,
            message_context: "Immediate driver step down requested. System remains stable and fully aligned.".into(),
        },
    ]
}
