<div align="center">
  <img src="assets/logo.png" alt="Ferromon" width="180" />

# Ferromon

A fast, lightweight TUI system monitor built in Rust. Check CPU, memory, disk, processes, services, and logs without leaving your terminal.

![Rust](https://img.shields.io/badge/rust-stable-orange)
[![Release](https://img.shields.io/github/v/release/ChrisJohnson89/Ferromon)](https://github.com/ChrisJohnson89/Ferromon/releases/latest)
![Platform](https://img.shields.io/badge/platform-linux%20%7C%20macOS-lightgrey)

</div>

## Install

```bash
curl -fsSL https://raw.githubusercontent.com/ChrisJohnson89/Ferromon/main/install.sh | bash
```

Installs to `/usr/local/bin/ferro` (falls back to `~/.local/bin` without sudo). Press `u` inside the TUI to self-update.

**Supported platforms:**
| Platform | Target |
|----------|--------|
| Linux x86_64 | `x86_64-unknown-linux-musl` (static) |
| macOS Apple Silicon | `aarch64-apple-darwin` |
| macOS Intel | `x86_64-apple-darwin` |

## Usage

```bash
ferro
ferro --tick-ms 750     # custom refresh rate
ferro --no-mouse        # disable mouse (tmux / SSH)
ferro --version
ferro --help
```

## Keybindings

| Key | Action |
|-----|--------|
| `q` / `Esc` | Quit / back to dashboard |
| `?` | Toggle help |
| `r` | Refresh now |
| `p` | Processes view |
| `d` | Disk dive |
| `v` | Services view (Linux) |
| `l` | Logs view (Linux) |
| `u` | Self-update |
| `x` | Print snapshot to stdout and exit |

### Contextual

| Screen | Key | Action |
|--------|-----|--------|
| Dashboard | `Tab` | Cycle dir target (CWD ↔ /var ↔ home ↔ /) |
| Dashboard | `f` | Toggle mount filter (filtered ↔ all) |
| Processes | `Tab` | Toggle sort (CPU ↔ Mem) |
| Disk dive | `Tab` | Cycle target (/var ↔ home ↔ /) |
| Disk dive | `s` | Scan directory |
| Disk dive | `Enter` | Drill into directory |
| Disk dive | `←` / `Backspace` | Go up |
| Services | `Tab` | Cycle filter (failed ↔ unhealthy ↔ active ↔ all) |
| Services | `Enter` / `l` | Open logs for selected unit |
| Logs | `Tab` | Cycle severity (`err+` ↔ `warning+` ↔ `info+` ↔ `debug+`) |
| Logs | `u` | Toggle selected unit ↔ all units |

## Build from Source

Requires Rust stable (rustc 1.80+). Minimum terminal size: **80×14**.

```bash
git clone https://github.com/ChrisJohnson89/Ferromon.git
cd Ferromon
cargo build --release
./target/release/ferro
```

Or install directly:

```bash
cargo install --git https://github.com/ChrisJohnson89/Ferromon --locked
```

## Related

- [Ferrolog](https://github.com/ChrisJohnson89/Ferrolog) — fast TUI log viewer

## License

MIT.
