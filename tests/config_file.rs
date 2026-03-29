#![allow(dead_code)]

#[path = "../src/cli.rs"]
mod cli;
#[path = "../src/config.rs"]
mod config;

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_config_path() -> PathBuf {
    let mut path = std::env::temp_dir();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time moved backwards")
        .as_nanos();
    path.push(format!("ferromon-config-test-{nanos}.toml"));
    path
}

#[test]
fn load_file_config_reads_supported_fields() {
    let path = temp_config_path();
    fs::write(
        &path,
        "tick_ms = 750\nno_mouse = true\ndefault_screen = \"services\"\n",
    )
    .expect("write config file");

    let parsed = config::load_file_config_from_path(&path);
    fs::remove_file(&path).expect("remove temp config file");

    assert_eq!(parsed.tick_ms, Some(750));
    assert_eq!(parsed.no_mouse, Some(true));
    assert_eq!(parsed.default_screen.as_deref(), Some("services"));
}

#[test]
fn cli_tick_ms_wins_over_file_value() {
    let file_cfg = config::FileConfig {
        tick_ms: Some(500),
        no_mouse: Some(false),
        default_screen: Some("dashboard".to_string()),
    };
    let cli_args = cli::Args {
        tick_ms_override: Some(1200),
        no_mouse: false,
        show_help: false,
        show_version: false,
    };

    let resolved = config::resolve_config(&file_cfg, &cli_args);
    assert_eq!(resolved.tick_ms, 1200);
}
