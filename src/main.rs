mod app;
mod cli;
mod disk;
mod services;
mod system;
mod types;
mod ui;
mod update;
mod utils;

use std::io;
use std::time::{Duration, Instant};

use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::{execute, terminal};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use sysinfo::{Disks, ProcessRefreshKind, RefreshKind, System};

use app::run_app;
use cli::{parse_args, print_cli_help};
use system::refresh;
use types::AppState;
use update::{check_update, load_update_cache, VERSION};

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

    let mut app = AppState {
        tick_ms: args.tick_ms,
        ..Default::default()
    };

    app.update = check_update(load_update_cache());

    let out = run_app(
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

    if let Ok(Some(txt)) = &out {
        println!("{txt}");
    }

    out.map(|_| ())
}
