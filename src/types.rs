use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

// proc_pid_rusage gives per-process phys_footprint (compressed included)
// without requiring task_for_pid or any special entitlements.
#[cfg(target_os = "macos")]
#[link(name = "proc")]
extern "C" {
    fn proc_pid_rusage(pid: i32, flavor: i32, buffer: *mut u8) -> i32;
}

use sysinfo::{Process, System};

// ── Screen navigation ────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    #[default]
    Dashboard,
    Processes,
    DiskDive,
    Services,
    Logs,
}

// ── Process sorting ──────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ProcSort {
    #[default]
    Cpu,
    Mem,
    Swap,
}

// ── Disk dive targets ────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum DiskTarget {
    #[default]
    Var,
    Home,
    Root,
}

// ── Dashboard dir targets ────────────────────────────────────────────────────

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum DashDirTarget {
    #[default]
    Cwd,
    Var,
    Home,
    Root,
}

impl DashDirTarget {
    pub fn next(self) -> Self {
        match self {
            DashDirTarget::Cwd => DashDirTarget::Var,
            DashDirTarget::Var => DashDirTarget::Home,
            DashDirTarget::Home => DashDirTarget::Root,
            DashDirTarget::Root => DashDirTarget::Cwd,
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            DashDirTarget::Cwd => "CWD",
            DashDirTarget::Var => "/var",
            DashDirTarget::Home => "HOME",
            DashDirTarget::Root => "/",
        }
    }
}

// ── Service / log filter enums ───────────────────────────────────────────────

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ServiceFilter {
    Failed,
    Unhealthy,
    Active,
    #[default]
    All,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum LogSeverity {
    Errors,
    Warnings,
    #[default]
    Info,
    Debug,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum LogUnitFilter {
    #[default]
    Selected,
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceHealth {
    Healthy,
    Warning,
    Critical,
}

// ── Disk entry types ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiskEntryKind {
    Directory,
    File,
}

#[derive(Debug, Clone)]
pub struct DiskEntry {
    pub path: PathBuf,
    pub bytes: u64,
    pub kind: DiskEntryKind,
}

// ── Thread-safe state wrappers ───────────────────────────────────────────────

#[derive(Clone, Default)]
pub struct DiskScan {
    pub inner: Arc<Mutex<DiskScanState>>,
}

#[derive(Clone, Default)]
pub struct ServiceState {
    pub inner: Arc<Mutex<ServiceStateInner>>,
}

#[derive(Clone, Default)]
pub struct LogState {
    pub inner: Arc<Mutex<LogStateInner>>,
}

#[derive(Default)]
pub struct DiskScanState {
    pub running: bool,
    pub last_target: Option<PathBuf>,
    pub current_path: Option<PathBuf>,
    pub last_started_at: Option<std::time::SystemTime>,
    pub last_finished_at: Option<std::time::SystemTime>,
    pub progress: String,
    pub results: Vec<DiskEntry>,
    pub error: Option<String>,
}

#[derive(Default)]
pub struct ServiceStateInner {
    pub running: bool,
    pub unsupported: Option<String>,
    pub error: Option<String>,
    pub rows: Vec<ServiceRow>,
    pub last_updated_at: Option<std::time::SystemTime>,
}

#[derive(Default)]
pub struct LogStateInner {
    pub running: bool,
    pub unsupported: Option<String>,
    pub error: Option<String>,
    pub lines: Vec<String>,
    pub last_updated_at: Option<std::time::SystemTime>,
    pub source: String,
}

// ── Service row ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ServiceRow {
    pub name: String,
    pub description: String,
    pub load_state: String,
    pub active_state: String,
    pub sub_state: String,
    pub restarts: u32,
    pub last_change: String,
    pub health: ServiceHealth,
}

// ── Disk row (mount info) ────────────────────────────────────────────────────

#[derive(Clone)]
pub struct DiskRow {
    pub fs: String,
    pub size: u64,
    pub used: u64,
    pub avail: u64,
    pub use_pct: f64,
    pub mount: String,
    pub read_bps: u64,
    pub write_bps: u64,
}

// ── VM snapshot ──────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct VmSnapshot {
    pub cpu_usage: f32,
    pub cpu_cores: usize,
    pub load_avg_one: f64,
    pub load_avg_five: f64,
    pub load_avg_fifteen: f64,
    pub total_memory: u64,
    pub used_memory: u64,
    pub available_memory: u64,
    pub memory_percent: f64,
    pub total_swap: u64,
    pub used_swap: u64,
    pub uptime_secs: u64,
}

// ── Process row ──────────────────────────────────────────────────────────────

fn proc_status_label(s: sysinfo::ProcessStatus) -> &'static str {
    match s {
        sysinfo::ProcessStatus::Run => "Run",
        sysinfo::ProcessStatus::Sleep => "Sleep",
        sysinfo::ProcessStatus::Idle => "Idle",
        sysinfo::ProcessStatus::Stop => "Stop",
        sysinfo::ProcessStatus::Zombie => "Zombie",
        sysinfo::ProcessStatus::Dead => "Dead",
        sysinfo::ProcessStatus::Tracing => "Trace",
        _ => "?",
    }
}

#[derive(Debug, Clone)]
pub struct ProcRow {
    pub pid: i32,
    pub name: String,
    pub cpu_x10: i32,
    pub mem_bytes: u64,
    pub swap_bytes: u64,
    pub status: &'static str,
}

