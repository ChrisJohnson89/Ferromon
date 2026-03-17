# Ferromon

Lightweight Rust TUI for CPU/memory/disk monitoring. Single binary (`ferro`) built from `src/main.rs`.

## Commands

```bash
cargo build                   # debug build
cargo run -- --help           # show CLI flags
cargo run -- --tick-ms 750    # custom tick rate
cargo run -- --no-mouse       # disable mouse (tmux/SSH)
cargo fmt                     # format
cargo clippy -- -D warnings   # lint (must pass clean)
cargo build --release         # release build
```

Cross-compile release targets:
```bash
cargo build --release --locked --target x86_64-unknown-linux-musl
cargo build --release --locked --target aarch64-apple-darwin
cargo build --release --locked --target x86_64-apple-darwin
```

## Architecture

- **Single file**: all logic lives in `src/main.rs` (~3400 lines)
- **Screens**: `Dashboard`, `Processes`, `Disk`, `Services`, `Logs`
- **Tick loop**: driven by `--tick-ms` (default 500ms, range 50–5000)
- **Self-update**: checks GitHub releases; disabled by `FERRO_NO_UPDATE_CHECK=1`
- **install.sh**: fetches latest GitHub release, verifies SHA-256, installs to `/usr/local/bin` or `~/.local/bin`

## Constraints

- Anything in the tick loop must be cheap: only sysinfo reads, state updates, rendering
- No filesystem traversal, blocking IO, or expensive allocations in the render loop
- Expensive work (disk scan, service refresh) runs on explicit keypress only
- Do not break `Cargo.lock`
- Never push directly to `main`

## Release

GitHub Actions (`.github/workflows/release.yml`) triggers on `v*` tags and manual dispatch. Artifacts: `ferromon-<tag>-<target>.tar.gz` + `.sha256`. Linux `aarch64` cross-compile is pending.
