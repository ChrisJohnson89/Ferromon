use std::cmp::Reverse;
use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind,
};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::{execute, terminal};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Gauge, Paragraph, Row, Table, Wrap};
use ratatui::{backend::CrosstermBackend, prelude::Alignment, Terminal};
use sysinfo::{Disks, Process, ProcessRefreshKind, RefreshKind, System};
use walkdir::WalkDir;

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Screen {
    #[default]
    Dashboard,
    Processes,
    DiskDive,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum ProcSort {
    #[default]
    Cpu,
    Mem,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum DiskTarget {
    #[default]
    Var,
    Home,
    Root,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum DashDirTarget {
    #[default]
    Cwd,
    Var,
}

#[derive(Default)]
struct AppState {
    screen: Screen,
    show_help: bool,

    proc_sort: ProcSort,
    proc_scroll: u16,

    disk_target: DiskTarget,
    disk_scroll: u16,
    disk_scan: DiskScan,

    // Dashboard caches (quick overview)
    dash_dir_target: DashDirTarget,
    dash_dir_sizes: Vec<String>,
    dash_top_cpu: Vec<String>,
    dash_top_mem: Vec<String>,
    dash_last_proc_at: Option<Instant>,
    dash_last_fs_at: Option<Instant>,
    dash_show_all_mounts: bool,
    footer_tip_idx: u8,
}

#[derive(Clone, Default)]
struct DiskScan {
    inner: Arc<Mutex<DiskScanState>>,
}

#[derive(Default)]
struct Args {
    tick_ms: u64,
    no_mouse: bool,
    show_help: bool,
    show_version: bool,
}

fn parse_args() -> Args {
    let mut tick_ms: u64 = 500;
    let mut no_mouse = false;
    let mut show_help = false;
    let mut show_version = false;

    let argv: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < argv.len() {
        let a = argv[i].as_str();
        match a {
            "-h" | "--help" => {
                show_help = true;
            }
            "-V" | "--version" => {
                show_version = true;
            }
            "--no-mouse" => {
                no_mouse = true;
            }
            "--tick-ms" => {
                if i + 1 >= argv.len() {
                    show_help = true;
                } else if let Ok(v) = argv[i + 1].parse::<u64>() {
                    tick_ms = v.clamp(50, 5000);
                    i += 1;
                } else {
                    show_help = true;
                }
            }
            _ if a.starts_with("--tick-ms=") => {
                if let Some(v) = a.split('=').nth(1) {
                    if let Ok(v) = v.parse::<u64>() {
                        tick_ms = v.clamp(50, 5000);
                    } else {
                        show_help = true;
                    }
                }
            }
            _ => {
                // unknown flag
                show_help = true;
            }
        }
        i += 1;
    }

    Args {
        tick_ms,
        no_mouse,
        show_help,
        show_version,
    }
}

fn print_cli_help() {
    println!("ferro {VERSION}");
    println!(
        "
USAGE:
  ferro [--tick-ms <ms>]
"
    );
    println!("OPTIONS:");
    println!("  --tick-ms <ms>   UI refresh tick (50..5000). Default: 500");
    println!("  --no-mouse       Disable mouse capture (useful in tmux/SSH)");
    println!("  -h, --help       Show help");
    println!("  -V, --version    Show version");
    println!(
        "
KEYS (in-app):
  q quit · ? help · Esc back · p processes · d disk dive · r refresh"
    );
}

#[derive(Default)]
struct DiskScanState {
    running: bool,
    last_target: Option<PathBuf>,
    last_started_at: Option<std::time::SystemTime>,
    last_finished_at: Option<std::time::SystemTime>,
    progress: String,
    results: Vec<(String, u64)>, // (dir, bytes)
    error: Option<String>,
}

fn main() -> io::Result<()> {
    let args = parse_args();
    if args.show_version {
        println!("{VERSION}");
        return Ok(());
    }
    if args.show_help {
        print_cli_help();
        return Ok(());
    }

    // Terminal setup
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    if args.no_mouse {
        execute!(stdout, EnterAlternateScreen)?;
    } else {
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    }
    terminal::enable_raw_mode()?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Keep dashboard refresh light, but allow process refresh when needed.
    let refresh_kind = RefreshKind::new()
        .with_cpu(sysinfo::CpuRefreshKind::everything())
        .with_memory(sysinfo::MemoryRefreshKind::everything())
        .with_processes(ProcessRefreshKind::everything());
    let mut system = System::new_with_specifics(refresh_kind);

    let mut disks = Disks::new_with_refreshed_list();

    refresh(&mut system, &mut disks, true);

    let tick_rate = Duration::from_millis(args.tick_ms);
    let mut last_tick = Instant::now();

    let mut app = AppState::default();

    let res = run_app(
        &mut terminal,
        &mut system,
        &mut disks,
        &mut app,
        tick_rate,
        &mut last_tick,
    );

    // Always restore terminal
    disable_raw_mode()?;
    if args.no_mouse {
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    } else {
        execute!(
            terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        )?;
    }
    terminal.show_cursor()?;

    res
}

fn render_too_small(frame: &mut ratatui::Frame, area: Rect) {
    let msg = vec![
        Line::from("Ferromon"),
        Line::from(""),
        Line::from("Terminal too small."),
        Line::from("Resize and try again."),
        Line::from(""),
        Line::from("Tip: you can also run: ferro --help"),
    ];

    frame.render_widget(
        Paragraph::new(msg)
            .alignment(Alignment::Center)
            .block(Block::default().title("Ferromon").borders(Borders::ALL)),
        area,
    );
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    system: &mut System,
    disks: &mut Disks,
    app: &mut AppState,
    tick_rate: Duration,
    last_tick: &mut Instant,
) -> io::Result<()> {
    // Keep the dashboard cheap: refresh processes + fs scan on a slower cadence.
    let dash_proc_every = Duration::from_secs(3);
    let mut tip_clock = Instant::now();

    loop {
        // Refresh data (keep it cheap; process refresh only when on the processes screen)
        if last_tick.elapsed() >= tick_rate {
            let refresh_processes = if matches!(app.screen, Screen::Processes) {
                true
            } else if matches!(app.screen, Screen::Dashboard) {
                // Only refresh process table occasionally; we just need top-N.
                match app.dash_last_proc_at {
                    Some(t) => t.elapsed() >= dash_proc_every,
                    None => true,
                }
            } else {
                false
            };
            refresh(system, disks, refresh_processes);
            if matches!(app.screen, Screen::Dashboard) && refresh_processes {
                // reuse this timestamp for both proc+fs scan cadence
                app.dash_last_proc_at = Some(Instant::now());
            }
            *last_tick = Instant::now();

            if tip_clock.elapsed() >= Duration::from_secs(12) {
                app.footer_tip_idx = app.footer_tip_idx.wrapping_add(1);
                tip_clock = Instant::now();
            }
        }

        let vm = snapshot(system, disks, app.dash_show_all_mounts);

        terminal.draw(|frame| {
            let size = frame.size();
            let rows = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1),
                    Constraint::Min(8),
                    Constraint::Length(if app.show_help { 7 } else { 1 }),
                ])
                .margin(1)
                .split(size);

            // Header
            frame.render_widget(render_header(app), rows[0]);

            // If terminal is too small, render a friendly message instead of a broken layout.
            if rows[1].width < 80 || rows[1].height < 14 {
                render_too_small(frame, rows[1]);
                // Footer/help still renders below.
                return;
            }

            // Main
            match app.screen {
                Screen::Dashboard => render_dashboard(frame, rows[1], &vm, app, system),
                Screen::Processes => render_processes(frame, rows[1], app, system),
                Screen::DiskDive => render_disk_dive(frame, rows[1], app),
            }

            // Footer/help
            if app.show_help {
                frame.render_widget(render_help(app), rows[2]);
            } else {
                frame.render_widget(render_footer(app), rows[2]);
            }
        })?;

        // Input
        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                // Avoid key-repeat spam on some terminals
                if key.kind != KeyEventKind::Press {
                    continue;
                }

                match key.code {
                    KeyCode::Char('q') => break,
                    KeyCode::Char('?') => app.show_help = !app.show_help,
                    KeyCode::Esc => {
                        app.show_help = false;
                        app.screen = Screen::Dashboard;
                    }
                    KeyCode::Char('p') => {
                        app.show_help = false;
                        app.screen = Screen::Processes;
                    }
                    KeyCode::Char('d') => {
                        app.show_help = false;
                        app.screen = Screen::DiskDive;
                    }
                    KeyCode::Char('r') => {
                        // manual refresh, including processes if currently viewing them
                        let refresh_processes = if matches!(app.screen, Screen::Processes) {
                            true
                        } else if matches!(app.screen, Screen::Dashboard) {
                            // Only refresh process table occasionally; we just need top-N.
                            match app.dash_last_proc_at {
                                Some(t) => t.elapsed() >= dash_proc_every,
                                None => true,
                            }
                        } else {
                            false
                        };
                        refresh(system, disks, refresh_processes);
                        if matches!(app.screen, Screen::Dashboard) && refresh_processes {
                            // reuse this timestamp for both proc+fs scan cadence
                            app.dash_last_proc_at = Some(Instant::now());
                        }
                        *last_tick = Instant::now();

                        if tip_clock.elapsed() >= Duration::from_secs(12) {
                            app.footer_tip_idx = app.footer_tip_idx.wrapping_add(1);
                            tip_clock = Instant::now();
                        }
                    }

                    // Processes + DiskDive share Tab for mode/target.
                    KeyCode::Up => {
                        if matches!(app.screen, Screen::Processes) {
                            app.proc_scroll = app.proc_scroll.saturating_sub(1);
                        } else if matches!(app.screen, Screen::DiskDive) {
                            app.disk_scroll = app.disk_scroll.saturating_sub(1);
                        }
                    }
                    KeyCode::Down => {
                        if matches!(app.screen, Screen::Processes) {
                            app.proc_scroll = app.proc_scroll.saturating_add(1);
                        } else if matches!(app.screen, Screen::DiskDive) {
                            app.disk_scroll = app.disk_scroll.saturating_add(1);
                        }
                    }

                    // Tab is contextual.
                    KeyCode::Tab => {
                        if matches!(app.screen, Screen::Dashboard) {
                            app.dash_dir_target = match app.dash_dir_target {
                                DashDirTarget::Cwd => DashDirTarget::Var,
                                DashDirTarget::Var => DashDirTarget::Cwd,
                            };
                            // Force refresh of quick scan.
                            app.dash_last_fs_at = None;
                        } else if matches!(app.screen, Screen::DiskDive) {
                            app.disk_target = match app.disk_target {
                                DiskTarget::Var => DiskTarget::Home,
                                DiskTarget::Home => DiskTarget::Root,
                                DiskTarget::Root => DiskTarget::Var,
                            };
                            app.disk_scroll = 0;
                        } else if matches!(app.screen, Screen::Processes) {
                            app.proc_sort = match app.proc_sort {
                                ProcSort::Cpu => ProcSort::Mem,
                                ProcSort::Mem => ProcSort::Cpu,
                            };
                            app.proc_scroll = 0;
                        }
                    }
                    KeyCode::Char('s') => {
                        if matches!(app.screen, Screen::DiskDive) {
                            start_disk_scan(app);
                        }
                    }
                    KeyCode::Char('f') => {
                        if matches!(app.screen, Screen::Dashboard) {
                            app.dash_show_all_mounts = !app.dash_show_all_mounts;
                        }
                    }

                    _ => {}
                }
            }
        }
    }

    Ok(())
}

