use std::io;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::{execute, terminal};
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Gauge, Paragraph};
use ratatui::Terminal;
use ratatui::{backend::CrosstermBackend, prelude::Alignment};
use sysinfo::System;

fn main() -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, terminal::EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut system = System::new_all();
    system.refresh_cpu();
    system.refresh_memory();

    let tick_rate = Duration::from_millis(500);
    let mut last_tick = Instant::now();

    loop {
        if last_tick.elapsed() >= tick_rate {
            system.refresh_cpu();
            system.refresh_memory();
            last_tick = Instant::now();
        }

        let cpu_usage = system.global_cpu_info().cpu_usage();
        let total_memory = system.total_memory();
        let used_memory = system.used_memory();
        let memory_percent = if total_memory == 0 {
            0.0
        } else {
            (used_memory as f64 / total_memory as f64) * 100.0
        };

        terminal.draw(|frame| {
            let size = frame.size();
            let chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .margin(1)
                .split(size);

            let cpu_block = Block::default()
                .title("CPU")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan));
            let cpu_gauge = Gauge::default()
                .block(cpu_block)
                .gauge_style(Style::default().fg(Color::Green))
                .label(format!("{cpu_usage:.1}%"))
                .ratio((cpu_usage as f64 / 100.0).min(1.0));

            let memory_block = Block::default()
                .title("Memory")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Magenta));
            let memory_lines = vec![
                Line::from(vec![
                    Span::styled("Used: ", Style::default().fg(Color::Gray)),
                    Span::styled(
                        format!("{} / {}", format_bytes(used_memory), format_bytes(total_memory)),
                        Style::default().fg(Color::White),
                    ),
                ]),
                Line::from(vec![
                    Span::styled("Usage: ", Style::default().fg(Color::Gray)),
                    Span::styled(
                        format!("{memory_percent:.1}%"),
                        Style::default().fg(Color::White),
                    ),
                ]),
            ];
            let memory_paragraph = Paragraph::new(memory_lines)
                .block(memory_block)
                .alignment(Alignment::Left);

            frame.render_widget(cpu_gauge, chunks[0]);
            frame.render_widget(memory_paragraph, chunks[1]);
        })?;

        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                if key.code == KeyCode::Char('q') {
                    break;
                }
            }
        }
    }

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        terminal::DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    Ok(())
}

fn format_bytes(kib: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;

    let bytes = kib as f64 * 1024.0;
    if bytes >= GB {
        format!("{:.2} GiB", bytes / GB)
    } else if bytes >= MB {
        format!("{:.2} MiB", bytes / MB)
    } else if bytes >= KB {
        format!("{:.2} KiB", bytes / KB)
    } else {
        format!("{bytes:.0} B")
    }
}
