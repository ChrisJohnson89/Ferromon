use std::cmp::Reverse;

use ratatui::layout::{Constraint, Rect};
use ratatui::prelude::Alignment;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table};
use sysinfo::System;

use crate::services::filtered_proc_rows;
use crate::types::{AppState, ProcRow, ProcSort};
use crate::utils::{centered_rect, format_bytes, trim_to};

pub fn render_processes(
    frame: &mut ratatui::Frame,
    area: Rect,
    app: &mut AppState,
    system: &System,
) {
    let mut procs: Vec<ProcRow> = system
        .processes()
        .iter()
        .map(|(pid, p)| ProcRow::from_process(*pid, p))
        .collect();

    // Sort by current mode
    match app.proc_sort {
        ProcSort::Cpu => procs.sort_by_key(|p| Reverse((p.cpu_x10 as i64, p.mem_bytes as i64))),
        ProcSort::Mem => procs.sort_by_key(|p| Reverse((p.mem_bytes as i64, p.cpu_x10 as i64))),
        ProcSort::Swap => procs.sort_by_key(|p| Reverse((p.swap_bytes as i64, p.mem_bytes as i64))),
    }

    // In Swap mode show only processes that are actually using swap.
    if matches!(app.proc_sort, ProcSort::Swap) {
        procs.retain(|p| p.swap_bytes > 0);
    }

    // Only show top N, but allow scrolling within that list
    let max_rows = 200usize;
    if procs.len() > max_rows {
        procs.truncate(max_rows);
    }

    // Apply search filter; when active, results are sorted by name for stability
    let procs = filtered_proc_rows(procs, &app.proc_search);

    let sort_label = match app.proc_sort {
        ProcSort::Cpu => "CPU",
        ProcSort::Mem => "Mem",
        ProcSort::Swap => "Swap",
    };
    let header_title = if app.proc_search_active {
        format!(
            "Processes ({})  /{}_",
            sort_label,
            trim_to(&app.proc_search, 24)
        )
    } else if app.proc_search.is_empty() {
        match app.proc_sort {
            ProcSort::Cpu => "Top processes (CPU)".to_string(),
            ProcSort::Mem => "Top processes (Memory)".to_string(),
            ProcSort::Swap => "Top processes (Swap)".to_string(),
        }
    } else {
        format!(
            "Processes ({})  /{}  ({} match{})",
            sort_label,
            trim_to(&app.proc_search, 24),
            procs.len(),
            if procs.len() == 1 { "" } else { "es" }
        )
    };

    let block = Block::default()
        .title(header_title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let inner = block.inner(area);

    let visible = (inner.height.saturating_sub(2)) as usize; // table header + footer-ish
    let offset = app.proc_scroll as usize;
    let offset = offset.min(procs.len().saturating_sub(1));

    let slice = &procs[offset..procs.len().min(offset + visible.max(1))];

    // Calculate available width for process name column
    let show_swap = matches!(app.proc_sort, ProcSort::Swap);
    // swap mode: PID(8)+SWAP(14)+STATE(7)=29; normal: PID(8)+CPU(10)+MEM(14)+STATE(7)=39
    let fixed: u16 = if show_swap { 29 } else { 39 };
    let name_width = ((inner.width.saturating_sub(fixed)) as usize).max(16);

    let rows = slice.iter().enumerate().map(|(i, p)| {
        let state_color = match p.status {
            "Run" => Color::Green,
            "Zombie" | "Dead" => Color::Red,
            "Stop" => Color::Yellow,
            _ => Color::DarkGray,
        };
        let cells: Vec<Cell> = if show_swap {
            vec![
                Cell::from(p.pid.to_string()),
                Cell::from(trim_to(&p.name, name_width)),
                Cell::from(format_bytes(p.swap_bytes)),
                Cell::from(p.status).style(Style::default().fg(state_color)),
            ]
        } else {
            vec![
                Cell::from(p.pid.to_string()),
                Cell::from(trim_to(&p.name, name_width)),
                Cell::from(format!("{:.1}%", p.cpu_x10 as f64 / 10.0)),
                Cell::from(format_bytes(p.mem_bytes)),
                Cell::from(p.status).style(Style::default().fg(state_color)),
            ]
        };
        let row = Row::new(cells);
        if i == 0 {
            row.style(Style::default().fg(Color::Black).bg(Color::Cyan))
        } else {
            row
        }
    });

    let (widths, header_cells): (Vec<Constraint>, Vec<&str>) = if show_swap {
        (
            vec![
                Constraint::Length(8),
                Constraint::Min(16),
                Constraint::Length(14),
                Constraint::Length(7),
            ],
            vec!["PID", "NAME", "SWAP", "STATE"],
        )
    } else {
        (
            vec![
                Constraint::Length(8),
                Constraint::Min(20),
                Constraint::Length(10),
                Constraint::Length(14),
                Constraint::Length(7),
            ],
            vec!["PID", "NAME", "CPU", "MEM", "STATE"],
        )
    };

    let table = Table::new(rows, widths)
        .header(
            Row::new(header_cells).style(
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
        )
        .block(block)
        .highlight_style(Style::default().add_modifier(Modifier::BOLD));

    frame.render_widget(table, area);

    // Hint line
    let hint = if app.proc_search_active {
        Paragraph::new(Line::from(vec![
            Span::styled("Enter", Style::default().fg(Color::Yellow)),
            Span::raw(" confirm · "),
            Span::styled("Esc", Style::default().fg(Color::Yellow)),
            Span::raw(" clear · typing: "),
            Span::styled(
                format!("/{}", trim_to(&app.proc_search, 28)),
                Style::default().fg(Color::Green),
            ),
        ]))
    } else if !app.proc_search.is_empty() {
        Paragraph::new(Line::from(vec![
            Span::styled("Tab", Style::default().fg(Color::Yellow)),
            Span::raw(" sort · "),
            Span::styled("/", Style::default().fg(Color::Yellow)),
            Span::raw(" search · "),
            Span::styled("Esc", Style::default().fg(Color::Yellow)),
            Span::raw(" clear filter"),
        ]))
    } else {
        Paragraph::new(Line::from(vec![
            Span::styled("Tab", Style::default().fg(Color::Yellow)),
            Span::raw(" CPU/Mem/Swap · "),
            Span::styled("↑/↓", Style::default().fg(Color::Yellow)),
            Span::raw(" scroll · "),
            Span::styled("/", Style::default().fg(Color::Yellow)),
            Span::raw(" search · "),
            Span::styled("k", Style::default().fg(Color::Red)),
            Span::raw(" kill · "),
            Span::styled("R", Style::default().fg(Color::Yellow)),
            Span::raw(" restart · top "),
            Span::styled(max_rows.to_string(), Style::default().fg(Color::White)),
        ]))
    };
    let hint = hint.alignment(Alignment::Left);

    let hint_area = Rect {
        x: inner.x,
        y: inner.y + inner.height.saturating_sub(1),
        width: inner.width,
        height: 1,
    };
    frame.render_widget(hint, hint_area);

    // Kill confirmation overlay
    if let Some((pid, name)) = &app.proc_kill_confirm {
        let popup = centered_rect(54, 5, area);
        frame.render_widget(Clear, popup);
        let block = Block::default()
            .title(" Kill Process ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Red));
        let inner_popup = block.inner(popup);
        frame.render_widget(block, popup);
        let text = vec![
            Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    trim_to(name, 28),
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(format!("  PID {}", pid)),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    "y",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" SIGTERM  "),
                Span::styled(
                    "K",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ),
                Span::raw(" SIGKILL  "),
                Span::styled("n/Esc", Style::default().fg(Color::Yellow)),
                Span::raw(" cancel"),
            ]),
        ];
        frame.render_widget(Paragraph::new(text), inner_popup);
    }

    // Restart confirmation overlay
    if let Some((pid, name, exe, _)) = &app.proc_restart_confirm {
        let popup = centered_rect(60, 6, area);
        frame.render_widget(Clear, popup);
        let block = Block::default()
            .title(" Restart Process ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow));
        let inner_popup = block.inner(popup);
        frame.render_widget(block, popup);
        let exe_str = exe.to_string_lossy();
        let text = vec![
            Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    trim_to(name, 28),
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(format!("  PID {}", pid)),
            ]),
            Line::from(vec![
                Span::styled("  exe: ", Style::default().fg(Color::Gray)),
                Span::styled(trim_to(&exe_str, 46), Style::default().fg(Color::DarkGray)),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::raw("  SIGTERM then respawn  "),
                Span::styled(
                    "y",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" confirm  "),
                Span::styled("n/Esc", Style::default().fg(Color::Yellow)),
                Span::raw(" cancel"),
            ]),
        ];
        frame.render_widget(Paragraph::new(text), inner_popup);
    }
}
