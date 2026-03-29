use std::cmp::Reverse;
use std::path::Path;
use std::process::Command;
use std::time::Duration;
use std::time::Instant;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::types::{
    AppState, LogSeverity, LogUnitFilter, ProcRow, Screen, ServiceFilter, ServiceHealth, ServiceRow,
};
use crate::utils::trim_to;

// ── Platform support check ────────────────────────────────────────────────────

fn services_supported_message() -> Option<String> {
    if cfg!(target_os = "linux") {
        None
    } else {
        Some("Service health and journal logs are currently Linux-only.".to_string())
    }
}

// ── Service refresh ───────────────────────────────────────────────────────────

pub fn refresh_services(app: &mut AppState, force: bool) {
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

// ── Log refresh ───────────────────────────────────────────────────────────────

pub fn refresh_logs(app: &mut AppState, force: bool) {
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

// ── Service collection ────────────────────────────────────────────────────────

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

pub fn parse_service_rows(stdout: &str) -> Vec<ServiceRow> {
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

// ── Log collection ────────────────────────────────────────────────────────────

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

// ── Service health helpers ────────────────────────────────────────────────────

pub fn service_health(
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

pub fn service_health_rank(health: ServiceHealth) -> u8 {
    match health {
        ServiceHealth::Critical => 0,
        ServiceHealth::Warning => 1,
        ServiceHealth::Healthy => 2,
    }
}

pub fn simplify_timestamp(ts: &str) -> String {
    if ts.trim().is_empty() || ts.trim() == "n/a" {
        "-".to_string()
    } else {
        trim_to(ts.trim(), 24)
    }
}

// ── Label helpers ─────────────────────────────────────────────────────────────

pub fn service_filter_label(filter: ServiceFilter) -> &'static str {
    match filter {
        ServiceFilter::Failed => "failed",
        ServiceFilter::Unhealthy => "unhealthy",
        ServiceFilter::Active => "active",
        ServiceFilter::All => "all",
    }
}

pub fn log_severity_label(sev: LogSeverity) -> &'static str {
    match sev {
        LogSeverity::Errors => "err+",
        LogSeverity::Warnings => "warning+",
        LogSeverity::Info => "info+",
        LogSeverity::Debug => "debug+",
    }
}

pub fn log_unit_filter_label(filter: LogUnitFilter, selected_unit: Option<&str>) -> String {
    match filter {
        LogUnitFilter::Selected => selected_unit.unwrap_or("selected").to_string(),
        LogUnitFilter::All => "all units".to_string(),
    }
}

// ── Filtering ─────────────────────────────────────────────────────────────────

pub fn filtered_proc_rows(rows: Vec<ProcRow>, search: &str) -> Vec<ProcRow> {
    if search.is_empty() {
        return rows;
    }
    let needle = search.trim().to_ascii_lowercase();
    let mut matched: Vec<ProcRow> = rows
        .into_iter()
        .filter(|p| p.name.to_ascii_lowercase().contains(&needle))
        .collect();
    matched.sort_by(|a, b| {
        a.name
            .to_ascii_lowercase()
            .cmp(&b.name.to_ascii_lowercase())
    });
    matched
}

pub fn filtered_service_rows(
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

pub fn selected_service(app: &AppState) -> Option<ServiceRow> {
    let state = app.service_state.inner.lock().unwrap();
    let rows = filtered_service_rows(&state.rows, app.service_filter, &app.service_search);
    rows.get(app.service_scroll as usize).cloned()
}

pub fn open_logs_for_selected_service(app: &mut AppState) {
    if let Some(row) = selected_service(app) {
        app.log_selected_unit = Some(row.name);
        app.logs_scroll = 0;
        app.screen = Screen::Logs;
        refresh_logs(app, true);
    }
}

// ── Key input handlers ────────────────────────────────────────────────────────

pub fn is_text_input_key(key: &KeyEvent) -> bool {
    key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT
}

pub fn handle_service_search_key(app: &mut AppState, key: &KeyEvent) -> bool {
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

pub fn handle_proc_search_key(app: &mut AppState, key: &KeyEvent) -> bool {
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

// ── Tests ─────────────────────────────────────────────────────────────────────

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
}
