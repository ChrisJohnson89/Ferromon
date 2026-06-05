use crate::types::Args;
use crate::update::VERSION;

pub fn parse_args() -> Result<Args, String> {
    let mut tick_ms: u64 = 500;
    let mut no_mouse = false;
    let mut show_help = false;
    let mut show_version = false;

    let argv: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < argv.len() {
        let a = argv[i].as_str();
        match a {
            "--help" | "-h" => show_help = true,
            "--version" | "-V" => show_version = true,
            "--no-mouse" => no_mouse = true,
            "--tick-ms" => {
                let Some(val) = argv.get(i + 1) else {
                    return Err("--tick-ms requires a value".to_string());
                };
                let ms = val
                    .parse::<u64>()
                    .map_err(|_| format!("invalid --tick-ms value: {val}"))?;
                tick_ms = ms.clamp(50, 5000);
                i += 1;
            }
            _ if a.starts_with('-') => {
                return Err(format!("unknown option: {a}"));
            }
            _ => {}
        }
        i += 1;
    }

    Ok(Args {
        tick_ms,
        no_mouse,
        show_help,
        show_version,
    })
}

pub fn print_cli_help() {
    println!(
        "ferro {VERSION}
Lightweight Rust TUI system monitor.

USAGE:
  ferro [OPTIONS]

OPTIONS:
  --tick-ms <MS>   Refresh interval in milliseconds (default: 500, range: 50-5000)
  --no-mouse       Disable mouse support (useful in tmux/SSH)
  --version, -V    Print version and exit
  --help, -h       Print help and exit

SCREENS:
  Dashboard  — CPU, memory, disk overview (default)
  p          — Processes
  d          — Disk dive (on-demand scanner)
  v          — Services (Linux/systemd only)
  l          — Logs (Linux/journalctl + syslog fallback)

  Esc        — Back to Dashboard
  q          — Quit
  ?          — Toggle help overlay
"
    );
}
