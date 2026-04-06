use std::collections::VecDeque;

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::prelude::Alignment;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Sparkline, Wrap};

use crate::types::{AppState, Screen};
use crate::utils::color_for_pct;

pub fn render_header(app: &AppState) -> Paragraph<'static> {
    let (screen_name, screen_hint) = match app.screen {
        Screen::Dashboard => ("Dashboard", "p: processes  d: disk  v: services  l: logs"),
        Screen::Processes => (
            "Processes",
            "Tab: CPU/Mem/Swap  k: kill  R: restart  Esc: back",
        ),
        Screen::DiskDive => ("Disk dive", "s: scan  Enter: open dir  ←: up  Tab: target"),
        Screen::Services => (
            "Services",
            "Tab: filter  /: search  Enter/l: logs  r: refresh",
        ),
        Screen::Logs => ("Logs", "Tab: severity  u: unit filter  r: refresh"),
    };

    Paragraph::new(Line::from(vec![
        Span::styled(
            "Ferromon",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  —  "),
        Span::styled(screen_name, Style::default().fg(Color::White)),
        Span::raw("  •  "),
        Span::styled(
            screen_hint,
            Style::default()
                .fg(Color::Gray)
                .add_modifier(Modifier::ITALIC),
        ),
        Span::raw("  •  "),
        Span::styled(app.hostname.clone(), Style::default().fg(Color::Yellow)),
        Span::raw("  •  "),
        Span::styled("q", Style::default().fg(Color::Yellow)),
        Span::raw(": quit  "),
        Span::styled("?", Style::default().fg(Color::Yellow)),
        Span::raw(": help"),
    ]))
}

