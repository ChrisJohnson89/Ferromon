use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use sysinfo::{Disks, System};

use crate::types::{AppState, DashDirTarget, DiskRow, ProcRow, ProcSort, VmSnapshot};
use crate::update::VERSION;
use crate::utils::{format_bytes, percent, trim_to};

// ── Core sysinfo refresh ──────────────────────────────────────────────────────

pub fn refresh(system: &mut System, disks: &mut Disks, refresh_processes: bool) {
    system.refresh_cpu();
    system.refresh_memory();
    if refresh_processes {
        system.refresh_processes();
    }
    disks.refresh();
}

// ── VM snapshot ───────────────────────────────────────────────────────────────

pub fn snapshot(system: &System) -> VmSnapshot {
    let cpu_usage = system.global_cpu_info().cpu_usage();
    let cpu_cores = system.cpus().len();
    let load_avg = System::load_average();
    let load_avg_one = load_avg.one;
    let load_avg_five = load_avg.five;
    let load_avg_fifteen = load_avg.fifteen;
    // sysinfo reports memory in bytes
    let total_memory = system.total_memory();
    let used_memory = system.used_memory();
    // On macOS available_memory() can return 0; free_memory() is the reliable fallback.
    let available_memory = system.available_memory().max(system.free_memory());
    let memory_percent = percent(used_memory, total_memory);
    let total_swap = system.total_swap();
    let used_swap = system.used_swap();

    VmSnapshot {
        cpu_usage,
        cpu_cores,
        load_avg_one,
        load_avg_five,
        load_avg_fifteen,
        total_memory,
        used_memory,
        available_memory,
        memory_percent,
        total_swap,
        used_swap,
        uptime_secs: System::uptime(),
    }
}

// ── Process helpers ───────────────────────────────────────────────────────────

pub fn format_top_processes(system: &System, sort: ProcSort, count: usize) -> Vec<String> {
    use std::cmp::Reverse;

    let mut procs: Vec<ProcRow> = system
        .processes()
        .iter()
        .map(|(pid, p)| ProcRow::from_process(*pid, p))
        .collect();

    match sort {
        ProcSort::Cpu => procs.sort_by_key(|p| Reverse((p.cpu_x10 as i64, p.mem_bytes as i64))),
        ProcSort::Mem => procs.sort_by_key(|p| Reverse((p.mem_bytes as i64, p.cpu_x10 as i64))),
        ProcSort::Swap => procs.sort_by_key(|p| Reverse((p.mem_bytes as i64, p.cpu_x10 as i64))),
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

pub fn format_memory_pressure(system: &System, top_n: usize) -> Vec<String> {
    use std::cmp::Reverse;

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

// ── Uptime ────────────────────────────────────────────────────────────────────

pub fn format_uptime(secs: u64) -> String {
    let days = secs / 86400;
    let hours = (secs % 86400) / 3600;
    let mins = (secs % 3600) / 60;
    if days > 0 {
        format!("{}d {}h {}m", days, hours, mins)
    } else if hours > 0 {
        format!("{}h {}m", hours, mins)
    } else {
        format!("{}m", mins)
    }
}

// ── Disk / mount rows ─────────────────────────────────────────────────────────

pub fn collect_mount_rows(limit: usize, show_all: bool) -> Option<Vec<DiskRow>> {
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
            use std::cmp::Reverse;
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

pub fn parse_df_row(line: &str) -> Option<DiskRow> {
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
        read_bps: 0,
        write_bps: 0,
        use_pct,
        mount: parts[numeric_idx + 4..].join(" "),
    })
}

pub fn should_hide_mount_row(row: &DiskRow) -> bool {
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

pub fn disks_table_filtered(disks: &Disks, limit: usize, show_all: bool) -> Vec<DiskRow> {
    use std::cmp::Reverse;

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
            read_bps: 0,
            write_bps: 0,
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

// ── Disk I/O rates ────────────────────────────────────────────────────────────

pub fn update_disk_io_rates(
    rows: &mut [DiskRow],
    prev_stats: &mut HashMap<String, (u64, u64)>,
    elapsed_secs: f64,
) {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (rows, prev_stats, elapsed_secs);
    }
    #[cfg(target_os = "linux")]
    {
        if elapsed_secs <= 0.0 {
            return;
        }
        let current = read_diskstats_map();
        for row in rows.iter_mut() {
            let dev = Path::new(&row.fs)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if let (Some(&(pr, pw)), Some(&(cr, cw))) = (prev_stats.get(&dev), current.get(&dev)) {
                row.read_bps = ((cr.saturating_sub(pr) * 512) as f64 / elapsed_secs) as u64;
                row.write_bps = ((cw.saturating_sub(pw) * 512) as f64 / elapsed_secs) as u64;
            }
        }
        *prev_stats = current;
    }
}

#[cfg(target_os = "linux")]
fn read_diskstats_map() -> HashMap<String, (u64, u64)> {
    use std::fs;

    let mut map = HashMap::new();
    if let Ok(content) = fs::read_to_string("/proc/diskstats") {
        for line in content.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 10 {
                let dev = parts[2].to_string();
                let read_sectors = parts[5].parse::<u64>().unwrap_or(0);
                let write_sectors = parts[9].parse::<u64>().unwrap_or(0);
                map.insert(dev, (read_sectors, write_sectors));
            }
        }
    }
    map
}

// ── Dashboard dir scan helpers ────────────────────────────────────────────────

pub fn dash_target_path(target: DashDirTarget) -> (String, PathBuf) {
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

pub fn scan_dir_quick(dir: &Path, limit: usize) -> Vec<String> {
    use std::cmp::Reverse;

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

// ── Snapshot dump ─────────────────────────────────────────────────────────────

pub fn format_snapshot(vm: &VmSnapshot, app: &AppState, system: &System, disks: &Disks) -> String {
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
