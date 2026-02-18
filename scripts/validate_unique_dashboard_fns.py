#!/usr/bin/env python3
"""Fail CI if known dashboard helper functions are defined more than once."""

from __future__ import annotations

import re
import sys
from pathlib import Path

MAIN_RS = Path("src/main.rs")
FUNCTIONS = [
    "render_memory_panel",
    "render_threshold_bands",
    "step_points_for_width",
    "render_vertical_usage_bar",
    "horizontal_line",
    "segmented_usage_spans",
    "color_for_memory_threshold",
    "summarize_health",
    "top_processes",
    "cpu_pressure_color",
    "memory_pressure_color",
    "available_percent",
    "swap_is_increasing",
    "swap_trend",
    "read_run_queue",
    "sample_steal_percent",
    "push_capped",
]


def main() -> int:
    if not MAIN_RS.exists():
        print(f"error: missing file: {MAIN_RS}", file=sys.stderr)
        return 2

    source = MAIN_RS.read_text(encoding="utf-8")
    failed = False

    for name in FUNCTIONS:
        count = len(re.findall(rf"(?m)^fn\s+{re.escape(name)}\s*\(", source))
        if count != 1:
            failed = True
            print(f"error: expected exactly 1 definition of `{name}`, found {count}")

    if failed:
        print("\nDuplicate or missing dashboard function definitions detected.")
        return 1

    print("OK: dashboard function definitions are unique.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
