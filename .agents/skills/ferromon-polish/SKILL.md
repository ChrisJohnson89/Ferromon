---
name: ferromon-polish
description: Polish Ferromon (Rust TUI) with small, user-facing improvements. Keep performance snappy, avoid heavy scans in hot loops, and always preserve installability (cargo install --git … --locked). Open PRs with clean commits + notes + screenshots if possible.
---

# Ferromon Polish Skill

You are an agent working in the `ChrisJohnson89/Ferromon` repo.

## Goal
Ship small, user-facing improvements to the Ferromon TUI while keeping it fast and installable.

## Hard rules
- MUST compile: `cargo fmt && cargo clippy -- -D warnings && cargo build --release`
- MUST be installable: `cargo install --git https://github.com/ChrisJohnson89/Ferromon --locked --force` (at least ensure `Cargo.lock` stays valid)
- Do NOT add expensive filesystem scans to the dashboard refresh loop.
- Prefer toggles + on-demand actions over background work.
- Keep changes PR-sized (one feature per PR unless tightly related).
- No force-push to `main`. Use PRs.

## Workflow
1) Read current UX pain points from README + recent commits.
2) Pick ONE improvement from the list below.
3) Implement with tests/guards as appropriate.
4) Update README and add a screenshot (or describe how to capture).
5) Open a PR with:
   - summary
   - how to test (exact commands)
   - perf notes (what runs every tick vs on-demand)

## Suggested improvements (pick 1)
- Add mount filter toggle (`f`) to switch filtered df view ↔ all mounts.
- Improve terminal-too-small screen (show required min size).
- Improve truncation/alignment for process rows and filesystem table.
- Add CLI flag `--no-mouse` (some terminals hate mouse capture).
- Add `--tick-ms` validation + show active tick in footer.
- Make dashboard Tab behavior more discoverable and consistent.

## Performance checklist
- Anything run every tick must be O(1) or cheap sysinfo calls.
- Any directory walk must be behind a keypress (on-demand) and show progress.
