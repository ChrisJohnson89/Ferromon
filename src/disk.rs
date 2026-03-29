use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use walkdir::WalkDir;

use crate::types::{AppState, DiskEntry, DiskEntryKind, DiskScanState, DiskTarget};

// ── Path lookup ───────────────────────────────────────────────────────────────

pub fn disk_target_path(target: DiskTarget) -> PathBuf {
    match target {
        DiskTarget::Var => PathBuf::from("/var"),
        DiskTarget::Home => std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| "/".into()),
        DiskTarget::Root => PathBuf::from("/"),
    }
}

// ── Package-like dir detection ────────────────────────────────────────────────

pub fn is_file_like_package(path: &Path) -> bool {
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

// ── Scan operations ───────────────────────────────────────────────────────────

pub fn start_disk_scan(app: &mut AppState) {
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

pub fn enter_selected_disk_dir(app: &mut AppState) {
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

pub fn navigate_disk_up(app: &mut AppState) {
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
    use std::cmp::Reverse;

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
        let bytes = std::fs::metadata(path).map(|md| md.len()).unwrap_or(0);
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
