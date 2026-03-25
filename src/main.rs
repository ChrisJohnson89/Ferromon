use std::cmp::Reverse;
use std::collections::{HashSet, VecDeque};
use std::fs;
use std::io;
use std::io::Read;
use std::process::Command;

use flate2::read::GzDecoder;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tar::Archive;

use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
    KeyModifiers,
};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::{execute, terminal};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Clear, Gauge, Paragraph, Row, Sparkline, Table, Wrap};
use ratatui::{backend::CrosstermBackend, prelude::Alignment, Terminal};
use sysinfo::{Disks, Process, ProcessRefreshKind, RefreshKind, System};
use walkdir::WalkDir;

const VERSION: &str = env!("FERRO_VERSION");
const REPO_OWNER: &str = "ChrisJohnson89";
const REPO_NAME: &str = "Ferromon";
const UPDATE_CHECK_TTL_SEC: u64 = 6 * 60 * 60;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Screen {
    #[default]
    Dashboard,
    Processes,
    DiskDive,
    Services,
    Logs,
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
    Home,
    Root,
}

impl DashDirTarget {
    fn next(self) -> Self {
        match self {
            DashDirTarget::Cwd => DashDirTarget::Var,
            DashDirTarget::Var => DashDirTarget::Home,
            DashDirTarget::Home => DashDirTarget::Root,
            DashDirTarget::Root => DashDirTarget::Cwd,
        }
    }

    fn title(self) -> &'static str {
        match self {
            DashDirTarget::Cwd => "CWD",
            DashDirTarget::Var => "/var",
            DashDirTarget::Home => "HOME",
            DashDirTarget::Root => "/",
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum ServiceFilter {
    Failed,
    Unhealthy,
    Active,
    #[default]
    All,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum LogSeverity {
    Errors,
    Warnings,
    #[default]
    Info,
    Debug,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum LogUnitFilter {
    #[default]
    Selected,
    All,
}

struct AppState {
    screen: Screen,
    show_help: bool,

    proc_sort: ProcSort,
    proc_scroll: u16,
    proc_search: String,
    proc_search_active: bool,
    proc_kill_confirm: Option<(i32, String)>,

    disk_target: DiskTarget,
    disk_scroll: u16,
    disk_scan: DiskScan,
    service_scroll: u16,
    service_filter: ServiceFilter,
    service_search: String,
    service_search_active: bool,
    service_state: ServiceState,
    service_last_refresh_at: Option<Instant>,
    logs_scroll: u16,
    log_severity: LogSeverity,
    log_unit_filter: LogUnitFilter,
    log_state: LogState,
    log_last_refresh_at: Option<Instant>,
    log_selected_unit: Option<String>,

    // Dashboard caches (quick overview)
    dash_dir_target: DashDirTarget,
    dash_dir_sizes: Vec<String>,
    dash_mount_rows: Vec<DiskRow>,
    dash_top_cpu: Vec<String>,
    dash_top_mem: Vec<String>,
    dash_mem_pressure: Vec<String>,
    dash_cpu_history: VecDeque<u16>,
    dash_mem_history: VecDeque<u16>,
    dash_last_proc_at: Option<Instant>,
    dash_last_fs_at: Option<Instant>,
    dash_last_history_at: Option<Instant>,
    dash_show_all_mounts: bool,
    footer_tip_idx: u8,
    tick_ms: u64,
    dump_snapshot: bool,

    update: UpdateState,
    do_update: bool,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            screen: Screen::default(),
            show_help: false,
            proc_sort: ProcSort::default(),
            proc_scroll: 0,
            proc_search: String::new(),
            proc_search_active: false,
            proc_kill_confirm: None,
            disk_target: DiskTarget::default(),
            disk_scroll: 0,
            disk_scan: DiskScan::default(),
            service_scroll: 0,
            service_filter: ServiceFilter::default(),
            service_search: String::new(),
            service_search_active: false,
            service_state: ServiceState::default(),
            service_last_refresh_at: None,
            logs_scroll: 0,
            log_severity: LogSeverity::default(),
            log_unit_filter: LogUnitFilter::default(),
            log_state: LogState::default(),
            log_last_refresh_at: None,
            log_selected_unit: None,
            dash_dir_target: DashDirTarget::default(),
            dash_dir_sizes: Vec::new(),
            dash_mount_rows: Vec::new(),
            dash_top_cpu: Vec::new(),
            dash_top_mem: Vec::new(),
            dash_mem_pressure: Vec::new(),
            dash_cpu_history: VecDeque::new(),
            dash_mem_history: VecDeque::new(),
            dash_last_proc_at: None,
            dash_last_fs_at: None,
            dash_last_history_at: None,
            dash_show_all_mounts: true,
            footer_tip_idx: 0,
            tick_ms: 500,
            dump_snapshot: false,
            update: UpdateState::default(),
            do_update: false,
        }
    }
}

#[derive(Clone, Default)]
struct DiskScan {
    inner: Arc<Mutex<DiskScanState>>,
}

#[derive(Clone, Default)]
struct ServiceState {
    inner: Arc<Mutex<ServiceStateInner>>,
}

#[derive(Clone, Default)]
struct LogState {
    inner: Arc<Mutex<LogStateInner>>,
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
                    let clamped = v.clamp(50, 5000);
                    if v != clamped {
                        eprintln!(
                            "Warning: --tick-ms {} is out of range, clamped to {}",
                            v, clamped
                        );
                    }
                    tick_ms = clamped;
                    i += 1;
                } else {
                    show_help = true;
                }
            }
            _ if a.starts_with("--tick-ms=") => {
                if let Some(v) = a.split('=').nth(1) {
                    if let Ok(v) = v.parse::<u64>() {
                        let clamped = v.clamp(50, 5000);
                        if v != clamped {
                            eprintln!(
                                "Warning: --tick-ms {} is out of range, clamped to {}",
                                v, clamped
                            );
                        }
                        tick_ms = clamped;
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
  q quit · ? help · Esc back · p processes · d disk dive · v services · l logs · r refresh · f mounts · u update/filter · x snapshot

UPDATE:
  Ferromon checks GitHub releases occasionally and can self-update.
  Set FERRO_NO_UPDATE_CHECK=1 to disable checks."
    );
}

#[derive(Clone, Default)]
struct UpdateState {
    // Cached result to avoid spamming GitHub.
    last_checked_at: Option<std::time::SystemTime>,
    latest_tag: Option<String>,
    available: bool,
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GhRelease {
    tag_name: String,
    assets: Vec<GhAsset>,
}

#[derive(Debug, Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn target_triple() -> &'static str {
    "x86_64-unknown-linux-musl"
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn target_triple() -> &'static str {
    "aarch64-apple-darwin"
}

#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
fn target_triple() -> &'static str {
    "x86_64-apple-darwin"
}

#[cfg(not(any(
    all(target_os = "linux", target_arch = "x86_64"),
    all(target_os = "macos", target_arch = "aarch64"),
    all(target_os = "macos", target_arch = "x86_64"),
)))]
fn target_triple() -> &'static str {
    "unknown"
}

fn update_api_url() -> String {
    format!(
        "https://api.github.com/repos/{}/{}/releases/latest",
        REPO_OWNER, REPO_NAME
    )
}

fn should_check_update(st: &UpdateState) -> bool {
    match st.last_checked_at {
        None => true,
        Some(t) => t
            .elapsed()
            .map(|d| d.as_secs() >= UPDATE_CHECK_TTL_SEC)
            .unwrap_or(true),
    }
}

fn update_cache_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(
        PathBuf::from(home)
            .join(".cache")
            .join("ferromon")
            .join("update.json"),
    )
}

fn load_update_cache() -> UpdateState {
    let path = match update_cache_path() {
        Some(p) => p,
        None => return UpdateState::default(),
    };

    let Ok(bytes) = fs::read(&path) else {
        return UpdateState::default();
    };

    #[derive(Deserialize)]
    struct Cache {
        last_checked_unix: u64,
        latest_tag: Option<String>,
        available: bool,
    }

    let Ok(c) = serde_json::from_slice::<Cache>(&bytes) else {
        return UpdateState::default();
    };

    UpdateState {
        last_checked_at: Some(std::time::UNIX_EPOCH + Duration::from_secs(c.last_checked_unix)),
        latest_tag: c.latest_tag,
        available: c.available,
        error: None,
    }
}

fn save_update_cache(st: &UpdateState) {
    let Some(path) = update_cache_path() else {
        return;
    };

    let Some(dir) = path.parent() else {
        return;
    };
    let _ = fs::create_dir_all(dir);

    let last_checked_unix = st
        .last_checked_at
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);

    #[derive(serde::Serialize)]
    struct Cache<'a> {
        last_checked_unix: u64,
        latest_tag: &'a Option<String>,
        available: bool,
    }

    let bytes = serde_json::to_vec_pretty(&Cache {
        last_checked_unix,
        latest_tag: &st.latest_tag,
        available: st.available,
    });

    if let Ok(bytes) = bytes {
        let _ = fs::write(&path, bytes);
    }
}

fn check_update(mut st: UpdateState) -> UpdateState {
    if std::env::var_os("FERRO_NO_UPDATE_CHECK").is_some() {
        return st;
    }

    if !should_check_update(&st) {
        return st;
    }

    let url = update_api_url();
    let req = ureq::get(&url)
        .set("User-Agent", "ferromon")
        .timeout(Duration::from_secs(3));

    match req.call() {
        Ok(resp) => {
            let Ok(body) = resp.into_string() else {
                st.error = Some("failed to read response".to_string());
                st.last_checked_at = Some(std::time::SystemTime::now());
                save_update_cache(&st);
                return st;
            };
            let parsed = serde_json::from_str::<GhRelease>(&body);
            match parsed {
                Ok(r) => {
                    st.latest_tag = Some(r.tag_name.clone());
                    st.available = r.tag_name.trim_start_matches('v') != VERSION;
                    st.error = None;
                }
                Err(e) => {
                    st.error = Some(format!("bad json: {e}"));
                }
            }
        }
        Err(e) => {
            st.error = Some(format!("update check failed: {e}"));
        }
    }

    st.last_checked_at = Some(std::time::SystemTime::now());
    save_update_cache(&st);
    st
}

fn install_path_user() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".local").join("bin").join("ferro"))
}

fn download_bytes(url: &str) -> Result<Vec<u8>, String> {
    let resp = ureq::get(url)
        .set("User-Agent", "ferromon")
        .timeout(Duration::from_secs(10))
        .call()
        .map_err(|e| format!("download failed: {e}"))?;

    let mut reader = resp.into_reader();
    let mut buf: Vec<u8> = Vec::new();
    reader
        .read_to_end(&mut buf)
        .map_err(|e| format!("read failed: {e}"))?;
    Ok(buf)
}