#[derive(Clone)]
struct DiskRow {
    fs: String,
    size: u64,
    used: u64,
    avail: u64,
    use_pct: f64,
    mount: String,
}

#[derive(Clone)]
struct VmSnapshot {
    cpu_usage: f32,
    cpu_cores: usize,
    total_memory: u64,
    used_memory: u64,
    memory_percent: f64,

    disks_table: Vec<DiskRow>,
}

fn snapshot(system: &System, disks: &Disks, show_all_mounts: bool) -> VmSnapshot {
    let cpu_usage = system.global_cpu_info().cpu_usage();
    let cpu_cores = system.cpus().len();
    // sysinfo reports memory in bytes
    let total_memory = system.total_memory();
    let used_memory = system.used_memory();
    let memory_percent = percent(used_memory, total_memory);

    let disks_table = disks_table_filtered(disks, 7, show_all_mounts);

    VmSnapshot {
        cpu_usage,
        cpu_cores,
        total_memory,
        used_memory,
        memory_percent,
        disks_table,
    }
}

fn render_header(app: &AppState) -> Paragraph<'static> {
    let (screen_name, screen_hint) = match app.screen {
        Screen::Dashboard => ("Dashboard", "p: processes  d: disk  f: filter  Tab: dir"),
        Screen::Processes => ("Processes", "Tab: sort CPU/Mem  Esc: back"),
        Screen::DiskDive => ("Disk dive", "s: scan  Tab: target  Esc: back"),
    };

    Paragraph::new(Line::from(vec![
        Span::styled(
            "Ferromon",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  —  "),
        Span::styled(screen_name, Style::default().fg(Color::White)),
        Span::raw("  •  "),
        Span::styled(
            screen_hint,
            Style::default()
                .fg(Color::Gray)
                .add_modifier(Modifier::ITALIC),
        ),
        Span::raw("  •  "),
        Span::styled("q", Style::default().fg(Color::Yellow)),
        Span::raw(": quit  "),
        Span::styled("?", Style::default().fg(Color::Yellow)),
        Span::raw(": help"),
    ]))
}

