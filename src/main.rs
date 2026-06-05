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
use std::process;
use std::time::{Duration, Instant};

use crossterm::cursor::Show;
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use sysinfo::{Disks, ProcessRefreshKind, RefreshKind, System};

use app::run_app;
use cli::{parse_args, print_cli_help};
use system::refresh;
use types::AppState;
use update::{check_update, load_update_cache, VERSION};

struct TerminalGuard {
    mouse_enabled: bool,
    active: bool,
}

impl TerminalGuard {
    fn enter(mouse_enabled: bool) -> io::Result<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        let guard = Self {
            mouse_enabled,
            active: true,
        };
        if mouse_enabled {
            execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        } else {
            execute!(stdout, EnterAlternateScreen)?;
        }
        Ok(guard)
    }

    fn restore(&mut self) -> io::Result<()> {
        if !self.active {
            return Ok(());
        }
        let mut stdout = io::stdout();
        let raw_result = disable_raw_mode();
        let screen_result = if self.mouse_enabled {
            execute!(stdout, LeaveAlternateScreen, DisableMouseCapture, Show)
        } else {
            execute!(stdout, LeaveAlternateScreen, Show)
        };
        self.active = false;
        raw_result?;
        screen_result
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

fn main() {
    if let Err(e) = run() {
        eprintln!("ferro: {e}");
        process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args = parse_args()?;
    if args.show_version {
        println!("{VERSION}");
        return Ok(());
    }
    if args.show_help {
        print_cli_help();
        return Ok(());
    }

    let mut guard = TerminalGuard::enter(!args.no_mouse).map_err(|e| e.to_string())?;

    let stdout = io::stdout();
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).map_err(|e| e.to_string())?;

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
    )
    .map_err(|e| e.to_string());

    guard.restore().map_err(|e| e.to_string())?;

    if let Ok(Some(txt)) = &out {
        println!("{txt}");
    }

    out.map(|_| ())
}