fn perform_self_update(latest_tag: &str) -> Result<String, String> {
    let target = target_triple();
    if target == "unknown" {
        return Err("unknown target".to_string());
    }

    let asset_name = format!("ferromon-{}-{}.tar.gz", latest_tag, target);
    let sha_name = format!("{}.sha256", asset_name);

    let rel_url = update_api_url();
    let resp = ureq::get(&rel_url)
        .set("User-Agent", "ferromon")
        .timeout(Duration::from_secs(3))
        .call()
        .map_err(|e| format!("update metadata fetch failed: {e}"))?;

    let body = resp
        .into_string()
        .map_err(|e| format!("read release json failed: {e}"))?;
    let r = serde_json::from_str::<GhRelease>(&body).map_err(|e| format!("bad json: {e}"))?;

    let mut asset_url: Option<String> = None;
    let mut sha_url: Option<String> = None;
    for a in r.assets {
        if a.name == asset_name {
            asset_url = Some(a.browser_download_url);
        } else if a.name == sha_name {
            sha_url = Some(a.browser_download_url);
        }
    }

    let asset_url = asset_url.ok_or_else(|| format!("missing asset {asset_name}"))?;
    let sha_url = sha_url.ok_or_else(|| format!("missing asset {sha_name}"))?;

    // Download both
    let tar_gz = download_bytes(&asset_url)?;
    let sha_txt =
        String::from_utf8(download_bytes(&sha_url)?).map_err(|e| format!("bad sha: {e}"))?;

    // Parse sha file: "<hex>  <filename>"
    let expected = sha_txt
        .split_whitespace()
        .next()
        .ok_or_else(|| "bad sha file".to_string())
        .map(|s| s.to_string())?;

    if expected.len() < 16 {
        return Err("checksum looked wrong".to_string());
    }

    // Extract `ferro` from tar.gz
    let mut ar = Archive::new(GzDecoder::new(&tar_gz[..]));
    let mut ferro_bytes: Option<Vec<u8>> = None;
    for entry in ar.entries().map_err(|e| format!("tar read failed: {e}"))? {
        let mut entry = entry.map_err(|e| format!("tar entry failed: {e}"))?;
        let path = entry
            .path()
            .map_err(|e| format!("tar path failed: {e}"))?
            .to_string_lossy()
            .to_string();
        if path == "ferro" {
            let mut buf = Vec::new();
            entry
                .read_to_end(&mut buf)
                .map_err(|e| format!("tar read ferro failed: {e}"))?;
            ferro_bytes = Some(buf);
            break;
        }
    }

    let ferro_bytes = ferro_bytes.ok_or_else(|| "missing ferro in archive".to_string())?;

    let dst = install_path_user().ok_or_else(|| "HOME not set".to_string())?;
    let dir = dst
        .parent()
        .ok_or_else(|| "invalid install destination".to_string())?;
    fs::create_dir_all(dir).map_err(|e| format!("mkdir failed: {e}"))?;

    let tmp = dst.with_extension("new");
    fs::write(&tmp, &ferro_bytes).map_err(|e| format!("write failed: {e}"))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perm = fs::metadata(&tmp)
            .map_err(|e| format!("metadata failed: {e}"))?
            .permissions();
        perm.set_mode(0o755);
        fs::set_permissions(&tmp, perm).map_err(|e| format!("chmod failed: {e}"))?;
    }

    fs::rename(&tmp, &dst).map_err(|e| format!("install failed: {e}"))?;

    Ok(format!(
        "Updated to {} (installed to {})
If this isn't on PATH, add ~/.local/bin to PATH.",
        latest_tag,
        dst.display()
    ))
}

#[derive(Default)]
struct DiskScanState {
    running: bool,
    last_target: Option<PathBuf>,
    current_path: Option<PathBuf>,
    last_started_at: Option<std::time::SystemTime>,
    last_finished_at: Option<std::time::SystemTime>,
    progress: String,
    results: Vec<DiskEntry>,
    error: Option<String>,
}

#[derive(Default)]
struct ServiceStateInner {
    running: bool,
    unsupported: Option<String>,
    error: Option<String>,
    rows: Vec<ServiceRow>,
    last_updated_at: Option<std::time::SystemTime>,
}

#[derive(Default)]
struct LogStateInner {
    running: bool,
    unsupported: Option<String>,
    error: Option<String>,
    lines: Vec<String>,
    last_updated_at: Option<std::time::SystemTime>,
    source: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ServiceHealth {
    Healthy,
    Warning,
    Critical,
}

#[derive(Debug, Clone)]
struct ServiceRow {
    name: String,
    description: String,
    load_state: String,
    active_state: String,
    sub_state: String,
    restarts: u32,
    last_change: String,
    health: ServiceHealth,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiskEntryKind {
    Directory,
    File,
}

#[derive(Debug, Clone)]
struct DiskEntry {
    path: PathBuf,
    bytes: u64,
    kind: DiskEntryKind,
}

fn is_file_like_package(path: &Path) -> bool {
    if !path.is_dir() {
        return false;
    }

    let Some(ext) = path.extension().and_then(|ext| ext.to_str()) else {
        return false;
    };

    matches!(
        ext,
        "app" | "bundle" | "framework" | "plugin" | "kext" | "pkg" | "xpc" | "appex"
    )
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

fn render_too_small(frame: &mut ratatui::Frame, area: Rect) {
    let size = frame.size();
    let msg = vec![
        Line::from("Ferromon"),
        Line::from(""),
        Line::from("Terminal too small."),
        Line::from(""),
        Line::from(vec![
            Span::styled("Current: ", Style::default().fg(Color::Gray)),
            Span::styled(
                format!("{}x{}", size.width, size.height),
                Style::default().fg(Color::Yellow),
            ),
        ]),
        Line::from(vec![
            Span::styled("Required: ", Style::default().fg(Color::Gray)),
            Span::styled("80x14 minimum", Style::default().fg(Color::Green)),
        ]),
        Line::from(""),
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

fn services_supported_message() -> Option<String> {
    if cfg!(target_os = "linux") {
        None
    } else {
        Some("Service health and journal logs are currently Linux-only.".to_string())
    }
}

fn refresh_services(app: &mut AppState, force: bool) {
    if let Some(msg) = services_supported_message() {
        let mut state = app.service_state.inner.lock().unwrap();
        state.running = false;
        state.unsupported = Some(msg);
        state.error = None;
        state.rows.clear();
        return;
    }

    let due = force
        || app
            .service_last_refresh_at
            .map(|t| t.elapsed() >= Duration::from_secs(15))
            .unwrap_or(true);
    if !due {
        return;
    }

    {
        let mut state = app.service_state.inner.lock().unwrap();
        if state.running {
            return;
        }
        state.running = true;
        state.error = None;
        state.unsupported = None;
    }

    app.service_last_refresh_at = Some(Instant::now());
    let inner = app.service_state.inner.clone();
    std::thread::spawn(move || {
        let result = collect_services();
        let mut state = inner.lock().unwrap();
        state.running = false;
        match result {
            Ok(rows) => {
                state.rows = rows;
                state.error = None;
                state.last_updated_at = Some(std::time::SystemTime::now());
            }
            Err(err) => {
                state.error = Some(err);
            }
        }
    });
}

fn refresh_logs(app: &mut AppState, force: bool) {
    if let Some(msg) = services_supported_message() {
        let mut state = app.log_state.inner.lock().unwrap();
        state.running = false;
        state.unsupported = Some(msg);
        state.error = None;
        state.lines.clear();
        state.source.clear();
        return;
    }

    let due = force
        || app
            .log_last_refresh_at
            .map(|t| t.elapsed() >= Duration::from_secs(5))
            .unwrap_or(true);
    if !due {
        return;
    }

    let unit = match app.log_unit_filter {
        LogUnitFilter::Selected => app.log_selected_unit.clone(),
        LogUnitFilter::All => None,
    };

    {
        let mut state = app.log_state.inner.lock().unwrap();
        if state.running {
            return;
        }
        state.running = true;
        state.error = None;
        state.unsupported = None;
    }

    app.log_last_refresh_at = Some(Instant::now());
    let inner = app.log_state.inner.clone();
    let severity = app.log_severity;
    std::thread::spawn(move || {
        let result = collect_logs(unit.as_deref(), severity);
        let mut state = inner.lock().unwrap();
        state.running = false;
        match result {
            Ok((source, lines)) => {
                state.source = source;
                state.lines = lines;
                state.error = None;
                state.last_updated_at = Some(std::time::SystemTime::now());
            }
            Err(err) => {
                state.error = Some(err);
            }
        }
    });
}

fn collect_services() -> Result<Vec<ServiceRow>, String> {
    if !cfg!(target_os = "linux") {
        return Err("services are unsupported on this OS".to_string());
    }

    let output = Command::new("systemctl")
        .args([
            "show",
            "*.service",
            "--type=service",
            "--all",
            "--no-pager",
            "--property=Id,Description,LoadState,ActiveState,SubState,NRestarts,ActiveEnterTimestamp,InactiveEnterTimestamp",
        ])
        .output()
        .map_err(|e| format!("failed to run systemctl: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            format!("systemctl exited with {}", output.status)
        } else {
            stderr
        });
    }

    Ok(parse_service_rows(&String::from_utf8_lossy(&output.stdout)))
}

fn parse_service_rows(stdout: &str) -> Vec<ServiceRow> {
    let mut rows = Vec::new();
    for block in stdout.split("\n\n") {
        let mut name = String::new();
        let mut description = String::new();
        let mut load_state = String::new();
        let mut active_state = String::new();
        let mut sub_state = String::new();
        let mut restarts = 0u32;
        let mut active_enter = String::new();
        let mut inactive_enter = String::new();

        for line in block.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            match key {
                "Id" => name = value.to_string(),
                "Description" => description = value.to_string(),
                "LoadState" => load_state = value.to_string(),
                "ActiveState" => active_state = value.to_string(),
                "SubState" => sub_state = value.to_string(),
                "NRestarts" => restarts = value.parse::<u32>().unwrap_or(0),
                "ActiveEnterTimestamp" => active_enter = value.to_string(),
                "InactiveEnterTimestamp" => inactive_enter = value.to_string(),
                _ => {}
            }
        }

        if name.is_empty() || !name.ends_with(".service") {
            continue;
        }

        let last_change = if active_state == "active" {
            active_enter
        } else {
            inactive_enter
        };
        let health = service_health(&load_state, &active_state, &sub_state, restarts);

        rows.push(ServiceRow {
            name,
            description,
            load_state,
            active_state,
            sub_state,
            restarts,
            last_change: simplify_timestamp(&last_change),
            health,
        });
    }

    rows.sort_by_key(|row| {
        (
            service_health_rank(row.health),
            Reverse(row.restarts),
            row.name.clone(),
        )
    });
    rows
}

fn collect_logs(
    unit: Option<&str>,
    severity: LogSeverity,
) -> Result<(String, Vec<String>), String> {
    if !cfg!(target_os = "linux") {
        return Err("logs are unsupported on this OS".to_string());
    }

    let mut cmd = Command::new("journalctl");
    cmd.args(["--no-pager", "-o", "short-iso", "-n", "80"]);
    match severity {
        LogSeverity::Errors => {
            cmd.args(["-p", "err"]);
        }
        LogSeverity::Warnings => {
            cmd.args(["-p", "warning"]);
        }
        LogSeverity::Info => {
            cmd.args(["-p", "info"]);
        }
        LogSeverity::Debug => {
            cmd.args(["-p", "debug"]);
        }
    }
    if let Some(unit) = unit {
        cmd.args(["-u", unit]);
    }

    match cmd.output() {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let lines = stdout
                .lines()
                .map(|line| line.to_string())
                .collect::<Vec<String>>();
            return Ok(("journalctl".to_string(), lines));
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            if !stderr.is_empty() {
                return fallback_syslog(unit, severity, &stderr);
            }
        }
        Err(err) => {
            return fallback_syslog(unit, severity, &format!("journalctl failed: {err}"));
        }
    }

    fallback_syslog(unit, severity, "journalctl failed")
}

fn fallback_syslog(
    unit: Option<&str>,
    severity: LogSeverity,
    journal_error: &str,
) -> Result<(String, Vec<String>), String> {
    let syslog_path = ["/var/log/syslog", "/var/log/messages"]
        .iter()
        .find(|path| Path::new(path).exists())
        .copied()
        .ok_or_else(|| journal_error.to_string())?;

    let output = Command::new("tail")
        .args(["-n", "120", syslog_path])
        .output()
        .map_err(|e| format!("{journal_error}; fallback tail failed: {e}"))?;
    if !output.status.success() {
        return Err(journal_error.to_string());
    }

    let mut lines = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| line.to_string())
        .collect::<Vec<String>>();