fn render_footer(app: &AppState) -> Paragraph<'static> {
    let tips_dashboard = [
        "Tab: toggle dir target (CWD ↔ /var)",
        "f: toggle mount filter (filtered ↔ all)",
        "p: processes · d: disk dive",
        "r: refresh now · ?: help",
        "Esc: back to dashboard",
    ];

    let tips_processes = ["Tab: sort CPU ↔ Mem", "↑/↓: scroll · q: quit", "Esc: back"];

    let tips_disk = [
        "s: scan (on-demand)",
        "Tab: change target (/var ↔ home ↔ /)",
        "↑/↓: scroll · Esc: back",
    ];

    let (label, tip) = match app.screen {
        Screen::Dashboard => (
            "Tip",
            tips_dashboard[(app.footer_tip_idx as usize) % tips_dashboard.len()],
        ),
        Screen::Processes => (
            "Tip",
            tips_processes[(app.footer_tip_idx as usize) % tips_processes.len()],
        ),
        Screen::DiskDive => (
            "Tip",
            tips_disk[(app.footer_tip_idx as usize) % tips_disk.len()],
        ),
    };

    Paragraph::new(Line::from(vec![
        Span::styled(
            format!("{label}: "),
            Style::default()
                .fg(Color::Gray)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(tip),
    ]))
}

