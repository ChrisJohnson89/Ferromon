# Ferromon

A fast, interactive **Rust TUI** for quick host checks.

Built with **ratatui + crossterm + sysinfo**.

## Why
You want a “what’s going on with this box?” view in ~2 seconds:
- CPU + memory pressure
- top processes
- disk usage (df-style)
- one-key deeper dives when needed

## Features
- Dashboard: CPU + Memory gauges + **top processes**
- Disk panel: compact **df-style** overview (filtered to “real” mounts)
- Processes view (`p`): top CPU/mem with scroll
- Disk dive (`d`): on-demand directory sizing (kept out of the hot loop)
- Refresh rate control via CLI flag

## Install

### Prebuilt binaries (recommended)
Grab the right archive from **GitHub Releases**, extract, and place `ferro` on your PATH.

Example (Linux x86_64):
```bash
# pick a version from: https://github.com/ChrisJohnson89/Ferromon/releases
VER=v0.3.1
TARGET=x86_64-unknown-linux-gnu
curl -L -o ferromon.tar.gz "https://github.com/ChrisJohnson89/Ferromon/releases/download/${VER}/ferromon-${VER}-${TARGET}.tar.gz"

tar -xzf ferromon.tar.gz
chmod +x ferro
sudo mv ferro /usr/local/bin/ferro

ferro --version
```

### Build from source
Rust required (**rustc 1.80+**). Minimum terminal size: **80x14**.

```bash
cargo install --git https://github.com/ChrisJohnson89/Ferromon --locked
```

Run:
```bash
ferro
```

## CLI
```bash
ferro --help
ferro --version
ferro --tick-ms 750
ferro --no-mouse        # disable mouse capture (useful in tmux/SSH)
```

## Keys
- `q` quit
- `?` help
- `Esc` back to dashboard
- `p` processes
- `d` disk dive
- `r` refresh now

### Contextual
- Dashboard: `Tab` toggles dir target (CWD ↔ /var), `f` toggles mount filter (filtered ↔ all), `x` prints a text snapshot to stdout and exits
- Processes: `Tab` toggles sort (CPU ↔ Mem)
- Disk dive: `Tab` cycles target (/var ↔ home ↔ /), `s` scans

## Screenshot
*(add one)*

## License
MIT.
