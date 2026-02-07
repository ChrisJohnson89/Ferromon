# Ferromon

A lightweight, interactive **Rust TUI** for quick host health checks (CPU / memory / disk) — built with **ratatui + crossterm + sysinfo**.

## Features
- Live **CPU** gauge
- **Memory** usage (used/total + percent)
- **Disk** usage (used/total + percent) for your primary disk
- **Processes view**: top CPU / top memory
- **Disk dive** (on-demand): find biggest directories without slowing the dashboard
- Fast refresh loop (default: 500ms)

## Controls
- `q` — quit
- `?` — help
- `p` — processes view
- `d` — disk dive
- `r` — refresh now

## Install (Rust required)
```bash
cargo install --git https://github.com/ChrisJohnson89/Ferromon --locked
```

Then run:
```bash
ferro
```

## Dev run
```bash
cargo run
```

## Notes / roadmap
- [ ] Per-core CPU view
- [ ] Threshold coloring / alerts
- [ ] Configurable refresh rate + CLI flags
- [ ] Export snapshot to JSON
- [ ] Process details (cmdline/user) + kill action
- [ ] Disk dive drill-down (enter) + ignore patterns

---

Built as a “usable first” tool: minimal, responsive, and easy to extend.
