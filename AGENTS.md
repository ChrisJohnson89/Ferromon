# AGENTS.md

## Repo Commands
- `cargo build`
- `cargo run -- --help`
- `cargo run -- --tick-ms 750`
- `cargo run -- --no-mouse`
- `cargo build --release --locked --target x86_64-unknown-linux-musl`
- `cargo build --release --locked --target aarch64-apple-darwin`
- `cargo build --release --locked --target x86_64-apple-darwin`

## Repo Facts
- The crate builds a single binary named `ferro` from `src/main.rs`.
- `install.sh` installs the latest GitHub release asset for the current platform and verifies the published SHA-256 checksum before copying `ferro` into `/usr/local/bin` or `~/.local/bin`.
- Self-update is implemented in the TUI; `FERRO_NO_UPDATE_CHECK=1` disables release update checks.

## Release Workflow
- GitHub Actions release automation lives in `.github/workflows/release.yml`.
- It runs on `v*` tags and on manual dispatch.
- The workflow builds release artifacts for:
  - `x86_64-unknown-linux-musl`
  - `aarch64-apple-darwin`
  - `x86_64-apple-darwin`
- Each release artifact is packaged as `ferromon-<tag>-<target>.tar.gz` with a matching `.sha256` file.
- TODO: Linux `aarch64` release builds are intentionally disabled in the workflow pending cross-compilation tooling.