fn render_help(app: &AppState) -> Paragraph<'static> {
    let mut lines = vec![
        Line::from("Global:"),
        Line::from("  q — quit"),
        Line::from("  ? — toggle help"),
        Line::from("  Esc — back to dashboard"),
        Line::from("  r — refresh now"),
        Line::from(""),
    ];

    match app.screen {
        Screen::Dashboard => {
            lines.push(Line::from("Dashboard:"));
            lines.push(Line::from("  p — processes"));
            lines.push(Line::from("  d — disk dive"));
            lines.push(Line::from("  f — toggle mount filter (filtered ↔ all)"));
            lines.push(Line::from("  Tab — toggle dir target (CWD ↔ /var)"));
        }
        Screen::Processes => {
            lines.push(Line::from("Processes:"));
            lines.push(Line::from("  Tab — toggle CPU/Mem list"));
            lines.push(Line::from("  ↑/↓ — scroll"));
        }
        Screen::DiskDive => {
            lines.push(Line::from("Disk dive:"));
            lines.push(Line::from("  s — start scan"));
            lines.push(Line::from("  Tab — change target (/var ↔ home ↔ /)"));
            lines.push(Line::from("  ↑/↓ — scroll"));
        }
    }

    Paragraph::new(lines)
        .block(Block::default().title("Help").borders(Borders::ALL))
        .wrap(Wrap { trim: true })
}