    if let Some(unit) = unit {
        lines.retain(|line| line.contains(unit));
    }

    let needle = match severity {
        LogSeverity::Errors => Some("err"),
        LogSeverity::Warnings => Some("warn"),
        LogSeverity::Info => None,
        LogSeverity::Debug => None,
    };
    if let Some(needle) = needle {
        let needle = needle.to_string();
        lines.retain(|line| line.to_lowercase().contains(&needle));
    }

    Ok((format!("syslog ({syslog_path})"), lines))
}

fn service_health(
    load_state: &str,
    active_state: &str,
    sub_state: &str,
    restarts: u32,
) -> ServiceHealth {
    if load_state != "loaded" || active_state == "failed" {
        ServiceHealth::Critical
    } else if active_state != "active" || sub_state != "running" || restarts > 0 {
        ServiceHealth::Warning
    } else {
        ServiceHealth::Healthy
    }
}

fn service_health_rank(health: ServiceHealth) -> u8 {
    match health {
        ServiceHealth::Critical => 0,
        ServiceHealth::Warning => 1,
        ServiceHealth::Healthy => 2,
    }
}

fn simplify_timestamp(ts: &str) -> String {
    if ts.trim().is_empty() || ts.trim() == "n/a" {
        "-".to_string()
    } else {
        trim_to(ts.trim(), 24)
    }
}

fn service_filter_label(filter: ServiceFilter) -> &'static str {
    match filter {
        ServiceFilter::Failed => "failed",
        ServiceFilter::Unhealthy => "unhealthy",
        ServiceFilter::Active => "active",
        ServiceFilter::All => "all",
    }
}

fn log_severity_label(sev: LogSeverity) -> &'static str {
    match sev {
        LogSeverity::Errors => "err+",
        LogSeverity::Warnings => "warning+",
        LogSeverity::Info => "info+",
        LogSeverity::Debug => "debug+",
    }
}

fn log_unit_filter_label(filter: LogUnitFilter, selected_unit: Option<&str>) -> String {
    match filter {
        LogUnitFilter::Selected => selected_unit.unwrap_or("selected").to_string(),
        LogUnitFilter::All => "all units".to_string(),
    }
}

fn is_text_input_key(key: &KeyEvent) -> bool {
    key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT
}

fn handle_service_search_key(app: &mut AppState, key: &KeyEvent) -> bool {
    if !matches!(app.screen, Screen::Services) {
        return false;
    }

    if app.service_search_active {
        match key.code {
            KeyCode::Enter => {
                app.service_search_active = false;
                true
            }
            KeyCode::Esc => {
                if app.service_search.is_empty() {
                    app.service_search_active = false;
                } else {
                    app.service_search.clear();
                    app.service_search_active = false;
                    app.service_scroll = 0;
                }
                true
            }
            KeyCode::Backspace => {
                if app.service_search.is_empty() {
                    app.service_search_active = false;
                } else {
                    app.service_search.pop();
                    app.service_scroll = 0;
                }
                true
            }
            KeyCode::Char(c) if is_text_input_key(key) => {
                app.service_search.push(c);
                app.service_scroll = 0;
                true
            }
            _ => false,
        }
    } else {
        match key.code {
            KeyCode::Char('/') if is_text_input_key(key) => {
                app.service_search_active = true;
                true
            }
            KeyCode::Esc if !app.service_search.is_empty() => {
                app.service_search.clear();
                app.service_scroll = 0;
                true
            }
            KeyCode::Backspace if !app.service_search.is_empty() => {
                app.service_search.pop();
                app.service_scroll = 0;
                true
            }
            _ => false,
        }
    }
}

fn handle_proc_search_key(app: &mut AppState, key: &KeyEvent) -> bool {
    if !matches!(app.screen, Screen::Processes) {
        return false;
    }

    if app.proc_search_active {
        match key.code {
            KeyCode::Enter => {
                app.proc_search_active = false;
                true
            }
            KeyCode::Esc => {
                if app.proc_search.is_empty() {
                    app.proc_search_active = false;
                } else {
                    app.proc_search.clear();
                    app.proc_search_active = false;
                    app.proc_scroll = 0;
                }
                true
            }
            KeyCode::Backspace => {
                if app.proc_search.is_empty() {
                    app.proc_search_active = false;
                } else {
                    app.proc_search.pop();
                    app.proc_scroll = 0;
                }
                true
            }
            KeyCode::Char(c) if is_text_input_key(key) => {
                app.proc_search.push(c);
                app.proc_scroll = 0;
                true
            }
            _ => false,
        }
    } else {
        match key.code {
            KeyCode::Char('/') if is_text_input_key(key) => {
                app.proc_search_active = true;
                true
            }
            KeyCode::Esc if !app.proc_search.is_empty() => {
                app.proc_search.clear();
                app.proc_scroll = 0;
                true
            }
            KeyCode::Backspace if !app.proc_search.is_empty() => {
                app.proc_search.pop();
                app.proc_scroll = 0;
                true
            }
            _ => false,
        }
    }
}

fn filtered_proc_rows(rows: Vec<ProcRow>, search: &str) -> Vec<ProcRow> {
    if search.is_empty() {
        return rows;
    }
    let needle = search.trim().to_ascii_lowercase();
    let mut matched: Vec<ProcRow> = rows
        .into_iter()
        .filter(|p| p.name.to_ascii_lowercase().contains(&needle))
        .collect();
    matched.sort_by(|a, b| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()));
    matched
}

fn filtered_service_rows(
    rows: &[ServiceRow],
    filter: ServiceFilter,
    search: &str,
) -> Vec<ServiceRow> {
    let search = search.trim().to_ascii_lowercase();
    rows.iter()
        .filter(|row| {
            let filter_match = match filter {
                ServiceFilter::Failed => row.health == ServiceHealth::Critical,
                ServiceFilter::Unhealthy => row.health != ServiceHealth::Healthy,
                ServiceFilter::Active => row.active_state == "active",
                ServiceFilter::All => true,
            };
            let search_match = search.is_empty()
                || row.name.to_ascii_lowercase().contains(&search)
                || row.description.to_ascii_lowercase().contains(&search);
            filter_match && search_match
        })
        .cloned()
        .collect()
}

fn selected_service(app: &AppState) -> Option<ServiceRow> {
    let state = app.service_state.inner.lock().unwrap();
    let rows = filtered_service_rows(&state.rows, app.service_filter, &app.service_search);
    rows.get(app.service_scroll as usize).cloned()
}

