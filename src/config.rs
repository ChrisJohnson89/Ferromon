use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::cli::{Args, MAX_TICK_MS, MIN_TICK_MS};

const DEFAULT_TICK_MS: u64 = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefaultScreen {
    Dashboard,
    Processes,
    DiskDive,
    Services,
    Logs,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct FileConfig {
    pub tick_ms: Option<u64>,
    pub no_mouse: Option<bool>,
    pub default_screen: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeConfig {
    pub tick_ms: u64,
    pub no_mouse: bool,
    pub default_screen: DefaultScreen,
}

pub fn load_file_config() -> FileConfig {
    match config_path() {
        Some(path) => load_file_config_from_path(&path),
        None => FileConfig::default(),
    }
}

pub fn load_file_config_from_path(path: &Path) -> FileConfig {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return FileConfig::default(),
        Err(err) => {
            eprintln!(
                "Warning: failed to read config at {}: {err}",
                path.display()
            );
            return FileConfig::default();
        }
    };

    match toml::from_str::<FileConfig>(&raw) {
        Ok(config) => config,
        Err(err) => {
            eprintln!(
                "Warning: failed to parse config at {}: {err}",
                path.display()
            );
            FileConfig::default()
        }
    }
}

pub fn resolve_config(file: &FileConfig, cli: &Args) -> RuntimeConfig {
    let mut tick_ms = DEFAULT_TICK_MS;
    if let Some(raw) = file.tick_ms {
        tick_ms = clamp_tick_ms(raw, "config tick_ms");
    }
    if let Some(raw) = cli.tick_ms_override {
        tick_ms = clamp_tick_ms(raw, "--tick-ms");
    }

    let mut no_mouse = file.no_mouse.unwrap_or(false);
    if cli.no_mouse {
        no_mouse = true;
    }

    let mut default_screen = DefaultScreen::Dashboard;
    if let Some(raw) = file.default_screen.as_deref() {
        if let Some(screen) = parse_default_screen(raw) {
            default_screen = screen;
        } else {
            eprintln!(
                "Warning: config default_screen '{}' is invalid; using dashboard",
                raw
            );
        }
    }

    RuntimeConfig {
        tick_ms,
        no_mouse,
        default_screen,
    }
}

fn config_path() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| {
        PathBuf::from(home)
            .join(".config")
            .join("ferro")
            .join("config.toml")
    })
}

fn clamp_tick_ms(raw: u64, source: &str) -> u64 {
    let clamped = raw.clamp(MIN_TICK_MS, MAX_TICK_MS);
    if raw != clamped {
        eprintln!(
            "Warning: {} {} is out of range, clamped to {}",
            source, raw, clamped
        );
    }
    clamped
}

fn parse_default_screen(raw: &str) -> Option<DefaultScreen> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "dashboard" | "dash" => Some(DefaultScreen::Dashboard),
        "processes" | "process" | "proc" => Some(DefaultScreen::Processes),
        "disk_dive" | "disk-dive" | "diskdive" | "disk" => Some(DefaultScreen::DiskDive),
        "services" | "service" | "svc" => Some(DefaultScreen::Services),
        "logs" | "log" => Some(DefaultScreen::Logs),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_screen_aliases() {
        assert_eq!(
            parse_default_screen("dashboard"),
            Some(DefaultScreen::Dashboard)
        );
        assert_eq!(parse_default_screen("PROC"), Some(DefaultScreen::Processes));
        assert_eq!(
            parse_default_screen("disk_dive"),
            Some(DefaultScreen::DiskDive)
        );
        assert_eq!(
            parse_default_screen("service"),
            Some(DefaultScreen::Services)
        );
        assert_eq!(parse_default_screen("logs"), Some(DefaultScreen::Logs));
        assert_eq!(parse_default_screen("unknown"), None);
    }

    #[test]
    fn resolve_defaults_when_missing() {
        let cli = Args::default();
        let cfg = resolve_config(&FileConfig::default(), &cli);

        assert_eq!(cfg.tick_ms, DEFAULT_TICK_MS);
        assert!(!cfg.no_mouse);
        assert_eq!(cfg.default_screen, DefaultScreen::Dashboard);
    }

    #[test]
    fn resolve_cli_overrides_file() {
        let file = FileConfig {
            tick_ms: Some(800),
            no_mouse: Some(false),
            default_screen: Some("logs".to_string()),
        };
        let cli = Args {
            tick_ms_override: Some(1200),
            no_mouse: true,
            ..Args::default()
        };

        let cfg = resolve_config(&file, &cli);
        assert_eq!(cfg.tick_ms, 1200);
        assert!(cfg.no_mouse);
        assert_eq!(cfg.default_screen, DefaultScreen::Logs);
    }

    #[test]
    fn resolve_clamps_tick_values() {
        let file_low = FileConfig {
            tick_ms: Some(1),
            ..FileConfig::default()
        };
        let cfg_low = resolve_config(&file_low, &Args::default());
        assert_eq!(cfg_low.tick_ms, MIN_TICK_MS);

        let cli_high = Args {
            tick_ms_override: Some(9_999),
            ..Args::default()
        };
        let cfg_high = resolve_config(&FileConfig::default(), &cli_high);
        assert_eq!(cfg_high.tick_ms, MAX_TICK_MS);
    }
}
