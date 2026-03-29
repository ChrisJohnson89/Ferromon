use std::collections::VecDeque;

use ratatui::layout::Rect;
use ratatui::style::Color;

// ── Numeric helpers ───────────────────────────────────────────────────────────

pub fn percent(used: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        (used as f64 / total as f64) * 100.0
    }
}

pub fn color_for_pct(pct: f64) -> Color {
    if pct >= 90.0 {
        Color::Red
    } else if pct >= 75.0 {
        Color::Yellow
    } else {
        Color::Green
    }
}

// ── History ring helpers ──────────────────────────────────────────────────────

pub fn push_history_sample(history: &mut VecDeque<u16>, value: f64, max_len: usize) {
    history.push_back(value.round().clamp(0.0, 100.0) as u16);
    while history.len() > max_len {
        history.pop_front();
    }
}

pub fn history_peak(history: &VecDeque<u16>) -> u16 {
    history.iter().copied().max().unwrap_or(0)
}

pub fn history_average(history: &VecDeque<u16>) -> f64 {
    if history.is_empty() {
        0.0
    } else {
        history.iter().map(|sample| *sample as u64).sum::<u64>() as f64 / history.len() as f64
    }
}

// ── String helpers ────────────────────────────────────────────────────────────

pub fn trim_to(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else if max <= 1 {
        "…".to_string()
    } else {
        format!("{}…", &s[..max - 1])
    }
}

// ── Byte / rate formatters ────────────────────────────────────────────────────

pub fn format_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;
    const TIB: f64 = GIB * 1024.0;

    let b = bytes as f64;
    if b >= TIB {
        format!("{:.2} TiB", b / TIB)
    } else if b >= GIB {
        format!("{:.2} GiB", b / GIB)
    } else if b >= MIB {
        format!("{:.2} MiB", b / MIB)
    } else if b >= KIB {
        format!("{:.2} KiB", b / KIB)
    } else {
        format!("{bytes} B")
    }
}

pub fn format_rate(bps: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;
    let b = bps as f64;
    if b >= GIB {
        format!("{:.1}G/s", b / GIB)
    } else if b >= MIB {
        format!("{:.1}M/s", b / MIB)
    } else if b >= KIB {
        format!("{:.0}K/s", b / KIB)
    } else if bps == 0 {
        "—".to_string()
    } else {
        format!("{bps}B/s")
    }
}

// ── Layout helpers ────────────────────────────────────────────────────────────

pub fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    Rect {
        x,
        y,
        width: width.min(area.width),
        height: height.min(area.height),
    }
}
