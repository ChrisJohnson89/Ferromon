## Summary

Add `f` key toggle on the Dashboard to switch between filtered and unfiltered mount views. By default, Ferromon filters out pseudo-filesystems (tmpfs, devtmpfs, udev, /run, /dev, /sys) for a cleaner df view. Pressing `f` shows everything when needed.

### Changes
- New `dash_show_all_mounts` boolean in `AppState` (defaults to false)
- `disks_table_filtered()` now accepts `show_all` parameter to skip filtering logic
- Disk panel title shows `(filtered)` or `(all mounts)` based on state
- Updated header hint, footer tips, help text, and README with `f` key

## How to test

```bash
# Build
cargo fmt && cargo clippy -- -D warnings && cargo build --release

# Run and toggle the filter with 'f'
cargo run

# In the Dashboard:
# - Press 'f' to toggle filter on/off
# - Observe Disk panel title changes from "(filtered)" to "(all mounts)"
# - Verify mount list includes/excludes tmpfs, devtmpfs, etc.

# Verify help shows new key
# - Press '?' to see help
# - Check Dashboard section lists 'f — toggle mount filter'

# Verify Cargo.lock
cargo check --locked
```

## Perf notes

- Zero runtime cost: toggling the flag does not trigger a disk refresh, just re-filters the existing snapshot.
- No per-tick overhead: filter decision happens once per snapshot call (which is already throttled).

_This PR was generated with [Warp](https://www.warp.dev/)._