fn render_dashboard(
    frame: &mut ratatui::Frame,
    area: Rect,
    vm: &VmSnapshot,
    app: &mut AppState,
    system: &System,
) {
    let panels = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(34),
            Constraint::Percentage(33),
            Constraint::Percentage(33),
        ])
        .split(area);

    // --- dashboard quick-overview cache ---
    // Keep this screen cheap: do tiny scans occasionally, not every frame.
    let now = Instant::now();
    let need_fs = match app.dash_last_fs_at {
        Some(t) => t.elapsed() >= Duration::from_secs(5),
        None => true,
    };

    if need_fs {
        app.dash_top_cpu = format_top_processes(system, ProcSort::Cpu, 3);
        app.dash_top_mem = format_top_processes(system, ProcSort::Mem, 3);
        let (label, path) = dash_target_path(app.dash_dir_target);
        app.dash_dir_sizes = scan_dir_quick(&path, 6);
        // stash label in first line of the list for display
        let prefix = match app.dash_dir_target {
            DashDirTarget::Cwd => "CWD",
            DashDirTarget::Var => "/var",
        };
        if !app.dash_dir_sizes.is_empty() {
            app.dash_dir_sizes
                .insert(0, format!("{}: {}", prefix, label));
        } else {
            app.dash_dir_sizes = vec![format!("{}: {}", prefix, label), "(no entries)".to_string()];
        }
        app.dash_last_fs_at = Some(now);
    }

    // CPU
    let cpu_block = Block::default()
        .title("CPU")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    frame.render_widget(cpu_block.clone(), panels[0]);

    let cpu_inner = cpu_block.inner(panels[0]);
    let cpu_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Length(6),
            Constraint::Min(0),
        ])
        .split(cpu_inner);

    let cpu_lines = vec![
        Line::from(vec![
            Span::styled("Used: ", Style::default().fg(Color::Gray)),
            Span::styled(
                format!("{:.1}%", vm.cpu_usage),
                Style::default().fg(Color::White),
            ),
        ]),
        Line::from(vec![
            Span::styled("Cores: ", Style::default().fg(Color::Gray)),
            Span::styled(
                format!("{}", vm.cpu_cores),
                Style::default().fg(Color::White),
            ),
        ]),
    ];
    let cpu_paragraph = Paragraph::new(cpu_lines).alignment(Alignment::Left);
    frame.render_widget(cpu_paragraph, cpu_chunks[0]);

    let cpu_gauge = Gauge::default()
        .gauge_style(Style::default().fg(color_for_pct(vm.cpu_usage as f64)))
        .ratio(((vm.cpu_usage as f64) / 100.0).clamp(0.0, 1.0));
    frame.render_widget(cpu_gauge, cpu_chunks[2]);

    let cpu_bottom = if app.dash_top_cpu.is_empty() {
        vec![Line::from(Span::styled(
            "Top CPU: (no data)",
            Style::default().fg(Color::Gray),
        ))]
    } else {
        let mut lines = vec![Line::from(vec![
            Span::styled(
                "Top CPU",
                Style::default()
                    .fg(Color::Gray)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(": "),
        ])];
        for (i, row) in app.dash_top_cpu.iter().enumerate() {
            lines.push(Line::from(vec![
                Span::styled(format!("{}. ", i + 1), Style::default().fg(Color::Gray)),
                Span::raw(row.clone()),
            ]));
        }
        lines
    };
    frame.render_widget(
        Paragraph::new(cpu_bottom).alignment(Alignment::Left),
        cpu_chunks[1],
    );

    // Memory
    let memory_block = Block::default()
        .title("Memory")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Magenta));
    frame.render_widget(memory_block.clone(), panels[1]);

    let memory_inner = memory_block.inner(panels[1]);
    let memory_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(6),
            Constraint::Min(0),
        ])
        .split(memory_inner);

    let memory_lines = vec![
        Line::from(vec![
            Span::styled("Used: ", Style::default().fg(Color::Gray)),
            Span::styled(
                format!(
                    "{} / {}",
                    format_bytes(vm.used_memory),
                    format_bytes(vm.total_memory)
                ),
                Style::default().fg(Color::White),
            ),
        ]),
        Line::from(vec![
            Span::styled("Usage: ", Style::default().fg(Color::Gray)),
            Span::styled(
                format!("{:.1}%", vm.memory_percent),
                Style::default().fg(Color::White),
            ),
        ]),
    ];
    let memory_paragraph = Paragraph::new(memory_lines).alignment(Alignment::Left);
    frame.render_widget(memory_paragraph, memory_chunks[0]);

    let memory_gauge = Gauge::default()
        .gauge_style(Style::default().fg(color_for_pct(vm.memory_percent)))
        .ratio((vm.memory_percent / 100.0).clamp(0.0, 1.0));
    frame.render_widget(memory_gauge, memory_chunks[2]);

    let mem_bottom = if app.dash_top_mem.is_empty() {
        vec![Line::from(Span::styled(
            "Top MEM: (no data)",
            Style::default().fg(Color::Gray),
        ))]
    } else {
        let mut lines = vec![Line::from(vec![
            Span::styled(
                "Top MEM",
                Style::default()
                    .fg(Color::Gray)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(": "),
        ])];
        for (i, row) in app.dash_top_mem.iter().enumerate() {
            lines.push(Line::from(vec![
                Span::styled(format!("{}. ", i + 1), Style::default().fg(Color::Gray)),
                Span::raw(row.clone()),
            ]));
        }
        lines
    };
    frame.render_widget(
        Paragraph::new(mem_bottom).alignment(Alignment::Left),
        memory_chunks[1],
    );

    // Disk
    let disk_title = if app.dash_show_all_mounts {
        "Disk (all mounts)"
    } else {
        "Disk (filtered)"
    };
    let disk_block = Block::default()
        .title(disk_title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Green));
    frame.render_widget(disk_block.clone(), panels[2]);

    let disk_inner = disk_block.inner(panels[2]);
    let disk_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(4), Constraint::Min(0)])
        .split(disk_inner);

    let df_rows = vm.disks_table.iter().map(|r| {
        Row::new(vec![
            Cell::from(trim_to(&r.fs, 14)),
            Cell::from(format_bytes(r.size)),
            Cell::from(format_bytes(r.used)),
            Cell::from(format_bytes(r.avail)),
            Cell::from(format!("{:.0}%", r.use_pct)),
            Cell::from(trim_to(&r.mount, 18)),
        ])
    });

    let df = Table::new(
        df_rows,
        [
            Constraint::Length(14),
            Constraint::Length(9),
            Constraint::Length(9),
            Constraint::Length(9),
            Constraint::Length(5),
            Constraint::Min(8),
        ],
    )
    .header(
        Row::new(vec!["FS", "Size", "Used", "Avail", "Use%", "Mount"]).style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
    )
    .block(Block::default().borders(Borders::NONE));

    frame.render_widget(df, disk_chunks[0]);

    let mut dir_lines: Vec<Line> = Vec::new();
    if app.dash_dir_sizes.is_empty() {
        dir_lines.push(Line::from(Span::styled(
            "Dir: (no data)",
            Style::default().fg(Color::Gray),
        )));
    } else {
        // First line is a label we inject during scan.
        let mut first = true;
        for row in app.dash_dir_sizes.iter() {
            if first {
                dir_lines.push(Line::from(Span::styled(
                    row.clone(),
                    Style::default()
                        .fg(Color::Gray)
                        .add_modifier(Modifier::BOLD),
                )));
                first = false;
                continue;
            }
            dir_lines.push(Line::from(Span::raw(row.clone())));
        }
    }

    frame.render_widget(
        Paragraph::new(dir_lines).alignment(Alignment::Left),
        disk_chunks[1],
    );
}

