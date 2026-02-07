use std::io;
use std::time::{Duration, Instant};

use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::{execute, terminal};
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Gauge, Paragraph, Wrap};
use ratatui::{backend::CrosstermBackend, prelude::Alignment, Terminal};
use sysinfo::{DiskKind, Disks, System};

#[derive(Default)]
struct AppState {
    show_help: bool,
}

fn main() -> io::Result<()> {
    // Terminal setup
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    terminal::enable_raw_mode()?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut system = System::new_all();
    let mut disks = Disks::new_with_refreshed_list();

    // Initial refresh
    refresh(&mut system, &mut disks);

    let tick_rate = Duration::from_millis(500);
    let mut last_tick = Instant::now();

    let mut app = AppState::default();

    let res = run_app(&mut terminal, &mut system, &mut disks, &mut app, tick_rate, &mut last_tick);

    // Always restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    res
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    system: &mut System,
    disks: &mut Disks,
    app: &mut AppState,
    tick_rate: Duration,
    last_tick: &mut Instant,
) -> io::Result<()> {
    loop {
        if last_tick.elapsed() >= tick_rate {
            refresh(system, disks);
            *last_tick = Instant::now();
        }

        let cpu_usage = system.global_cpu_info().cpu_usage();
        let total_memory = system.total_memory();
        let used_memory = system.used_memory();
        let memory_percent = percent(used_memory, total_memory);

        let disk = pick_primary_disk(disks);
        let (disk_label, disk_used, disk_total, disk_percent) = match disk {
            Some(d) => {
                let total = d.total_space();
                let avail = d.available_space();
                let used = total.saturating_sub(avail);
                let pct = percent(used, total);
                (
                    format!("{}", d.mount_point().display()),
                    used,
                    total,
                    pct,
                )
            }
            None => ("(no disks)".to_string(), 0, 0, 0.0),
        };

        terminal.draw(|frame| {
            let size = frame.size();
            let rows = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1),
                    Constraint::Min(8),
                    Constraint::Length(if app.show_help { 5 } else { 1 }),
                ])
                .margin(1)
                .split(size);

            // Header
            let header = Paragraph::new(Line::from(vec![
                Span::styled(
                    "Ferromon",
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                ),
                Span::raw("  —  "),
                Span::styled(
                    "q",
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                ),
                Span::raw(": quit  "),
                Span::styled(
                    "r",
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                ),
                Span::raw(": refresh  "),
                Span::styled(
                    "h",
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                ),
                Span::raw(": help"),
            ]));
            frame.render_widget(header, rows[0]);

            // Main panels
            let panels = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(34),
                    Constraint::Percentage(33),
                    Constraint::Percentage(33),
                ])
                .split(rows[1]);

            // CPU
            let cpu_block = Block::default()
                .title("CPU")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan));
            let cpu_gauge = Gauge::default()
                .block(cpu_block)
                .gauge_style(Style::default().fg(color_for_pct(cpu_usage as f64)))
                .label(format!("{cpu_usage:.1}%"))
                .ratio((cpu_usage as f64 / 100.0).clamp(0.0, 1.0));
            frame.render_widget(cpu_gauge, panels[0]);

            // Memory
            let memory_block = Block::default()
                .title("Memory")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Magenta));
            let memory_lines = vec![
                Line::from(vec![
                    Span::styled("Used: ", Style::default().fg(Color::Gray)),
                    Span::styled(
                        format!(
                            "{} / {}",
                            format_bytes_kib(used_memory),
                            format_bytes_kib(total_memory)
                        ),
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
            frame.render_widget(memory_paragraph, panels[1]);

            // Disk
            let disk_block = Block::default()
                .title("Disk")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Green));
            let disk_lines = vec![
                Line::from(vec![
                    Span::styled("Mount: ", Style::default().fg(Color::Gray)),
                    Span::styled(disk_label, Style::default().fg(Color::White)),
                ]),
                Line::from(vec![
                    Span::styled("Used: ", Style::default().fg(Color::Gray)),
                    Span::styled(
                        format!("{} / {}", format_bytes(disk_used), format_bytes(disk_total)),
                        Style::default().fg(Color::White),
                    ),
                ]),
                Line::from(vec![
                    Span::styled("Usage: ", Style::default().fg(Color::Gray)),
                    Span::styled(
                        format!("{disk_percent:.1}%"),
                        Style::default().fg(Color::White),
                    ),
                ]),
            ];
            frame.render_widget(
                Paragraph::new(disk_lines).block(disk_block).alignment(Alignment::Left),
                panels[2],
            );

            // Footer/help
            if app.show_help {
                let help = Paragraph::new(vec![
                    Line::from("Controls:"),
                    Line::from("  q — quit"),
                    Line::from("  r — refresh now"),
                    Line::from("  h — toggle this help"),
                ])
                .block(Block::default().title("Help").borders(Borders::ALL))
                .wrap(Wrap { trim: true });
                frame.render_widget(help, rows[2]);
            } else {
                let footer = Paragraph::new(Line::from(vec![
                    Span::styled(
                        "Tip: ",
                        Style::default().fg(Color::Gray).add_modifier(Modifier::BOLD),
                    ),
                    Span::raw("press "),
                    Span::styled("h", Style::default().fg(Color::Yellow)),
                    Span::raw(" for help"),
                ]));
                frame.render_widget(footer, rows[2]);
            }
        })?;

        // Input
        if event::poll(Duration::from_millis(50))? {
            match event::read()? {
                Event::Key(key) => match key.code {
                    KeyCode::Char('q') => break,
                    KeyCode::Char('r') => {
                        refresh(system, disks);
                        *last_tick = Instant::now();
                    }
                    KeyCode::Char('h') => {
                        app.show_help = !app.show_help;
                    }
                    _ => {}
                },
                _ => {}
            }
        }
    }

    Ok(())
}

fn refresh(system: &mut System, disks: &mut Disks) {
    system.refresh_cpu();
    system.refresh_memory();
    disks.refresh();
}

fn percent(used: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        (used as f64 / total as f64) * 100.0
    }
}

fn color_for_pct(pct: f64) -> Color {
    if pct >= 90.0 {
        Color::Red
    } else if pct >= 75.0 {
        Color::Yellow
    } else {
        Color::Green
    }
}

fn pick_primary_disk(disks: &Disks) -> Option<&sysinfo::Disk> {
    // Prefer an actual physical-ish disk; otherwise take first.
    disks
        .iter()
        .find(|d| matches!(d.kind(), DiskKind::HDD | DiskKind::SSD))
        .or_else(|| disks.iter().next())
}

fn format_bytes_kib(kib: u64) -> String {
    // sysinfo memory uses KiB
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;

    let bytes = kib as f64 * 1024.0;
    if bytes >= GIB {
        format!("{:.2} GiB", bytes / GIB)
    } else if bytes >= MIB {
        format!("{:.2} MiB", bytes / MIB)
    } else if bytes >= KIB {
        format!("{:.2} KiB", bytes / KIB)
    } else {
        format!("{bytes:.0} B")
    }
}

fn format_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;

    let b = bytes as f64;
    if b >= GIB {
        format!("{:.2} GiB", b / GIB)
    } else if b >= MIB {
        format!("{:.2} MiB", b / MIB)
    } else if b >= KIB {
        format!("{:.2} KiB", b / KIB)
    } else {
        format!("{bytes} B")
    }
}
