pub const MIN_TICK_MS: u64 = 50;
pub const MAX_TICK_MS: u64 = 5000;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Args {
    pub tick_ms_override: Option<u64>,
    pub no_mouse: bool,
    pub show_help: bool,
    pub show_version: bool,
}

pub fn parse_args() -> Args {
    let mut tick_ms_override = None;
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
                    tick_ms_override = Some(v);
                    i += 1;
                } else {
                    show_help = true;
                }
            }
            _ if a.starts_with("--tick-ms=") => {
                if let Some(v) = a.split('=').nth(1) {
                    if let Ok(v) = v.parse::<u64>() {
                        tick_ms_override = Some(v);
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
        tick_ms_override,
        no_mouse,
        show_help,
        show_version,
    }
}

pub fn print_cli_help(version: &str) {
    println!("ferro {version}");
    println!(
        "
USAGE:
  ferro [--tick-ms <ms>]
"
    );
    println!("OPTIONS:");
    println!("  --tick-ms <ms>   UI refresh tick ({MIN_TICK_MS}..{MAX_TICK_MS}). Default: 500");
    println!("  --no-mouse       Disable mouse capture (useful in tmux/SSH)");
    println!("  -h, --help       Show help");
    println!("  -V, --version    Show version");
    println!(
        "
CONFIG:
  ~/.config/ferro/config.toml supports: tick_ms, no_mouse, default_screen

KEYS (in-app):
  q quit · ? help · Esc back · p processes · d disk dive · v services · l logs · r refresh · f mounts · u update/filter · x snapshot

UPDATE:
  Ferromon checks GitHub releases occasionally and can self-update.
  Set FERRO_NO_UPDATE_CHECK=1 to disable checks."
    );
}