fn open_logs_for_selected_service(app: &mut AppState) {
    if let Some(row) = selected_service(app) {
        app.log_selected_unit = Some(row.name);
        app.logs_scroll = 0;
        app.screen = Screen::Logs;
        refresh_logs(app, true);
    }
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    system: &mut System,
    disks: &mut Disks,
    app: &mut AppState,
    tick_rate: Duration,
    last_tick: &mut Instant,
) -> io::Result<Option<String>> {
    // Keep the dashboard cheap: refresh processes + fs scan on a slower cadence.
    let dash_proc_every = Duration::from_secs(3);
    let dash_history_every = tick_rate.max(Duration::from_millis(500));
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
            if matches!(app.screen, Screen::Services) {
                refresh_services(app, false);
            }
            if matches!(app.screen, Screen::Logs) {
                refresh_logs(app, false);
            }
            *last_tick = Instant::now();

            if tip_clock.elapsed() >= Duration::from_secs(12) {
                app.footer_tip_idx = app.footer_tip_idx.wrapping_add(1);
                tip_clock = Instant::now();
            }
        }

        let vm = snapshot(system);
        if matches!(app.screen, Screen::Dashboard) {
            let due = app
                .dash_last_history_at
                .map(|t| t.elapsed() >= dash_history_every)
                .unwrap_or(true);
            if due {
                push_history_sample(&mut app.dash_cpu_history, vm.cpu_usage as f64, 48);
                push_history_sample(&mut app.dash_mem_history, vm.memory_percent, 48);
                app.dash_last_history_at = Some(Instant::now());
            }
        }

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
                Screen::Dashboard => render_dashboard(frame, rows[1], &vm, app, system, disks),
                Screen::Processes => render_processes(frame, rows[1], app, system),
                Screen::DiskDive => render_disk_dive(frame, rows[1], app),
                Screen::Services => render_services(frame, rows[1], app),
                Screen::Logs => render_logs(frame, rows[1], app),
            }

            // Footer/help
            if app.show_help {
                frame.render_widget(render_help(app), rows[2]);
            } else {
                frame.render_widget(render_footer(app), rows[2]);
            }
        })?;

        if app.dump_snapshot {
            app.dump_snapshot = false;
            return Ok(Some(format_snapshot(&vm, app, system, disks)));
        }

        if app.do_update {
            app.do_update = false;
            let latest = app.update.latest_tag.clone().unwrap_or_default();
            if latest.is_empty() {
                return Ok(Some("No latest tag found".to_string()));
            }
            return Ok(Some(match perform_self_update(&latest) {
                Ok(msg) => msg,
                Err(e) => format!("Update failed: {e}"),
            }));
        }

        // Input
        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                // Avoid key-repeat spam on some terminals
                if key.kind != KeyEventKind::Press {
                    continue;
                }

                // Kill confirm mode intercepts all keys.
                if app.proc_kill_confirm.is_some() {
                    let pid = app.proc_kill_confirm.as_ref().map(|(p, _)| *p).unwrap();
                    match key.code {
                        KeyCode::Char('y') => {
                            if let Some(proc) = system.process(sysinfo::Pid::from_u32(pid as u32)) {
                                let _ = proc.kill_with(sysinfo::Signal::Term);
                            }
                            app.proc_kill_confirm = None;
                        }
                        KeyCode::Char('K') => {
                            if let Some(proc) = system.process(sysinfo::Pid::from_u32(pid as u32)) {
                                proc.kill();
                            }
                            app.proc_kill_confirm = None;
                        }
                        KeyCode::Char('n') | KeyCode::Esc => {
                            app.proc_kill_confirm = None;
                        }
                        _ => {}
                    }
                    continue;
                }

                if handle_proc_search_key(app, &key) {
                    continue;
                }

                if handle_service_search_key(app, &key) {
                    continue;
                }

                match key.code {
                    KeyCode::Char('q') => return Ok(None),
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
                    KeyCode::Char('v') => {
                        app.show_help = false;
                        app.screen = Screen::Services;
                        refresh_services(app, true);
                    }
                    KeyCode::Char('l') => {
                        app.show_help = false;
                        if matches!(app.screen, Screen::Services) {
                            open_logs_for_selected_service(app);
                        } else {
                            app.screen = Screen::Logs;
                            refresh_logs(app, true);
                        }
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
                        if matches!(app.screen, Screen::Services) {
                            refresh_services(app, true);
                        }
                        if matches!(app.screen, Screen::Logs) {
                            refresh_logs(app, true);
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
                        } else if matches!(app.screen, Screen::Services) {
                            app.service_scroll = app.service_scroll.saturating_sub(1);
                        } else if matches!(app.screen, Screen::Logs) {
                            app.logs_scroll = app.logs_scroll.saturating_sub(1);
                        }
                    }
                    KeyCode::Down => {
                        if matches!(app.screen, Screen::Processes) {
                            app.proc_scroll = app.proc_scroll.saturating_add(1);
                        } else if matches!(app.screen, Screen::DiskDive) {
                            app.disk_scroll = app.disk_scroll.saturating_add(1);
                        } else if matches!(app.screen, Screen::Services) {
                            app.service_scroll = app.service_scroll.saturating_add(1);
                        } else if matches!(app.screen, Screen::Logs) {
                            app.logs_scroll = app.logs_scroll.saturating_add(1);
                        }
                    }

                    // Tab is contextual.
                    KeyCode::Tab => {
                        if matches!(app.screen, Screen::Dashboard) {
                            app.dash_dir_target = app.dash_dir_target.next();
                            // Force refresh of quick scan.
                            app.dash_last_fs_at = None;
                        } else if matches!(app.screen, Screen::DiskDive) {
                            app.disk_target = match app.disk_target {
                                DiskTarget::Var => DiskTarget::Home,
                                DiskTarget::Home => DiskTarget::Root,
                                DiskTarget::Root => DiskTarget::Var,
                            };
                            app.disk_scroll = 0;
                            let mut state = app.disk_scan.inner.lock().unwrap();
                            state.current_path = None;
                            state.results.clear();
                            state.error = None;
                            state.progress.clear();
                        } else if matches!(app.screen, Screen::Processes) {
                            app.proc_sort = match app.proc_sort {
                                ProcSort::Cpu => ProcSort::Mem,
                                ProcSort::Mem => ProcSort::Cpu,
                            };
                            app.proc_scroll = 0;
                        } else if matches!(app.screen, Screen::Services) {
                            app.service_filter = match app.service_filter {
                                ServiceFilter::Failed => ServiceFilter::Unhealthy,
                                ServiceFilter::Unhealthy => ServiceFilter::Active,
                                ServiceFilter::Active => ServiceFilter::All,
                                ServiceFilter::All => ServiceFilter::Failed,
                            };
                            app.service_scroll = 0;
                        } else if matches!(app.screen, Screen::Logs) {
                            app.log_severity = match app.log_severity {
                                LogSeverity::Errors => LogSeverity::Warnings,
                                LogSeverity::Warnings => LogSeverity::Info,
                                LogSeverity::Info => LogSeverity::Debug,
                                LogSeverity::Debug => LogSeverity::Errors,
                            };
                            app.logs_scroll = 0;
                            refresh_logs(app, true);
                        }
                    }
                    KeyCode::Char('k') => {
                        if matches!(app.screen, Screen::Processes) && !app.proc_search_active {
                            // Rebuild filtered list to get the PID at the cursor.
                            let mut procs: Vec<ProcRow> = system
                                .processes()
                                .iter()
                                .map(|(pid, p)| ProcRow::from_process(*pid, p))
                                .collect();
                            match app.proc_sort {
                                ProcSort::Cpu => procs.sort_by_key(|p| Reverse((p.cpu_x10 as i64, p.mem_bytes as i64))),
                                ProcSort::Mem => procs.sort_by_key(|p| Reverse((p.mem_bytes as i64, p.cpu_x10 as i64))),
                            }
                            if procs.len() > 200 { procs.truncate(200); }
                            let procs = filtered_proc_rows(procs, &app.proc_search);
                            let idx = (app.proc_scroll as usize).min(procs.len().saturating_sub(1));
                            if let Some(row) = procs.get(idx) {
                                app.proc_kill_confirm = Some((row.pid, row.name.clone()));
                            }
                        }
                    }
                    KeyCode::Char('s') => {
                        if matches!(app.screen, Screen::DiskDive) {
                            start_disk_scan(app);
                        }
                    }
                    KeyCode::Enter => {
                        if matches!(app.screen, Screen::DiskDive) {
                            enter_selected_disk_dir(app);
                        } else if matches!(app.screen, Screen::Services) {
                            open_logs_for_selected_service(app);
                        }
                    }
                    KeyCode::Left | KeyCode::Backspace => {
                        if matches!(app.screen, Screen::DiskDive) {
                            navigate_disk_up(app);
                        }
                    }
                    KeyCode::Char('f') => {
                        if matches!(app.screen, Screen::Dashboard) {
                            app.dash_show_all_mounts = !app.dash_show_all_mounts;
                            app.dash_last_fs_at = None;
                        }
                    }
                    KeyCode::Char('x') => {
                        if matches!(app.screen, Screen::Dashboard) {
                            app.dump_snapshot = true;
                        }
                    }
                    KeyCode::Char('u') => {
                        if matches!(app.screen, Screen::Dashboard) && app.update.available {
                            app.do_update = true;
                        } else if matches!(app.screen, Screen::Logs) {
                            app.log_unit_filter = match app.log_unit_filter {
                                LogUnitFilter::Selected => LogUnitFilter::All,
                                LogUnitFilter::All => LogUnitFilter::Selected,
                            };
                            app.logs_scroll = 0;
                            refresh_logs(app, true);
                        }
                    }

                    _ => {}
                }
            }
        }
    }
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
    load_avg_one: f64,
    total_memory: u64,
    used_memory: u64,
    available_memory: u64,
    memory_percent: f64,
    total_swap: u64,
    used_swap: u64,
}

fn snapshot(system: &System) -> VmSnapshot {
    let cpu_usage = system.global_cpu_info().cpu_usage();
    let cpu_cores = system.cpus().len();
    let load_avg_one = System::load_average().one;
    // sysinfo reports memory in bytes
    let total_memory = system.total_memory();
    let used_memory = system.used_memory();
    let available_memory = system.available_memory();
    let memory_percent = percent(used_memory, total_memory);
    let total_swap = system.total_swap();
    let used_swap = system.used_swap();

    VmSnapshot {
        cpu_usage,
        cpu_cores,
        load_avg_one,
        total_memory,
        used_memory,
        available_memory,
        memory_percent,
        total_swap,
        used_swap,
    }
}

