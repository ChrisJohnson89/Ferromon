use std::cmp::Reverse;

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table};

use crate::disk::disk_target_path;
use crate::types::{AppState, DiskEntryKind};
use crate::utils::format_bytes;

pub fn render_disk_dive(frame: &mut ratatui::Frame, area: Rect, app: &mut AppState) {
    let target = disk_target_path(app.disk_target);

    let state = app.disk_scan.inner.lock().unwrap();
    let current_path = state.current_path.clone().unwrap_or_else(|| target.clone());

    let title = if state.running {
        format!(
            "Disk dive  (target: {})  •  scanning",
            current_path.display()
        )
    } else {
        format!("Disk dive  (target: {})", current_path.display())
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Green));

    frame.render_widget(block.clone(), area);
    let inner = block.inner(area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(1)])
        .split(inner);

    // Status line(s)
    let status_line = if let Some(err) = &state.error {
        Line::from(vec![
            Span::styled("Error: ", Style::default().fg(Color::Red)),
            Span::raw(err.clone()),
        ])
    } else if state.running {
        Line::from(vec![
            Span::styled("Scanning… ", Style::default().fg(Color::Yellow)),
            Span::raw(state.progress.clone()),
        ])
    } else if state.results.is_empty() {
        Line::from(vec![
            Span::styled("Press ", Style::default().fg(Color::Gray)),
            Span::styled("s", Style::default().fg(Color::Yellow)),
            Span::styled(
                " to scan this directory · ",
                Style::default().fg(Color::Gray),
            ),
            Span::styled("Tab", Style::default().fg(Color::Yellow)),
            Span::styled(" to change target", Style::default().fg(Color::Gray)),
        ])
    } else {
        Line::from(vec![
            Span::styled("Cached results. ", Style::default().fg(Color::Gray)),
            Span::styled("s", Style::default().fg(Color::Yellow)),
            Span::styled(" rescan · ", Style::default().fg(Color::Gray)),
            Span::styled("Enter", Style::default().fg(Color::Yellow)),
            Span::styled(" open dir · ", Style::default().fg(Color::Gray)),
            Span::styled("←", Style::default().fg(Color::Yellow)),
            Span::styled(" up · ", Style::default().fg(Color::Gray)),
            Span::styled("↑/↓", Style::default().fg(Color::Yellow)),
            Span::styled(" select", Style::default().fg(Color::Gray)),
        ])
    };

    let status = Paragraph::new(vec![status_line]).alignment(ratatui::prelude::Alignment::Left);
    frame.render_widget(status, rows[0]);

    // Results table
    let mut results = state.results.clone();
    drop(state);
    results.sort_by_key(|entry| Reverse(entry.bytes));

    let visible = rows[1].height.saturating_sub(2) as usize; // table header + borders
    let selected = (app.disk_scroll as usize).min(results.len().saturating_sub(1));
    app.disk_scroll = selected as u16;
    let offset = selected.saturating_sub(visible.saturating_sub(1));
    let slice = &results[offset..results.len().min(offset + visible.max(1))];

    let table_rows = slice.iter().enumerate().map(|(i, entry)| {
        let absolute_idx = offset + i;
        let base_style = if absolute_idx.is_multiple_of(2) {
            Style::default().fg(Color::White)
        } else {
            Style::default().fg(Color::Gray)
        };
        let style = if absolute_idx == selected {
            base_style
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            base_style
        };
        let name = entry
            .path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| entry.path.display().to_string());
        let kind = match entry.kind {
            DiskEntryKind::Directory => "dir",
            DiskEntryKind::File => "file",
        };

        Row::new(vec![
            Cell::from(kind),
            Cell::from(name),
            Cell::from(format_bytes(entry.bytes)),
        ])
        .style(style)
    });

    let table = Table::new(
        table_rows,
        [
            Constraint::Length(6),
            Constraint::Percentage(66),
            Constraint::Length(14),
        ],
    )
    .header(
        Row::new(vec!["Kind", "Name", "Size"]).style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
    )
    .block(
        Block::default()
            .title("Largest entries")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Green)),
    );

    frame.render_widget(table, rows[1]);
}
