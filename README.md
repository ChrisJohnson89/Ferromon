# Ferromon

A lightweight, interactive **Rust TUI** for quick host health checks (CPU / memory / disk) — built with **ratatui + crossterm + sysinfo**.

## Features
- Live **CPU** gauge
- **Memory** usage (used/total + percent)
- **Disk** usage (used/total + percent) for your primary disk
- Fast refresh loop (default: 500ms)

## Controls
- `q` — quit
- `r` — refresh now
- `h` — toggle help

## Run
```bash
cargo run
```

## Notes / roadmap
- [ ] Multi-disk view + sorting
- [ ] Per-core CPU view
- [ ] Threshold coloring / alerts
- [ ] Configurable refresh rate + CLI flags
- [ ] Export snapshot to JSON

---

Built as a “usable first” tool: minimal, responsive, and easy to extend.