fn read_proc_swap_bytes(pid: u32) -> u64 {
    #[cfg(target_os = "linux")]
    {
        use std::fs;
        if let Ok(content) = fs::read_to_string(format!("/proc/{}/status", pid)) {
            for line in content.lines() {
                if let Some(rest) = line.strip_prefix("VmSwap:") {
                    let kb: u64 = rest
                        .split_whitespace()
                        .next()
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(0);
                    return kb * 1024;
                }
            }
        }
        0
    }
    #[cfg(target_os = "macos")]
    {
        // proc_pid_rusage works without task_for_pid / special entitlements.
        // ri_phys_footprint (physical footprint incl. compressed pages) minus
        // ri_resident_size (pages physically in RAM) gives the compressed delta —
        // the macOS equivalent of per-process swap.
        use std::mem;

        // rusage_info_v2 layout up through ri_phys_footprint (flavor = 2).
        #[repr(C)]
        struct RusageInfoV2 {
            ri_uuid: [u8; 16],
            ri_user_time: u64,
            ri_system_time: u64,
            ri_pkg_idle_wkups: u64,
            ri_interrupt_wkups: u64,
            ri_pageins: u64,
            ri_wired_size: u64,
            ri_resident_size: u64,
            ri_phys_footprint: u64,
        }

        unsafe {
            let mut info: RusageInfoV2 = mem::zeroed();
            let ret = proc_pid_rusage(
                pid as i32,
                2, // RUSAGE_INFO_V2
                &mut info as *mut RusageInfoV2 as *mut u8,
            );
            if ret == 0 {
                info.ri_phys_footprint.saturating_sub(info.ri_resident_size)
            } else {
                0
            }
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = pid;
        0
    }
}

impl ProcRow {
    pub fn from_process(pid: sysinfo::Pid, p: &Process) -> Self {
        // sysinfo CPU is percent float; store x10 to sort stably without floats
        let cpu_x10 = (p.cpu_usage() * 10.0) as i32;
        let mem_bytes = p.memory();
        ProcRow {
            pid: pid.as_u32() as i32,
            name: p.name().to_string(),
            cpu_x10,
            mem_bytes,
            swap_bytes: read_proc_swap_bytes(pid.as_u32()),
            status: proc_status_label(p.status()),
        }
    }
}

// ── Update state ─────────────────────────────────────────────────────────────

#[derive(Clone, Default)]
pub struct UpdateState {
    pub last_checked_at: Option<std::time::SystemTime>,
    pub latest_tag: Option<String>,
    pub available: bool,
    pub error: Option<String>,
}

// ── CLI args ─────────────────────────────────────────────────────────────────

#[derive(Default)]
pub struct Args {
    pub tick_ms: u64,
    pub no_mouse: bool,
    pub show_help: bool,
    pub show_version: bool,
}

// ── Application state ────────────────────────────────────────────────────────

pub struct AppState {
    pub screen: Screen,
    pub show_help: bool,

    pub proc_sort: ProcSort,
    pub proc_scroll: u16,
    pub proc_search: String,
    pub proc_search_active: bool,
    pub proc_kill_confirm: Option<(i32, String)>,
    pub proc_restart_confirm: Option<(i32, String, std::path::PathBuf, Vec<String>)>,

    pub disk_target: DiskTarget,
    pub disk_scroll: u16,
    pub disk_scan: DiskScan,
    pub service_scroll: u16,
    pub service_filter: ServiceFilter,
    pub service_search: String,
    pub service_search_active: bool,
    pub service_state: ServiceState,
    pub service_last_refresh_at: Option<Instant>,
    pub logs_scroll: u16,
    pub log_severity: LogSeverity,
    pub log_unit_filter: LogUnitFilter,
    pub log_state: LogState,
    pub log_last_refresh_at: Option<Instant>,
    pub log_selected_unit: Option<String>,

    // Dashboard caches (quick overview)
    pub dash_dir_target: DashDirTarget,
    pub dash_dir_sizes: Vec<String>,
    pub dash_mount_rows: Vec<DiskRow>,
    pub dash_top_cpu: Vec<String>,
    pub dash_top_mem: Vec<String>,
    pub dash_mem_pressure: Vec<String>,
    pub dash_cpu_history: VecDeque<u16>,
    pub dash_mem_history: VecDeque<u16>,
    pub dash_last_proc_at: Option<Instant>,
    pub dash_last_fs_at: Option<Instant>,
    pub dash_last_history_at: Option<Instant>,
    pub dash_last_io_at: Option<Instant>,
    pub dash_diskstats_prev: HashMap<String, (u64, u64)>,
    pub dash_show_all_mounts: bool,
    pub hostname: String,
    pub footer_tip_idx: u8,
    pub tick_ms: u64,
    pub dump_snapshot: bool,

    pub update: UpdateState,
    pub do_update: bool,
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
            proc_restart_confirm: None,
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
            dash_last_io_at: None,
            dash_diskstats_prev: HashMap::new(),
            dash_show_all_mounts: true,
            hostname: System::host_name().unwrap_or_else(|| "unknown".to_string()),
            footer_tip_idx: 0,
            tick_ms: 500,
            dump_snapshot: false,
            update: UpdateState::default(),
            do_update: false,
        }
    }
}

// Test for DashDirTarget cycling
#[cfg(test)]
mod tests {
    use super::*;

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
