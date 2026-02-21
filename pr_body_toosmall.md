## Summary

Improve the terminal-too-small screen by showing the current terminal size and required minimum (80x14). Previously it just said "Terminal too small" without specifics, making it harder for users to know how much to resize.

### Changes
- `render_too_small()` now displays:
  - Current terminal size (e.g., "Current: 70x12" in yellow)
  - Required minimum ("Required: 80x14 minimum" in green)
- Added minimum size requirement to README Install section

## How to test

```bash
# Build
cargo fmt && cargo clippy -- -D warnings && cargo build --release

# Test by resizing terminal
# 1. Make terminal smaller than 80x14
cargo run

# 2. Observe the new too-small screen shows:
# - Your current terminal dimensions
# - The required minimum (80x14)

# 3. Resize terminal to meet minimum
# - App should render normally

# Verify Cargo.lock
cargo check --locked
```

## Perf notes

- Zero cost: only renders when terminal is too small (rare case).
- No changes to the main render loop or any hot paths.

_This PR was generated with [Warp](https://www.warp.dev/)._
