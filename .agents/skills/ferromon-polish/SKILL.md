---
name: ferromon-polish
description: Refine and clean up the Ferromon Rust TUI with small user-facing improvements. Maintain performance, keep the code simple, and ensure the project remains installable via cargo.
---

# Ferromon Polish Skill

You are working inside the `ChrisJohnson89/Ferromon` repository.

Ferromon is a lightweight Rust terminal system monitor built with:

- ratatui
- crossterm
- sysinfo

The purpose of this skill is to **polish the project**, not redesign it.

Focus on clarity, usability, and code cleanliness while preserving performance.

---

# Goal

Deliver small, user-facing improvements that make Ferromon feel cleaner and more reliable without increasing complexity.

Examples of acceptable polish work:

- UI readability improvements
- alignment and truncation fixes
- simplifying code paths
- removing dead code
- improving error handling
- improving CLI argument validation
- small UX refinements

Do not introduce large new features.

---

# Hard Rules

Code must compile cleanly.

Run:

cargo fmt  
cargo clippy -- -D warnings  
cargo build --release  

The project must remain installable with:

cargo install --git https://github.com/ChrisJohnson89/Ferromon --locked --force

Do not break Cargo.lock.

Do not add heavy background work.

Do not introduce filesystem scans or expensive operations inside the dashboard refresh loop.

Prefer on-demand actions triggered by keypress instead of continuous background processing.

---

# Performance Expectations

Ferromon is a real-time terminal tool.

Anything executed every tick must be extremely cheap.

Allowed in the refresh loop:

- sysinfo metric reads
- lightweight state updates
- rendering

Not allowed in the refresh loop:

- filesystem traversal
- expensive allocations
- blocking IO

Expensive work must only run on explicit user action.

---

# Workflow

1. Read the current code and README.
2. Identify one small polish improvement.
3. Implement the improvement.
4. Ensure formatting, linting, and compilation pass.
5. Update README if the behavior or UX changes.

Open a PR containing:

- short summary of the improvement
- exact commands to test the change
- performance notes if the change touches the render loop

Keep commits small and focused.

Never push directly to main.