fn render_processes(frame: &mut ratatui::Frame, area: Rect, app: &mut AppState, system: &System) {
    let mut procs: Vec<ProcRow> = system
        .processes()
        .iter()
        .map(|(pid, p)| ProcRow::from_process(*pid, p))
        .collect();

    // Sort by current mode
    match app.proc_sort {
        ProcSort::Cpu => procs.sort_by_key(|p| Reverse((p.cpu_x10 as i64, p.mem_bytes as i64))),
        ProcSort::Mem => procs.sort_by_key(|p| Reverse((p.mem_bytes as i64, p.cpu_x10 as i64))),
    }

    // Only show top N, but allow scrolling within that list
    let max_rows = 200usize;
    if procs.len() > max_rows {
        procs.truncate(max_rows);
    }

    let header_title = match app.proc_sort {
        ProcSort::Cpu => "Top processes (CPU)",
        ProcSort::Mem => "Top processes (Memory)",
    };

    let block = Block::default()
        .title(header_title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let inner = block.inner(area);

    let visible = (inner.height.saturating_sub(2)) as usize; // table header + footer-ish
    let offset = app.proc_scroll as usize;
    let offset = offset.min(procs.len().saturating_sub(1));

    let slice = &procs[offset..procs.len().min(offset + visible.max(1))];

    let rows = slice.iter().map(|p| {
        Row::new(vec![
            Cell::from(p.pid.to_string()),
            Cell::from(p.name.clone()),
            Cell::from(format!("{:.1}%", p.cpu_x10 as f64 / 10.0)),
            Cell::from(format_bytes(p.mem_bytes)),
        ])
    });

    let table = Table::new(
        rows,
        [
            Constraint::Length(8),
            Constraint::Percentage(55),
            Constraint::Length(10),
            Constraint::Length(14),
        ],
    )
    .header(
        Row::new(vec!["PID", "NAME", "CPU", "MEM"]).style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
    )
    .block(block)
    .highlight_style(Style::default().add_modifier(Modifier::BOLD));

    frame.render_widget(table, area);

    // Hint line
    let hint = Paragraph::new(Line::from(vec![
        Span::styled("Tab", Style::default().fg(Color::Yellow)),
        Span::raw(" toggles CPU/Mem · "),
        Span::styled("↑/↓", Style::default().fg(Color::Yellow)),
        Span::raw(" scroll · Showing top "),
        Span::styled(max_rows.to_string(), Style::default().fg(Color::White)),
    ]))
    .alignment(Alignment::Left);

    let hint_area = Rect {
        x: inner.x,
        y: inner.y + inner.height.saturating_sub(1),
        width: inner.width,
        height: 1,
    };
    frame.render_widget(hint, hint_area);
}

#[derive(Debug, Clone)]
struct ProcRow {
    pid: i32,
    name: String,
    cpu_x10: i32,
    mem_bytes: u64,
}

impl ProcRow {
    fn from_process(pid: sysinfo::Pid, p: &Process) -> Self {
        // sysinfo CPU is percent float; store x10 to sort stably without floats
        let cpu_x10 = (p.cpu_usage() * 10.0) as i32;
        let mem_bytes = p.memory();
        ProcRow {
            pid: pid.as_u32() as i32,
            name: p.name().to_string(),
            cpu_x10,
            mem_bytes,
        }
    }
}

fn render_disk_dive(frame: &mut ratatui::Frame, area: Rect, app: &mut AppState) {
    let target = disk_target_path(app.disk_target);

    let state = app.disk_scan.inner.lock().unwrap();

    let title = if state.running {
        format!("Disk dive  (target: {})  •  scanning", target.display())
    } else {
        format!("Disk dive  (target: {})", target.display())
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Green));

    frame.render_widget(block.clone(), area);
    let inner = block.inner(area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(1)])
        .split(inner);

    // Status line(s)
    let status_line = if let Some(err) = &state.error {
        Line::from(vec![
            Span::styled("Error: ", Style::default().fg(Color::Red)),
            Span::raw(err.clone()),
        ])
    } else if state.running {
        Line::from(vec![
            Span::styled("Scanning… ", Style::default().fg(Color::Yellow)),
            Span::raw(state.progress.clone()),
        ])
    } else if state.results.is_empty() {
        Line::from(vec![
            Span::styled("Press ", Style::default().fg(Color::Gray)),
            Span::styled("s", Style::default().fg(Color::Yellow)),
            Span::styled(" to scan (on-demand) · ", Style::default().fg(Color::Gray)),
            Span::styled("Tab", Style::default().fg(Color::Yellow)),
            Span::styled(" to change target", Style::default().fg(Color::Gray)),
        ])
    } else {
        Line::from(vec![
            Span::styled("Cached results. ", Style::default().fg(Color::Gray)),
            Span::styled("s", Style::default().fg(Color::Yellow)),
            Span::styled(" rescan · ", Style::default().fg(Color::Gray)),
            Span::styled("Tab", Style::default().fg(Color::Yellow)),
            Span::styled(" target · ", Style::default().fg(Color::Gray)),
            Span::styled("↑/↓", Style::default().fg(Color::Yellow)),
            Span::styled(" scroll", Style::default().fg(Color::Gray)),
        ])
    };

    let status = Paragraph::new(vec![status_line]).alignment(Alignment::Left);
    frame.render_widget(status, rows[0]);

    // Results table
    let mut results = state.results.clone();
    drop(state);
    results.sort_by_key(|(_, bytes)| Reverse(*bytes));

    let visible = rows[1].height.saturating_sub(2) as usize; // table header + borders
    let offset = (app.disk_scroll as usize).min(results.len().saturating_sub(1));
    let slice = &results[offset..results.len().min(offset + visible.max(1))];

    let table_rows = slice.iter().enumerate().map(|(i, (dir, bytes))| {
        let zebra = if (offset + i) % 2 == 0 {
            Style::default().fg(Color::White)
        } else {
            Style::default().fg(Color::Gray)
        };

        Row::new(vec![
            Cell::from(dir.clone()),
            Cell::from(format_bytes(*bytes)),
        ])
        .style(zebra)
    });

    let table = Table::new(
        table_rows,
        [Constraint::Percentage(72), Constraint::Length(14)],
    )
    .header(
        Row::new(vec!["Directory", "Size"]).style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
    )
    .block(
        Block::default()
            .title("Top dirs")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Green)),
    );

    frame.render_widget(table, rows[1]);
}