fn render_header(app: &AppState) -> Paragraph<'static> {
    let (screen_name, screen_hint) = match app.screen {
        Screen::Dashboard => ("Dashboard", "p: processes  d: disk  v: services  l: logs"),
        Screen::Processes => ("Processes", "Tab: sort CPU/Mem  Esc: back"),
        Screen::DiskDive => ("Disk dive", "s: scan  Enter: open dir  ←: up  Tab: target"),
        Screen::Services => (
            "Services",
            "Tab: filter  /: search  Enter/l: logs  r: refresh",
        ),
        Screen::Logs => ("Logs", "Tab: severity  u: unit filter  r: refresh"),
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
        "Tab: cycle dir target (CWD ↔ /var ↔ HOME ↔ /)",
        "f: toggle mount filter (filtered ↔ all)",
        "p: processes · d: disk dive · v: services · l: logs",
        "r: refresh now · ?: help",
        "Esc: back to dashboard",
    ];

    let tips_processes = ["Tab: sort CPU ↔ Mem", "↑/↓: scroll · k: kill", "Esc: back"];

    let tips_disk = [
        "s: scan (on-demand)",
        "Tab: change target (/var ↔ home ↔ /)",
        "Enter: open dir · ←/Backspace: up",
        "↑/↓: select · Esc: back",
    ];

    let tips_services = [
        "Tab: filter failed/unhealthy/active/all",
        "/: search unit or description",
        "Esc clears search, then returns to dashboard",
        "Enter or l: open logs for selected unit",
        "↑/↓: select unit · r: refresh",
    ];

    let tips_logs = [
        "Tab: cycle severity err+/warning+/info+/debug+",
        "u: selected unit ↔ all units",
        "↑/↓: scroll · r: refresh",
    ];

    let (label, tip) = match app.screen {
        Screen::Dashboard => {
            let idx = app.footer_tip_idx as usize % (tips_dashboard.len() + 1);
            if idx == tips_dashboard.len() {
                ("Info", format!("Refresh rate: {}ms", app.tick_ms))
            } else {
                ("Tip", tips_dashboard[idx].to_string())
            }
        }
        Screen::Processes => (
            "Tip",
            tips_processes[(app.footer_tip_idx as usize) % tips_processes.len()].to_string(),
        ),
        Screen::DiskDive => (
            "Tip",
            tips_disk[(app.footer_tip_idx as usize) % tips_disk.len()].to_string(),
        ),
        Screen::Services => (
            "Tip",
            tips_services[(app.footer_tip_idx as usize) % tips_services.len()].to_string(),
        ),
        Screen::Logs => (
            "Tip",
            tips_logs[(app.footer_tip_idx as usize) % tips_logs.len()].to_string(),
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
        Line::from("  v — services"),
        Line::from("  l — logs"),
        Line::from(""),
    ];

    match app.screen {
        Screen::Dashboard => {
            lines.push(Line::from("Dashboard:"));
            lines.push(Line::from("  p — processes"));
            lines.push(Line::from("  d — disk dive"));
            lines.push(Line::from("  f — toggle mount filter (filtered ↔ all)"));
            lines.push(Line::from(
                "  Tab — cycle dir target (CWD ↔ /var ↔ HOME ↔ /)",
            ));
        }
        Screen::Processes => {
            lines.push(Line::from("Processes:"));
            lines.push(Line::from("  Tab — toggle CPU/Mem list"));
            lines.push(Line::from("  ↑/↓ — scroll · / — search"));
            lines.push(Line::from("  k — kill selected (y=SIGTERM, K=SIGKILL)"));
        }
        Screen::DiskDive => {
            lines.push(Line::from("Disk dive:"));
            lines.push(Line::from("  s — start scan"));
            lines.push(Line::from("  Tab — change target (/var ↔ home ↔ /)"));
            lines.push(Line::from("  ↑/↓ — select"));
            lines.push(Line::from("  Enter — scan selected directory"));
            lines.push(Line::from("  ← / Backspace — go to parent directory"));
        }
        Screen::Services => {
            lines.push(Line::from("Services (Linux-only):"));
            lines.push(Line::from(
                "  Tab — cycle filters (failed ↔ unhealthy ↔ active ↔ all)",
            ));
            lines.push(Line::from("  / — search by service name or description"));
            lines.push(Line::from(
                "  Backspace — edit search · Esc — clear search/back",
            ));
            lines.push(Line::from("  ↑/↓ — select service"));
            lines.push(Line::from("  Enter / l — open logs for selected unit"));
            lines.push(Line::from("  r — refresh service list"));
        }
        Screen::Logs => {
            lines.push(Line::from("Logs (Linux-only):"));
            lines.push(Line::from(
                "  Tab — cycle severity (err+ ↔ warning+ ↔ info+ ↔ debug+)",
            ));
            lines.push(Line::from("  u — selected unit ↔ all units"));
            lines.push(Line::from("  ↑/↓ — scroll"));
            lines.push(Line::from("  r — refresh logs"));
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
    disks: &Disks,
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
        app.dash_top_cpu = format_top_processes(system, ProcSort::Cpu, 5);
        app.dash_top_mem = format_top_processes(system, ProcSort::Mem, 5);
        app.dash_mem_pressure = format_memory_pressure(system, 5);
        app.dash_mount_rows = collect_mount_rows(12, app.dash_show_all_mounts)
            .unwrap_or_else(|| disks_table_filtered(disks, 12, app.dash_show_all_mounts));
        let (label, path) = dash_target_path(app.dash_dir_target);
        app.dash_dir_sizes = scan_dir_quick(&path, 6);
        if !app.dash_dir_sizes.is_empty() {
            app.dash_dir_sizes.insert(0, label);
        } else {
            app.dash_dir_sizes = vec![label, "(no entries)".to_string()];
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
    let cpu_sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(48), Constraint::Percentage(52)])
        .split(cpu_inner);

    let cpu_summary_block = Block::default().borders(Borders::ALL).title("Summary");
    frame.render_widget(cpu_summary_block.clone(), cpu_sections[0]);
    let cpu_summary_inner = cpu_summary_block.inner(cpu_sections[0]);
    let cpu_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(cpu_summary_inner);

    let cpu_pct_color = color_for_pct(vm.cpu_usage as f64);
    let cpu_lines = vec![
        Line::from(vec![
            Span::styled("CPU ", Style::default().fg(Color::Gray)),
            Span::styled(
                format!("{:.1}%", vm.cpu_usage),
                Style::default().fg(cpu_pct_color),
            ),
        ]),
        Line::from(vec![
            Span::styled("Load ", Style::default().fg(Color::Gray)),
            Span::styled(
                format!("{:.2}", vm.load_avg_one),
                Style::default().fg(Color::White),
            ),
            Span::raw("  "),
            Span::styled("Cores ", Style::default().fg(Color::Gray)),
            Span::styled(
                format!("{}", vm.cpu_cores),
                Style::default().fg(Color::White),
            ),
        ]),
    ];
    let cpu_paragraph = Paragraph::new(cpu_lines).alignment(Alignment::Left);
    frame.render_widget(cpu_paragraph, cpu_chunks[0]);

    let cpu_gauge = Gauge::default()
        .gauge_style(Style::default().fg(cpu_pct_color))
        .label("")
        .ratio(((vm.cpu_usage as f64) / 100.0).clamp(0.0, 1.0));
    frame.render_widget(cpu_gauge, cpu_chunks[1]);

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
        {
            let cpus = system.cpus();
            if !cpus.is_empty() {
                lines.push(Line::from(""));
                lines.push(Line::from(vec![Span::styled(
                    "Per-core CPU",
                    Style::default()
                        .fg(Color::Gray)
                        .add_modifier(Modifier::BOLD),
                )]));
                let bar_width = 16usize;
                for (idx, cpu) in cpus.iter().enumerate().take(24) {
                    let pct = cpu.cpu_usage() as f64;
                    let filled = ((pct / 100.0) * bar_width as f64).round() as usize;
                    let empty = bar_width.saturating_sub(filled);
                    let bar = format!("{}{}", "█".repeat(filled), "░".repeat(empty));
                    let color = color_for_pct(pct);
                    lines.push(Line::from(vec![
                        Span::styled(
                            format!("c{:<2} ", idx),
                            Style::default().fg(Color::Gray),
                        ),
                        Span::styled(bar, Style::default().fg(color)),
                        Span::styled(
                            format!(" {:>3.0}%", pct),
                            Style::default().fg(color),
                        ),
                    ]));
                }
            }
        }
        lines
    };
    frame.render_widget(
        Paragraph::new(cpu_bottom).alignment(Alignment::Left),
        cpu_chunks[2],
    );

    render_detail_panel(
        frame,
        cpu_sections[1],
        "Signals",
        vec![
            format!("Now {:.1}%", vm.cpu_usage),
            format!("Peak {}%", history_peak(&app.dash_cpu_history)),
            format!("Recent avg {:.1}%", history_average(&app.dash_cpu_history)),
            format!("Headroom {:.1}%", (100.0 - vm.cpu_usage as f64).max(0.0)),
        ],
        &app.dash_cpu_history,
        cpu_pct_color,
    );

    // Memory
    let memory_block = Block::default()
        .title("Memory")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Magenta));
    frame.render_widget(memory_block.clone(), panels[1]);

    let memory_inner = memory_block.inner(panels[1]);
    let memory_sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(48), Constraint::Percentage(52)])
        .split(memory_inner);

    let memory_summary_block = Block::default().borders(Borders::ALL).title("Summary");
    frame.render_widget(memory_summary_block.clone(), memory_sections[0]);
    let memory_summary_inner = memory_summary_block.inner(memory_sections[0]);
    let memory_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(memory_summary_inner);

    let mem_pct_color = color_for_pct(vm.memory_percent);
    let memory_lines = vec![
        Line::from(vec![
            Span::styled("Mem ", Style::default().fg(Color::Gray)),
            Span::styled(
                format!(
                    "{} / {}",
                    format_bytes(vm.used_memory),
                    format_bytes(vm.total_memory)
                ),
                Style::default().fg(mem_pct_color),
            ),
        ]),
        Line::from(vec![
            Span::styled("Avail ", Style::default().fg(Color::Gray)),
            Span::styled(
                format_bytes(vm.available_memory),
                Style::default().fg(Color::White),
            ),
            Span::raw("  "),
            Span::styled("Use ", Style::default().fg(Color::Gray)),
            Span::styled(
                format!("{:.1}%", vm.memory_percent),
                Style::default().fg(mem_pct_color),
            ),
        ]),
        Line::from(vec![
            Span::styled("Swap ", Style::default().fg(Color::Gray)),
            Span::styled(
                if vm.total_swap > 0 {
                    format!(
                        "{}/{}",
                        format_bytes(vm.used_swap),
                        format_bytes(vm.total_swap)
                    )
                } else {
                    "off".to_string()
                },
                Style::default().fg(Color::White),
            ),
        ]),
    ];
    let memory_paragraph = Paragraph::new(memory_lines).alignment(Alignment::Left);
    frame.render_widget(memory_paragraph, memory_chunks[0]);

    let memory_gauge = Gauge::default()
        .gauge_style(Style::default().fg(mem_pct_color))
        .label("")
        .ratio((vm.memory_percent / 100.0).clamp(0.0, 1.0));
    frame.render_widget(memory_gauge, memory_chunks[1]);

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
        if !app.dash_mem_pressure.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![Span::styled(
                "Pressure",
                Style::default()
                    .fg(Color::Gray)
                    .add_modifier(Modifier::BOLD),
            )]));
            for row in &app.dash_mem_pressure {
                lines.push(Line::from(row.clone()));
            }
        }
        lines
    };
    frame.render_widget(
        Paragraph::new(mem_bottom).alignment(Alignment::Left),
        memory_chunks[2],
    );

    let mut memory_signals = vec![
        format!("Now {:.1}%", vm.memory_percent),
        format!("Peak {}%", history_peak(&app.dash_mem_history)),
        format!("Recent avg {:.1}%", history_average(&app.dash_mem_history)),
        format!("Headroom {}", format_bytes(vm.available_memory)),
    ];
    memory_signals.push(if vm.total_swap > 0 {
        format!(
            "Swap {:.0}% ({}/{})",
            percent(vm.used_swap, vm.total_swap),
            format_bytes(vm.used_swap),
            format_bytes(vm.total_swap)
        )
    } else {
        "Swap off".to_string()
    });
    render_detail_panel(
        frame,
        memory_sections[1],
        "Signals",
        memory_signals,
        &app.dash_mem_history,
        mem_pct_color,
    );

    // Disk
    let disk_block = Block::default()
        .title("Disk")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Green));
    frame.render_widget(disk_block.clone(), panels[2]);

    let disk_inner = disk_block.inner(panels[2]);
    let disk_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(48), Constraint::Percentage(52)])
        .split(disk_inner);

    let mounts_title = if app.dash_show_all_mounts {
        "Mounts (all)"
    } else {
        "Mounts (filtered)"
    };
    let df_rows = app.dash_mount_rows.iter().map(|r| {
        Row::new(vec![
            Cell::from(trim_to(&r.mount, 14)),
            Cell::from(Span::styled(
                format!("{:.0}%", r.use_pct),
                Style::default().fg(color_for_pct(r.use_pct)),
            )),
            Cell::from(format!("{}/{}", format_bytes(r.used), format_bytes(r.size))),
            Cell::from(trim_to(&r.fs, 14)),
        ])
    });

    let df = Table::new(
        df_rows,
        [
            Constraint::Length(14),
            Constraint::Length(6),
            Constraint::Length(18),
            Constraint::Min(12),
        ],
    )
    .header(
        Row::new(vec!["MOUNT", "USE", "USED/TOTAL", "FS"]).style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
    )
    .block(Block::default().borders(Borders::ALL).title(mounts_title));

    frame.render_widget(df, disk_chunks[0]);

    let dir_title = app.dash_dir_target.title();
    let mut dir_lines: Vec<Line> = Vec::new();
    if let Some((path, entries)) = app.dash_dir_sizes.split_first() {
        dir_lines.push(Line::from(vec![
            Span::styled("Path: ", Style::default().fg(Color::Gray)),
            Span::styled(path.clone(), Style::default().fg(Color::White)),
        ]));
        for row in entries {
            dir_lines.push(Line::from(Span::raw(row.clone())));
        }
    } else {
        dir_lines.push(Line::from(Span::styled(
            "Path: unavailable",
            Style::default().fg(Color::Gray),
        )));
        dir_lines.push(Line::from(Span::styled(
            "(no entries)",
            Style::default().fg(Color::Gray),
        )));
    }

    frame.render_widget(
        Paragraph::new(dir_lines)
            .block(Block::default().borders(Borders::ALL).title(dir_title))
            .alignment(Alignment::Left)
            .wrap(Wrap { trim: false }),
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

    // Apply search filter; when active, results are sorted by name for stability
    let procs = filtered_proc_rows(procs, &app.proc_search);

    let header_title = if app.proc_search_active {
        format!(
            "Processes ({})  /{}_",
            match app.proc_sort { ProcSort::Cpu => "CPU", ProcSort::Mem => "Mem" },
            trim_to(&app.proc_search, 24)
        )
    } else if app.proc_search.is_empty() {
        match app.proc_sort {
            ProcSort::Cpu => "Top processes (CPU)".to_string(),
            ProcSort::Mem => "Top processes (Memory)".to_string(),
        }
    } else {
        format!(
            "Processes ({})  /{}  ({} match{})",
            match app.proc_sort { ProcSort::Cpu => "CPU", ProcSort::Mem => "Mem" },
            trim_to(&app.proc_search, 24),
            procs.len(),
            if procs.len() == 1 { "" } else { "es" }
        )
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

    // Calculate available width for process name column
    let name_width = ((inner.width.saturating_sub(8 + 10 + 14 + 4)) as usize).max(20);

    let rows = slice.iter().enumerate().map(|(i, p)| {
        let row = Row::new(vec![
            Cell::from(p.pid.to_string()),
            Cell::from(trim_to(&p.name, name_width)),
            Cell::from(format!("{:.1}%", p.cpu_x10 as f64 / 10.0)),
            Cell::from(format_bytes(p.mem_bytes)),
        ]);
        if i == 0 {
            row.style(Style::default().fg(Color::Black).bg(Color::Cyan))
        } else {
            row
        }
    });

    let table = Table::new(
        rows,
        [
            Constraint::Length(8),
            Constraint::Min(20),
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
    let hint = if app.proc_search_active {
        Paragraph::new(Line::from(vec![
            Span::styled("Enter", Style::default().fg(Color::Yellow)),
            Span::raw(" confirm · "),
            Span::styled("Esc", Style::default().fg(Color::Yellow)),
            Span::raw(" clear · typing: "),
            Span::styled(
                format!("/{}", trim_to(&app.proc_search, 28)),
                Style::default().fg(Color::Green),
            ),
        ]))
    } else if !app.proc_search.is_empty() {
        Paragraph::new(Line::from(vec![
            Span::styled("Tab", Style::default().fg(Color::Yellow)),
            Span::raw(" sort · "),
            Span::styled("/", Style::default().fg(Color::Yellow)),
            Span::raw(" search · "),
            Span::styled("Esc", Style::default().fg(Color::Yellow)),
            Span::raw(" clear filter"),
        ]))
    } else {
        Paragraph::new(Line::from(vec![
            Span::styled("Tab", Style::default().fg(Color::Yellow)),
            Span::raw(" CPU/Mem · "),
            Span::styled("↑/↓", Style::default().fg(Color::Yellow)),
            Span::raw(" scroll · "),
            Span::styled("/", Style::default().fg(Color::Yellow)),
            Span::raw(" search · "),
            Span::styled("k", Style::default().fg(Color::Red)),
            Span::raw(" kill · top "),
            Span::styled(max_rows.to_string(), Style::default().fg(Color::White)),
        ]))
    };
    let hint = hint.alignment(Alignment::Left);

    let hint_area = Rect {
        x: inner.x,
        y: inner.y + inner.height.saturating_sub(1),
        width: inner.width,
        height: 1,
    };
    frame.render_widget(hint, hint_area);

    // Kill confirmation overlay
    if let Some((pid, name)) = &app.proc_kill_confirm {
        let popup = centered_rect(54, 5, area);
        frame.render_widget(Clear, popup);
        let block = Block::default()
            .title(" Kill Process ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Red));
        let inner_popup = block.inner(popup);
        frame.render_widget(block, popup);
        let text = vec![
            Line::from(vec![
                Span::raw("  "),
                Span::styled(trim_to(name, 28), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                Span::raw(format!("  PID {}", pid)),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::raw("  "),
                Span::styled("y", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::raw(" SIGTERM  "),
                Span::styled("K", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                Span::raw(" SIGKILL  "),
                Span::styled("n/Esc", Style::default().fg(Color::Yellow)),
                Span::raw(" cancel"),
            ]),
        ];
        frame.render_widget(Paragraph::new(text), inner_popup);
    }
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
    let current_path = state.current_path.clone().unwrap_or_else(|| target.clone());

    let title = if state.running {
        format!(
            "Disk dive  (target: {})  •  scanning",
            current_path.display()
        )
    } else {
        format!("Disk dive  (target: {})", current_path.display())
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
            Span::styled(
                " to scan this directory · ",
                Style::default().fg(Color::Gray),
            ),
            Span::styled("Tab", Style::default().fg(Color::Yellow)),
            Span::styled(" to change target", Style::default().fg(Color::Gray)),
        ])
    } else {
        Line::from(vec![
            Span::styled("Cached results. ", Style::default().fg(Color::Gray)),
            Span::styled("s", Style::default().fg(Color::Yellow)),
            Span::styled(" rescan · ", Style::default().fg(Color::Gray)),
            Span::styled("Enter", Style::default().fg(Color::Yellow)),
            Span::styled(" open dir · ", Style::default().fg(Color::Gray)),
            Span::styled("←", Style::default().fg(Color::Yellow)),
            Span::styled(" up · ", Style::default().fg(Color::Gray)),
            Span::styled("↑/↓", Style::default().fg(Color::Yellow)),
            Span::styled(" select", Style::default().fg(Color::Gray)),
        ])
    };

    let status = Paragraph::new(vec![status_line]).alignment(Alignment::Left);
    frame.render_widget(status, rows[0]);

    // Results table
    let mut results = state.results.clone();
    drop(state);
    results.sort_by_key(|entry| Reverse(entry.bytes));

    let visible = rows[1].height.saturating_sub(2) as usize; // table header + borders
    let selected = (app.disk_scroll as usize).min(results.len().saturating_sub(1));
    app.disk_scroll = selected as u16;
    let offset = selected.saturating_sub(visible.saturating_sub(1));
    let slice = &results[offset..results.len().min(offset + visible.max(1))];

    let table_rows = slice.iter().enumerate().map(|(i, entry)| {
        let absolute_idx = offset + i;
        let base_style = if absolute_idx.is_multiple_of(2) {
            Style::default().fg(Color::White)
        } else {
            Style::default().fg(Color::Gray)
        };
        let style = if absolute_idx == selected {
            base_style
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            base_style
        };
        let name = entry
            .path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| entry.path.display().to_string());
        let kind = match entry.kind {
            DiskEntryKind::Directory => "dir",
            DiskEntryKind::File => "file",
        };

        Row::new(vec![
            Cell::from(kind),
            Cell::from(name),
            Cell::from(format_bytes(entry.bytes)),
        ])
        .style(style)
    });

    let table = Table::new(
        table_rows,
        [
            Constraint::Length(6),
            Constraint::Percentage(66),
            Constraint::Length(14),
        ],
    )
    .header(
        Row::new(vec!["Kind", "Name", "Size"]).style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
    )
    .block(
        Block::default()
            .title("Largest entries")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Green)),
    );

    frame.render_widget(table, rows[1]);
}

fn render_services(frame: &mut ratatui::Frame, area: Rect, app: &mut AppState) {
    let state = app.service_state.inner.lock().unwrap();

    if let Some(msg) = &state.unsupported {
        frame.render_widget(
            Paragraph::new(vec![
                Line::from("Services"),
                Line::from(""),
                Line::from(msg.clone()),
                Line::from(""),
                Line::from("You can still ship this build and test the Linux path on a server."),
            ])
            .block(Block::default().title("Services").borders(Borders::ALL))
            .alignment(Alignment::Center),
            area,
        );
        return;
    }

    let rows = filtered_service_rows(&state.rows, app.service_filter, &app.service_search);
    let error = state.error.clone();
    let failed = state
        .rows
        .iter()
        .filter(|row| row.health == ServiceHealth::Critical)
        .count();
    let unhealthy = state
        .rows
        .iter()
        .filter(|row| row.health == ServiceHealth::Warning)
        .count();
    let active = state
        .rows
        .iter()
        .filter(|row| row.active_state == "active")
        .count();
    let updated = state
        .last_updated_at
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| {
            format!(
                "updated {}s ago",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .ok()
                    .map(|now| now.as_secs().saturating_sub(d.as_secs()))
                    .unwrap_or(0)
            )
        })
        .unwrap_or_else(|| "not loaded yet".to_string());
    let selected = rows.get(app.service_scroll as usize).cloned();
    drop(state);

    let title = if app.service_search_active {
        format!(
            "Services ({})  /{}_",
            service_filter_label(app.service_filter),
            trim_to(&app.service_search, 18)
        )
    } else if app.service_search.is_empty() {
        format!("Services ({})", service_filter_label(app.service_filter))
    } else {
        format!(
            "Services ({})  /{}",
            service_filter_label(app.service_filter),
            trim_to(&app.service_search, 18)
        )
    };
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Blue));
    frame.render_widget(block.clone(), area);
    let inner = block.inner(area);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(6),
            Constraint::Length(5),
        ])
        .split(inner);

    let summary = Paragraph::new(vec![Line::from(vec![
        Span::styled(
            "Failed ",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ),
        Span::raw(failed.to_string()),
        Span::raw("  "),
        Span::styled(
            "Warning ",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(unhealthy.to_string()),
        Span::raw("  "),
        Span::styled(
            "Active ",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(active.to_string()),
        Span::raw("  •  "),
        Span::styled(updated, Style::default().fg(Color::Gray)),
        if app.service_search_active {
            Span::styled(
                format!("  •  typing /{}", trim_to(&app.service_search, 20)),
                Style::default().fg(Color::Gray),
            )
        } else if app.service_search.is_empty() {
            Span::raw("")
        } else {
            Span::styled(
                format!("  •  search /{}", trim_to(&app.service_search, 20)),
                Style::default().fg(Color::Gray),
            )
        },
        if rows.is_empty() {
            Span::styled("  •  no matching units", Style::default().fg(Color::Gray))
        } else {
            Span::styled(
                format!("  •  showing {}", rows.len()),
                Style::default().fg(Color::Gray),
            )
        },
    ])]);
    frame.render_widget(summary, chunks[0]);

    let visible = chunks[1].height.saturating_sub(3) as usize;
    let selected_idx = (app.service_scroll as usize).min(rows.len().saturating_sub(1));
    app.service_scroll = selected_idx as u16;
    let offset = selected_idx.saturating_sub(visible.saturating_sub(1));
    let slice = &rows[offset..rows.len().min(offset + visible.max(1))];

    let table_rows = slice.iter().enumerate().map(|(i, row)| {
        let absolute_idx = offset + i;
        let base_style = match row.health {
            ServiceHealth::Critical => Style::default().fg(Color::Red),
            ServiceHealth::Warning => Style::default().fg(Color::Yellow),
            ServiceHealth::Healthy => Style::default().fg(Color::Green),
        };
        let style = if absolute_idx == selected_idx {
            base_style
                .bg(Color::White)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD)
        } else {
            base_style
        };
        Row::new(vec![
            Cell::from(trim_to(&row.name, 26)),
            Cell::from(format!("{} ({})", row.active_state, row.sub_state)),
            Cell::from(row.restarts.to_string()),
            Cell::from(trim_to(&row.last_change, 24)),
            Cell::from(trim_to(&row.description, 28)),
        ])
        .style(style)
    });

    let table = Table::new(
        table_rows,
        [
            Constraint::Length(26),
            Constraint::Length(20),
            Constraint::Length(8),
            Constraint::Length(24),
            Constraint::Min(18),
        ],
    )
    .header(
        Row::new(vec!["UNIT", "STATE", "RESTARTS", "LAST CHANGE", "DESC"]).style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
    )
    .block(Block::default().borders(Borders::ALL).title("Units"));
    frame.render_widget(table, chunks[1]);

    let detail_lines = if let Some(error) = error {
        vec![
            Line::from(vec![
                Span::styled(
                    "Service refresh failed: ",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ),
                Span::raw(error),
            ]),
            Line::from("Press r to retry."),
        ]
    } else if let Some(row) = selected {
        vec![
            Line::from(vec![
                Span::styled("Selected: ", Style::default().fg(Color::Gray)),
                Span::raw(row.name),
                Span::raw("  •  "),
                Span::styled("Load ", Style::default().fg(Color::Gray)),
                Span::raw(row.load_state),
            ]),
            Line::from(vec![
                Span::styled("State: ", Style::default().fg(Color::Gray)),
                Span::raw(format!("{} ({})", row.active_state, row.sub_state)),
                Span::raw("  •  "),
                Span::styled("Restarts ", Style::default().fg(Color::Gray)),
                Span::raw(row.restarts.to_string()),
            ]),
            Line::from(vec![
                Span::styled("When: ", Style::default().fg(Color::Gray)),
                Span::raw(row.last_change),
            ]),
            Line::from(vec![
                Span::styled("Hint: ", Style::default().fg(Color::Gray)),
                Span::raw("press Enter or l to tail logs for this service"),
            ]),
        ]
    } else {
        let mut lines = vec![Line::from("No services match the current filter.")];
        if !app.service_search.is_empty() {
            lines.push(Line::from(format!("Search: /{}", app.service_search)));
            lines.push(Line::from("Press Backspace or Esc to clear the search."));
        } else {
            lines.push(Line::from(
                "Press / to search by service name or description.",
            ));
        }
        lines.push(Line::from("Press Tab to change the filter."));
        lines
    };
    frame.render_widget(
        Paragraph::new(detail_lines)
            .block(Block::default().borders(Borders::ALL).title("Detail"))
            .wrap(Wrap { trim: true }),
        chunks[2],
    );
}