pub fn render_footer(app: &AppState) -> Paragraph<'static> {
    let tips_dashboard = [
        "Tab: cycle dir target (CWD ↔ /var ↔ HOME ↔ /)",
        "f: toggle mount filter (filtered ↔ all)",
        "p: processes · d: disk dive · v: services · l: logs",
        "r: refresh now · ?: help",
        "Esc: back to dashboard",
    ];

    let tips_processes = [
        "Tab: sort CPU → Mem → Swap → CPU",
        "↑/↓: scroll · k: kill · R: restart",
        "Swap column: Linux only (0 on macOS)",
        "Esc: back",
    ];

    let tips_disk = [
        "s: scan (on-demand)",
        "Tab: change target (/var ↔ home ↔ /)",
        "Enter: open dir · ←/Backspace: up",
        "↑/↓: select · Esc: back",
    ];

    let tips_services = [
        "Tab: filter failed/unhealthy/active/all",
        "/: search unit or description",
        "Esc clears search, then returns to dashboard",
        "Enter or l: open logs for selected unit",
        "↑/↓: select unit · r: refresh",
    ];

    let tips_logs = [
        "Tab: cycle severity err+/warning+/info+/debug+",
        "u: selected unit ↔ all units",
        "↑/↓: scroll · r: refresh",
    ];

    let (label, tip) = match app.screen {
        Screen::Dashboard => {
            let idx = app.footer_tip_idx as usize % (tips_dashboard.len() + 1);
            if idx == tips_dashboard.len() {
                ("Info", format!("Refresh rate: {}ms", app.tick_ms))
            } else {
                ("Tip", tips_dashboard[idx].to_string())
            }
        }
        Screen::Processes => (
            "Tip",
            tips_processes[(app.footer_tip_idx as usize) % tips_processes.len()].to_string(),
        ),
        Screen::DiskDive => (
            "Tip",
            tips_disk[(app.footer_tip_idx as usize) % tips_disk.len()].to_string(),
        ),
        Screen::Services => (
            "Tip",
            tips_services[(app.footer_tip_idx as usize) % tips_services.len()].to_string(),
        ),
        Screen::Logs => (
            "Tip",
            tips_logs[(app.footer_tip_idx as usize) % tips_logs.len()].to_string(),
        ),
    };

    let tip_line = Line::from(vec![
        Span::styled(
            format!("{label}: "),
            Style::default()
                .fg(Color::Gray)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(tip),
    ]);

    if app.update.available {
        let tag = app
            .update
            .latest_tag
            .clone()
            .unwrap_or_else(|| "new".to_string());
        let update_line = Line::from(vec![
            Span::styled(
                "Update available ",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(tag, Style::default().fg(Color::Green)),
            Span::styled(
                "  —  press ",
                Style::default().fg(Color::Yellow),
            ),
            Span::styled("[u]", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::styled(" on Dashboard to install", Style::default().fg(Color::Yellow)),
        ]);
        Paragraph::new(vec![update_line, tip_line])
    } else {
        Paragraph::new(tip_line)
    }
}

pub fn render_help(app: &AppState) -> Paragraph<'static> {
    let mut lines = vec![
        Line::from("Global:"),
        Line::from("  q — quit"),
        Line::from("  ? — toggle help"),
        Line::from("  Esc — back to dashboard"),
        Line::from("  r — refresh now"),
        Line::from("  v — services"),
        Line::from("  l — logs"),
        Line::from(""),
    ];

    match app.screen {
        Screen::Dashboard => {
            lines.push(Line::from("Dashboard:"));
            lines.push(Line::from("  p — processes"));
            lines.push(Line::from("  d — disk dive"));
            lines.push(Line::from("  f — toggle mount filter (filtered ↔ all)"));
            lines.push(Line::from(
                "  Tab — cycle dir target (CWD ↔ /var ↔ HOME ↔ /)",
            ));
        }
        Screen::Processes => {
            lines.push(Line::from("Processes:"));
            lines.push(Line::from("  Tab — cycle CPU / Mem / Swap sort"));
            lines.push(Line::from("  ↑/↓ — scroll · / — search"));
            lines.push(Line::from("  k — kill selected (y=SIGTERM, K=SIGKILL)"));
            lines.push(Line::from("  R — restart selected (SIGTERM then respawn)"));
            lines.push(Line::from("  Swap column: Linux only (macOS shows 0 B)"));
        }
        Screen::DiskDive => {
            lines.push(Line::from("Disk dive:"));
            lines.push(Line::from("  s — start scan"));
            lines.push(Line::from("  Tab — change target (/var ↔ home ↔ /)"));
            lines.push(Line::from("  ↑/↓ — select"));
            lines.push(Line::from("  Enter — scan selected directory"));
            lines.push(Line::from("  ← / Backspace — go to parent directory"));
        }
        Screen::Services => {
            lines.push(Line::from("Services (Linux-only):"));
            lines.push(Line::from(
                "  Tab — cycle filters (failed ↔ unhealthy ↔ active ↔ all)",
            ));
            lines.push(Line::from("  / — search by service name or description"));
            lines.push(Line::from(
                "  Backspace — edit search · Esc — clear search/back",
            ));
            lines.push(Line::from("  ↑/↓ — select service"));
            lines.push(Line::from("  Enter / l — open logs for selected unit"));
            lines.push(Line::from("  r — refresh service list"));
        }
        Screen::Logs => {
            lines.push(Line::from("Logs (Linux-only):"));
            lines.push(Line::from(
                "  Tab — cycle severity (err+ ↔ warning+ ↔ info+ ↔ debug+)",
            ));
            lines.push(Line::from("  u — selected unit ↔ all units"));
            lines.push(Line::from("  ↑/↓ — scroll"));
            lines.push(Line::from("  r — refresh logs"));
        }
    }

    Paragraph::new(lines)
        .block(Block::default().title("Help").borders(Borders::ALL))
        .wrap(Wrap { trim: true })
}

pub fn render_too_small(frame: &mut ratatui::Frame, area: Rect) {
    let size = frame.size();
    let msg = vec![
        Line::from("Ferromon"),
        Line::from(""),
        Line::from("Terminal too small."),
        Line::from(""),
        Line::from(vec![
            Span::styled("Current: ", Style::default().fg(Color::Gray)),
            Span::styled(
                format!("{}x{}", size.width, size.height),
                Style::default().fg(Color::Yellow),
            ),
        ]),
        Line::from(vec![
            Span::styled("Required: ", Style::default().fg(Color::Gray)),
            Span::styled("80x14 minimum", Style::default().fg(Color::Green)),
        ]),
        Line::from(""),
        Line::from("Resize and try again."),
        Line::from(""),
        Line::from("Tip: you can also run: ferro --help"),
    ];

    frame.render_widget(
        Paragraph::new(msg)
            .alignment(Alignment::Center)
            .block(Block::default().title("Ferromon").borders(Borders::ALL)),
        area,
    );
}

pub fn render_detail_panel(
    frame: &mut ratatui::Frame,
    area: Rect,
    title: &str,
    lines: Vec<String>,
    history: &VecDeque<u16>,
    color: Color,
) {
    let block = Block::default().borders(Borders::ALL).title(title);
    frame.render_widget(block.clone(), area);
    let inner = block.inner(area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(lines.len() as u16), Constraint::Min(1)])
        .split(inner);

    frame.render_widget(
        Paragraph::new(lines.into_iter().map(Line::from).collect::<Vec<_>>())
            .style(Style::default().fg(Color::Gray))
            .alignment(Alignment::Left),
        chunks[0],
    );

    let data: Vec<u64> = history.iter().map(|sample| *sample as u64).collect();
    if !data.is_empty() && chunks[1].width > 0 && chunks[1].height > 0 {
        let spark_color = history
            .back()
            .map(|sample| color_for_pct(*sample as f64))
            .unwrap_or(color);
        let spark_width = chunks[1].width.min(data.len() as u16);
        let spark_x = chunks[1].x + (chunks[1].width.saturating_sub(spark_width)) / 2;
        let spark_area = Rect {
            x: spark_x,
            y: chunks[1].y,
            width: spark_width,
            height: chunks[1].height,
        };
        frame.render_widget(
            Sparkline::default()
                .data(&data)
                .max(100)
                .style(Style::default().fg(spark_color)),
            spark_area,
        );
    }
}