fn start_disk_scan(app: &mut AppState) {
    let target = disk_target_path(app.disk_target);

    // If already scanning, ignore.
    {
        let mut state = app.disk_scan.inner.lock().unwrap();
        if state.running {
            return;
        }
        state.running = true;
        state.error = None;
        state.progress = String::new();
        state.results.clear();
        state.last_target = Some(target.clone());
        state.last_started_at = Some(std::time::SystemTime::now());
        state.last_finished_at = None;
    }

    let inner = app.disk_scan.inner.clone();

    std::thread::spawn(move || {
        let res = scan_top_dirs(&target, &inner);
        let mut state = inner.lock().unwrap();
        state.running = false;
        state.last_finished_at = Some(std::time::SystemTime::now());
        if let Err(e) = res {
            state.error = Some(e);
        }
    });
}

fn scan_top_dirs(target: &Path, inner: &Arc<Mutex<DiskScanState>>) -> Result<(), String> {
    let base = target.to_path_buf();
    if !base.exists() {
        return Err(format!("Target does not exist: {}", base.display()));
    }

    // Quick heuristic: we compute sizes for immediate children (depth 1) and their descendants (depth up to 12)
    // but we stop early if the filesystem is huge.
    let mut children: Vec<PathBuf> = vec![];
    if base.is_dir() {
        if let Ok(rd) = std::fs::read_dir(&base) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    children.push(p);
                }
            }
        }
    }

    if children.is_empty() {
        return Err("No child directories found to scan".to_string());
    }

    let mut results: Vec<(String, u64)> = Vec::new();
    let mut total_seen: u64 = 0;

    for (idx, child) in children.iter().enumerate() {
        {
            let mut st = inner.lock().unwrap();
            st.progress = format!("{}/{}: {}", idx + 1, children.len(), child.display());
        }

        let mut size: u64 = 0;
        let mut seen: u64 = 0;

        // Walk with a depth limit to stay responsive.
        for entry in WalkDir::new(child)
            .follow_links(false)
            .max_depth(12)
            .into_iter()
            .flatten()
        {
            let ft = entry.file_type();
            if ft.is_file() {
                if let Ok(md) = entry.metadata() {
                    size = size.saturating_add(md.len());
                }
                seen += 1;
                total_seen += 1;
                // Safety cap: don't scan endlessly.
                if seen >= 50_000 || total_seen >= 300_000 {
                    break;
                }
            }
        }

        results.push((child.display().to_string(), size));

        // Keep top 25 as we go.
        results.sort_by_key(|(_, b)| Reverse(*b));
        results.truncate(25);

        {
            let mut st = inner.lock().unwrap();
            st.results = results.clone();
        }

        if total_seen >= 300_000 {
            let mut st = inner.lock().unwrap();
            st.progress = "Reached scan cap (kept it lightweight).".to_string();
            break;
        }
    }

    Ok(())
}

