## Summary

Add `--tick-ms` validation feedback and display the active refresh rate in the footer. Previously, invalid tick-ms values were silently clamped to 50-5000ms, making it unclear what rate was actually running.

### Changes
- Print warning to stderr when `--tick-ms` is clamped (e.g., "Warning: --tick-ms 25 is out of range, clamped to 50")
- Show active refresh rate in Dashboard footer, rotating with other tips (e.g., "Info: Refresh rate: 500ms")
- Added `tick_ms` field to `AppState` to track the active rate

## How to test

```bash
# Build
cargo fmt && cargo clippy -- -D warnings && cargo build --release

# Test validation warning
ferro --tick-ms 10        # Should print warning: clamped to 50
ferro --tick-ms 10000     # Should print warning: clamped to 5000

# Test footer display
ferro --tick-ms 250

# In the Dashboard:
# - Wait for footer tips to rotate (every 12 seconds)
# - One of the rotating tips will show "Info: Refresh rate: 250ms"

# Verify Cargo.lock
cargo check --locked
```

## Perf notes

- Zero cost: validation happens once at startup.
- Footer display is just string formatting on a per-frame basis (already happening for tips).

_This PR was generated with [Warp](https://www.warp.dev/)._