fn render_logs(frame: &mut ratatui::Frame, area: Rect, app: &mut AppState) {
    let state = app.log_state.inner.lock().unwrap();

    if let Some(msg) = &state.unsupported {
        frame.render_widget(
            Paragraph::new(vec![
                Line::from("Logs"),
                Line::from(""),
                Line::from(msg.clone()),
                Line::from(""),
                Line::from("On Linux this uses journalctl first and falls back to syslog."),
            ])
            .block(Block::default().title("Logs").borders(Borders::ALL))
            .alignment(Alignment::Center),
            area,
        );
        return;
    }

    let source = if state.source.is_empty() {
        "journalctl".to_string()
    } else {
        state.source.clone()
    };
    let lines = state.lines.clone();
    let err = state.error.clone();
    let running = state.running;
    drop(state);

    let block = Block::default()
        .title("Logs")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Magenta));
    frame.render_widget(block.clone(), area);
    let inner = block.inner(area);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(4)])
        .split(inner);

    let header = Paragraph::new(vec![Line::from(vec![
        Span::styled("Severity ", Style::default().fg(Color::Gray)),
        Span::raw(log_severity_label(app.log_severity)),
        Span::raw("  •  "),
        Span::styled("Unit ", Style::default().fg(Color::Gray)),
        Span::raw(log_unit_filter_label(
            app.log_unit_filter,
            app.log_selected_unit.as_deref(),
        )),
        Span::raw("  •  "),
        Span::styled("Source ", Style::default().fg(Color::Gray)),
        Span::raw(source),
        if running {
            Span::styled("  •  refreshing", Style::default().fg(Color::Yellow))
        } else {
            Span::raw("")
        },
    ])]);
    frame.render_widget(header, chunks[0]);

    let body_lines = if let Some(err) = err {
        vec![
            Line::from(vec![
                Span::styled("Error: ", Style::default().fg(Color::Red)),
                Span::raw(err),
            ]),
            Line::from("Try switching to all units with `u` or refreshing with `r`."),
        ]
    } else if lines.is_empty() {
        vec![
            Line::from("No log lines matched the current filters."),
            Line::from("Try `Tab` for severity or `u` for all units."),
        ]
    } else {
        lines.into_iter().map(Line::from).collect::<Vec<Line>>()
    };

    let scroll = app
        .logs_scroll
        .min((body_lines.len().saturating_sub(chunks[1].height as usize)) as u16);
    app.logs_scroll = scroll;
    frame.render_widget(
        Paragraph::new(body_lines)
            .block(Block::default().borders(Borders::ALL).title("Recent lines"))
            .scroll((scroll, 0))
            .wrap(Wrap { trim: false }),
        chunks[1],
    );
}