fn disk_target_path(target: DiskTarget) -> PathBuf {
    match target {
        DiskTarget::Var => PathBuf::from("/var"),
        DiskTarget::Home => std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| "/".into()),
        DiskTarget::Root => PathBuf::from("/"),
    }
}

fn refresh(system: &mut System, disks: &mut Disks, refresh_processes: bool) {
    system.refresh_cpu();
    system.refresh_memory();
    if refresh_processes {
        system.refresh_processes();
    }
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

fn disks_table_filtered(disks: &Disks, limit: usize, show_all: bool) -> Vec<DiskRow> {
    // Filter noisy mounts (tmpfs/udev/ramfs, etc.) and show the real stuff.
    let mut seen_mounts: HashSet<String> = HashSet::new();
    let mut rows: Vec<DiskRow> = Vec::new();

    for d in disks.iter() {
        let mount = d.mount_point().to_string_lossy().to_string();
        if seen_mounts.contains(&mount) {
            continue;
        }
        seen_mounts.insert(mount.clone());

        let fs = d.name().to_string_lossy().to_string();
        let total = d.total_space();
        let avail = d.available_space();
        let used = total.saturating_sub(avail);
        let pct = percent(used, total);

        // Heuristic: hide pseudo filesystems by name/mount (unless show_all is true).
        // This is intentionally simple; if it hides something useful we can tune.
        if !show_all {
            let fs_l = fs.to_lowercase();
            if fs_l.contains("tmpfs") || fs_l.contains("udev") || fs_l.contains("devtmpfs") {
                continue;
            }
            if mount.starts_with("/run") || mount.starts_with("/dev") || mount.starts_with("/sys") {
                continue;
            }
        }

        rows.push(DiskRow {
            fs,
            size: total,
            used,
            avail,
            use_pct: pct,
            mount,
        });
    }

    // Biggest first.
    rows.sort_by_key(|r| Reverse(r.size));
    rows.truncate(limit);
    rows
}

fn format_top_processes(system: &System, sort: ProcSort, count: usize) -> Vec<String> {
    let mut procs: Vec<ProcRow> = system
        .processes()
        .iter()
        .map(|(pid, p)| ProcRow::from_process(*pid, p))
        .collect();

    match sort {
        ProcSort::Cpu => procs.sort_by_key(|p| Reverse((p.cpu_x10 as i64, p.mem_bytes as i64))),
        ProcSort::Mem => procs.sort_by_key(|p| Reverse((p.mem_bytes as i64, p.cpu_x10 as i64))),
    }

    procs
        .into_iter()
        .take(count)
        .map(|p| {
            let cpu = format!("{:.1}%", p.cpu_x10 as f64 / 10.0);
            let mem = format_bytes(p.mem_bytes);
            // Keep it short; this is dashboard real estate.
            format!("{}  {}  {}", trim_to(&p.name, 18), cpu, mem)
        })
        .collect()
}

fn dash_target_path(target: DashDirTarget) -> (String, PathBuf) {
    match target {
        DashDirTarget::Cwd => {
            let p = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
            let label = p.to_string_lossy().to_string();
            (label, p)
        }
        DashDirTarget::Var => ("/var".to_string(), PathBuf::from("/var")),
    }
}

fn scan_dir_quick(dir: &Path, limit: usize) -> Vec<String> {
    let mut items: Vec<(String, Option<u64>, bool)> = Vec::new(); // (name, size, is_dir)
    let rd = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(_) => return vec![],
    };

    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        let md = match e.metadata() {
            Ok(md) => md,
            Err(_) => continue,
        };
        let is_dir = md.is_dir();
        let size = if md.is_file() { Some(md.len()) } else { None };
        items.push((name, size, is_dir));
    }

    // Sort: biggest files first; then dirs; stable by name.
    items.sort_by_key(|(name, size, is_dir)| {
        let dir_rank = if *is_dir { 1 } else { 0 };
        (dir_rank, Reverse(size.unwrap_or(0)), name.clone())
    });

    let mut out: Vec<String> = Vec::new();
    for (name, size, is_dir) in items.into_iter().take(limit) {
        if is_dir {
            out.push(format!("{}/  (dir)", name));
        } else {
            out.push(format!("{}  {}", name, format_bytes(size.unwrap_or(0))));
        }
    }
    out
}

fn trim_to(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else if max <= 1 {
        "…".to_string()
    } else {
        format!("{}…", &s[..max - 1])
    }
}

fn format_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;
    const TIB: f64 = GIB * 1024.0;

    let b = bytes as f64;
    if b >= TIB {
        format!("{:.2} TiB", b / TIB)
    } else if b >= GIB {
        format!("{:.2} GiB", b / GIB)
    } else if b >= MIB {
        format!("{:.2} MiB", b / MIB)
    } else if b >= KIB {
        format!("{:.2} KiB", b / KIB)
    } else {
        format!("{bytes} B")
    }
}
