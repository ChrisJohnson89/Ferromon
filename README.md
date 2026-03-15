```text
███████╗███████╗██████╗ ██████╗  ██████╗ ███╗   ███╗ ██████╗ ███╗   ██╗
██╔════╝██╔════╝██╔══██╗██╔══██╗██╔═══██╗████╗ ████║██╔═══██╗████╗  ██║
█████╗  █████╗  ██████╔╝██████╔╝██║   ██║██╔████╔██║██║   ██║██╔██╗ ██║
██╔══╝  ██╔══╝  ██╔══██╗██╔══██╗██║   ██║██║╚██╔╝██║██║   ██║██║╚██╗██║
██║     ███████╗██║  ██║██║  ██║╚██████╔╝██║ ╚═╝ ██║╚██████╔╝██║ ╚████║
╚═╝     ╚══════╝╚═╝  ╚═╝╚═╝  ╚═╝ ╚═════╝ ╚═╝     ╚═╝ ╚═════╝ ╚═╝  ╚═══╝

                     Forge‑Grade Terminal Monitoring
                      CPU • Memory • System Insight
```
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
- Disk dive (`d`): on-demand directory sizing with drill-down into directories and large files
- Services view (`v`, Linux): `systemd` unit health, failed services, restart counts, recent state changes
- Logs view (`l`, Linux): `journalctl` tailing with severity and unit filters, with syslog fallback
- Refresh rate control via CLI flag

## Install

### One-liner install (recommended)
This installs the latest release for your OS/arch and verifies checksums.

```bash
curl -fsSL https://raw.githubusercontent.com/ChrisJohnson89/Ferromon/main/install.sh | bash
```

After installing, Ferromon can also self-update from inside the TUI (press `u`).

### Prebuilt binaries (manual)
Grab the right archive from **GitHub Releases**, extract, and place `ferro` on your PATH.

Example (Linux x86_64):
```bash
# pick a version from: https://github.com/ChrisJohnson89/Ferromon/releases
VER=v0.3.12
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
- `v` services (Linux)
- `l` logs (Linux)
- `r` refresh now

### Contextual
- Dashboard: `Tab` cycles dir target (CWD ↔ /var ↔ home ↔ /), `f` toggles mount filter (filtered ↔ all)
- Dashboard: `x` prints a text snapshot to stdout and exits
- Dashboard: `u` downloads and installs the latest release to `~/.local/bin/ferro`
- Processes: `Tab` toggles sort (CPU ↔ Mem)
- Disk dive: `Tab` cycles target (/var ↔ home ↔ /), `s` scans, `Enter` drills into a directory, `←`/`Backspace` goes up
- Services: `Tab` cycles filters (failed ↔ unhealthy ↔ active ↔ all), `Enter`/`l` opens logs for the selected unit
- Logs: `Tab` cycles severity (`err+` ↔ `warning+` ↔ `info+` ↔ `debug+`), `u` toggles selected unit ↔ all units

## SRE Roadmap
Features that would make Ferromon much stronger as a sysadmin/SRE first-response tool:

### Highest priority
- Service health: `systemd` units, failed services, restart counts, recent state changes
- Log tailing: `journalctl`/syslog view with severity and unit filters
- Network visibility: listening ports, established connections, RX/TX throughput, top sockets by process
- Host pressure signals: load average, swap, iowait, PSI, inode usage, open file descriptors
- Process inspection/actions: full command line, parent/child tree, cwd/exe path, signals/kill
- Better snapshots: JSON/text export with hostname, kernel, timestamp, services, network, and log context
- Container/runtime visibility: Docker/containerd/Kubernetes pod and container summaries
- Threshold highlighting: obvious warnings for hot CPU, low memory, failed units, full disks, inode exhaustion

### Second wave
- Historical mini-trends: small sparklines or a short rolling history for CPU, memory, network, and disk IO
- Filesystem detail: read-only mounts, mount options, NFS/stale mount visibility
- Search/filter UX: quick filtering for processes, services, mounts, and logs
- Remote mode: SSH collection or remote snapshot mode for fast fleet triage

### Suggested build order
1. Service health
2. Logs
3. Network
4. Load/swap/pressure/inodes
5. Process inspection/actions
6. JSON snapshot/export
7. Container visibility

## Screenshot
*(add one)*

## License
MIT.