fn start_disk_scan(app: &mut AppState) {
    let target = disk_target_path(app.disk_target);

    // If already scanning, ignore.
    let scan_path = {
        let state = app.disk_scan.inner.lock().unwrap();
        state.current_path.clone().unwrap_or_else(|| target.clone())
    };

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
        state.current_path = Some(scan_path.clone());
        state.last_started_at = Some(std::time::SystemTime::now());
        state.last_finished_at = None;
    }

    let inner = app.disk_scan.inner.clone();
    app.disk_scroll = 0;

    std::thread::spawn(move || {
        let res = scan_path_entries(&scan_path, &inner);
        let mut state = inner.lock().unwrap();
        state.running = false;
        state.last_finished_at = Some(std::time::SystemTime::now());
        if let Err(e) = res {
            state.error = Some(e);
        }
    });
}

fn enter_selected_disk_dir(app: &mut AppState) {
    let next_path = {
        let state = app.disk_scan.inner.lock().unwrap();
        let idx = app.disk_scroll as usize;
        state.results.get(idx).and_then(|entry| {
            if entry.kind == DiskEntryKind::Directory {
                Some(entry.path.clone())
            } else {
                None
            }
        })
    };

    if let Some(path) = next_path {
        let mut state = app.disk_scan.inner.lock().unwrap();
        state.current_path = Some(path);
        state.results.clear();
        state.error = None;
        app.disk_scroll = 0;
        drop(state);
        start_disk_scan(app);
    }
}

fn navigate_disk_up(app: &mut AppState) {
    let target = disk_target_path(app.disk_target);
    let parent = {
        let state = app.disk_scan.inner.lock().unwrap();
        let current = state.current_path.clone().unwrap_or_else(|| target.clone());
        if current == target {
            None
        } else {
            current.parent().map(Path::to_path_buf)
        }
    };

    if let Some(path) = parent {
        let mut state = app.disk_scan.inner.lock().unwrap();
        state.current_path = Some(path);
        state.results.clear();
        state.error = None;
        app.disk_scroll = 0;
        drop(state);
        start_disk_scan(app);
    }
}

fn scan_path_entries(target: &Path, inner: &Arc<Mutex<DiskScanState>>) -> Result<(), String> {
    let base = target.to_path_buf();
    if !base.exists() {
        return Err(format!("Target does not exist: {}", base.display()));
    }
    if !base.is_dir() {
        return Err(format!("Target is not a directory: {}", base.display()));
    }

    let mut children: Vec<PathBuf> = vec![];
    if let Ok(rd) = std::fs::read_dir(&base) {
        for e in rd.flatten() {
            let p = e.path();
            if p.exists() {
                children.push(p);
            }
        }
    }

    if children.is_empty() {
        return Err("No child files or directories found to scan".to_string());
    }

    let mut results: Vec<DiskEntry> = Vec::new();
    let mut total_seen: u64 = 0;

    for (idx, child) in children.iter().enumerate() {
        {
            let mut st = inner.lock().unwrap();
            st.progress = format!("{}/{}: {}", idx + 1, children.len(), child.display());
        }

        let (kind, size, seen) = scan_entry_size(child, total_seen);
        total_seen = total_seen.saturating_add(seen);

        results.push(DiskEntry {
            path: child.clone(),
            bytes: size,
            kind,
        });

        // Keep top 40 as we go.
        results.sort_by_key(|entry| Reverse(entry.bytes));
        results.truncate(40);

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

fn scan_entry_size(path: &Path, total_seen: u64) -> (DiskEntryKind, u64, u64) {
    if path.is_file() {
        let bytes = fs::metadata(path).map(|md| md.len()).unwrap_or(0);
        return (DiskEntryKind::File, bytes, 1);
    }

    if is_file_like_package(path) {
        let (size, seen) = scan_dir_size(path, total_seen);
        return (DiskEntryKind::File, size, seen);
    }

    let (size, seen) = scan_dir_size(path, total_seen);
    (DiskEntryKind::Directory, size, seen)
}

fn scan_dir_size(path: &Path, total_seen: u64) -> (u64, u64) {
    let mut size: u64 = 0;
    let mut seen: u64 = 0;

    // Walk with a depth limit to stay responsive.
    for entry in WalkDir::new(path)
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
            if seen >= 50_000 || total_seen.saturating_add(seen) >= 300_000 {
                break;
            }
        }
    }

    (size, seen)
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

fn format_snapshot(vm: &VmSnapshot, app: &AppState, system: &System, disks: &Disks) -> String {
    let mut out: Vec<String> = Vec::new();
    out.push(format!("ferro {} snapshot", VERSION));
    out.push("".to_string());

    out.push(format!("CPU: {:.1}% cores={}", vm.cpu_usage, vm.cpu_cores));
    out.push(format!(
        "MEM: {} / {} ({:.1}%)",
        format_bytes(vm.used_memory),
        format_bytes(vm.total_memory),
        vm.memory_percent
    ));
    out.push("".to_string());

    out.push("Top CPU:".to_string());
    for row in format_top_processes(system, ProcSort::Cpu, 5) {
        out.push(format!("  {row}"));
    }
    out.push("Top MEM:".to_string());
    for row in format_top_processes(system, ProcSort::Mem, 5) {
        out.push(format!("  {row}"));
    }
    out.push("".to_string());

    let disk_rows = if app.dash_mount_rows.is_empty() {
        collect_mount_rows(12, app.dash_show_all_mounts)
            .unwrap_or_else(|| disks_table_filtered(disks, 12, app.dash_show_all_mounts))
    } else {
        app.dash_mount_rows.clone()
    };
    out.push(if app.dash_show_all_mounts {
        "Filesystems (all):".to_string()
    } else {
        "Filesystems (filtered):".to_string()
    });
    out.push("FS	Size	Used	Avail	Use%	Mount".to_string());
    for r in disk_rows {
        out.push(format!(
            "{}	{}	{}	{}	{:.0}%	{}",
            r.fs,
            format_bytes(r.size),
            format_bytes(r.used),
            format_bytes(r.avail),
            r.use_pct,
            r.mount
        ));
    }

    out.join("\n")
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

fn collect_mount_rows(limit: usize, show_all: bool) -> Option<Vec<DiskRow>> {
    let output = Command::new("df").args(["-kP"]).output().ok()?;
    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut seen_mounts: HashSet<String> = HashSet::new();
    let mut rows: Vec<DiskRow> = stdout
        .lines()
        .skip(1)
        .filter_map(parse_df_row)
        .filter(|row| seen_mounts.insert(row.mount.clone()))
        .filter(|row| show_all || !should_hide_mount_row(row))
        .collect();

    if !show_all {
        rows.sort_by_key(|row| {
            (
                Reverse((row.use_pct * 10.0) as i64),
                Reverse(row.size),
                row.mount.clone(),
            )
        });
    }
    rows.truncate(limit);
    Some(rows)
}

fn parse_df_row(line: &str) -> Option<DiskRow> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 6 {
        return None;
    }

    let numeric_idx = parts.len().saturating_sub(5);
    let size = parts[numeric_idx].parse::<u64>().ok()?.saturating_mul(1024);
    let used = parts[numeric_idx + 1]
        .parse::<u64>()
        .ok()?
        .saturating_mul(1024);
    let avail = parts[numeric_idx + 2]
        .parse::<u64>()
        .ok()?
        .saturating_mul(1024);
    let use_pct = parts[numeric_idx + 3]
        .trim_end_matches('%')
        .parse::<f64>()
        .ok()
        .unwrap_or_else(|| percent(used, size));

    Some(DiskRow {
        fs: parts[..numeric_idx].join(" "),
        size,
        used,
        avail,
        use_pct,
        mount: parts[numeric_idx + 4..].join(" "),
    })
}

fn should_hide_mount_row(row: &DiskRow) -> bool {
    let fs_l = row.fs.to_lowercase();
    if fs_l.contains("tmpfs")
        || fs_l.contains("udev")
        || fs_l.contains("devtmpfs")
        || fs_l == "devfs"
        || fs_l.starts_with("map ")
    {
        return true;
    }

    if row.mount.starts_with("/run")
        || row.mount.starts_with("/dev")
        || row.mount.starts_with("/sys")
    {
        return true;
    }

    #[cfg(target_os = "macos")]
    {
        if row.mount.starts_with("/System/Volumes/")
            && row.mount != "/System/Volumes/Data"
            && row.mount != "/System/Volumes/VM"
        {
            return true;
        }
    }

    row.size == 0
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

        let row = DiskRow {
            fs,
            size: total,
            used,
            avail,
            use_pct: pct,
            mount,
        };

        if !show_all && should_hide_mount_row(&row) {
            continue;
        }

        rows.push(row);
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

fn format_memory_pressure(system: &System, top_n: usize) -> Vec<String> {
    let total_memory = system.total_memory();
    let available_memory = system.available_memory();
    let total_swap = system.total_swap();
    let used_swap = system.used_swap();
    let available_pct = percent(available_memory, total_memory);
    let swap_pct = percent(used_swap, total_swap);

    let mut procs: Vec<ProcRow> = system
        .processes()
        .iter()
        .map(|(pid, p)| ProcRow::from_process(*pid, p))
        .collect();
    procs.sort_by_key(|p| Reverse((p.mem_bytes as i64, p.cpu_x10 as i64)));
    let top_total: u64 = procs.into_iter().take(top_n).map(|p| p.mem_bytes).sum();

    vec![
        format!("Top {top_n} using {}", format_bytes(top_total)),
        format!("Avail {:.0}% of RAM", available_pct),
        if total_swap > 0 {
            format!("Swap {:.0}% in use", swap_pct)
        } else {
            "Swap off".to_string()
        },
    ]
}

fn push_history_sample(history: &mut VecDeque<u16>, value: f64, max_len: usize) {
    history.push_back(value.round().clamp(0.0, 100.0) as u16);
    while history.len() > max_len {
        history.pop_front();
    }
}

fn history_peak(history: &VecDeque<u16>) -> u16 {
    history.iter().copied().max().unwrap_or(0)
}

fn history_average(history: &VecDeque<u16>) -> f64 {
    if history.is_empty() {
        0.0
    } else {
        history.iter().map(|sample| *sample as u64).sum::<u64>() as f64 / history.len() as f64
    }
}

fn render_detail_panel(
    frame: &mut ratatui::Frame,
    area: Rect,
    title: &str,
    lines: Vec<String>,
    history: &VecDeque<u16>,
    color: Color,
) {
    let block = Block::default().borders(Borders::ALL).title(title);
    frame.render_widget(block.clone(), area);
    let inner = block.inner(area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(lines.len() as u16), Constraint::Min(1)])
        .split(inner);

    frame.render_widget(
        Paragraph::new(lines.into_iter().map(Line::from).collect::<Vec<_>>())
            .style(Style::default().fg(Color::Gray))
            .alignment(Alignment::Left),
        chunks[0],
    );

    let data: Vec<u64> = history.iter().map(|sample| *sample as u64).collect();
    if !data.is_empty() && chunks[1].width > 0 && chunks[1].height > 0 {
        let spark_color = history
            .back()
            .map(|sample| color_for_pct(*sample as f64))
            .unwrap_or(color);
        let spark_width = chunks[1].width.min(data.len() as u16);
        let spark_x = chunks[1].x + (chunks[1].width.saturating_sub(spark_width)) / 2;
        let spark_area = Rect {
            x: spark_x,
            y: chunks[1].y,
            width: spark_width,
            height: chunks[1].height,
        };
        frame.render_widget(
            Sparkline::default()
                .data(&data)
                .max(100)
                .style(Style::default().fg(spark_color)),
            spark_area,
        );
    }
}

fn dash_target_path(target: DashDirTarget) -> (String, PathBuf) {
    match target {
        DashDirTarget::Cwd => {
            let p = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
            let label = p.to_string_lossy().to_string();
            (label, p)
        }
        DashDirTarget::Var => ("/var".to_string(), PathBuf::from("/var")),
        DashDirTarget::Home => {
            let p = std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("/"));
            let label = p.to_string_lossy().to_string();
            (label, p)
        }
        DashDirTarget::Root => ("/".to_string(), PathBuf::from("/")),
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

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    Rect {
        x,
        y,
        width: width.min(area.width),
        height: height.min(area.height),
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_service_rows_extracts_service_units() {
        let stdout = "\
Id=sshd.service
Description=OpenSSH server daemon
LoadState=loaded
ActiveState=active
SubState=running
NRestarts=2
ActiveEnterTimestamp=Thu 2026-03-12 10:00:00 UTC
InactiveEnterTimestamp=n/a

Id=apt-daily.service
Description=Daily apt download activities
LoadState=loaded
ActiveState=inactive
SubState=dead
NRestarts=0
ActiveEnterTimestamp=Thu 2026-03-12 08:00:00 UTC
InactiveEnterTimestamp=Thu 2026-03-12 09:00:00 UTC

Id=session-1.scope
Description=User Session
LoadState=loaded
ActiveState=active
SubState=running
NRestarts=0
ActiveEnterTimestamp=Thu 2026-03-12 07:00:00 UTC
InactiveEnterTimestamp=n/a
";

        let rows = parse_service_rows(stdout);

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].name, "sshd.service");
        assert_eq!(rows[0].last_change, "Thu 2026-03-12 10:00:00…");
        assert_eq!(rows[1].name, "apt-daily.service");
        assert_eq!(rows[1].last_change, "Thu 2026-03-12 09:00:00…");
    }

    #[test]
    fn filtered_service_rows_respects_search_query() {
        let rows = vec![
            ServiceRow {
                name: "sshd.service".to_string(),
                description: "OpenSSH server daemon".to_string(),
                load_state: "loaded".to_string(),
                active_state: "active".to_string(),
                sub_state: "running".to_string(),
                restarts: 0,
                last_change: "-".to_string(),
                health: ServiceHealth::Healthy,
            },
            ServiceRow {
                name: "nginx.service".to_string(),
                description: "High performance web server".to_string(),
                load_state: "loaded".to_string(),
                active_state: "active".to_string(),
                sub_state: "running".to_string(),
                restarts: 0,
                last_change: "-".to_string(),
                health: ServiceHealth::Healthy,
            },
        ];

        let rows = filtered_service_rows(&rows, ServiceFilter::All, "ssh");

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "sshd.service");
    }

    #[test]
    fn dashboard_dir_target_cycles_all_targets() {
        let mut target = DashDirTarget::Cwd;

        target = target.next();
        assert_eq!(target, DashDirTarget::Var);

        target = target.next();
        assert_eq!(target, DashDirTarget::Home);

        target = target.next();
        assert_eq!(target, DashDirTarget::Root);

        target = target.next();
        assert_eq!(target, DashDirTarget::Cwd);
    }
}
