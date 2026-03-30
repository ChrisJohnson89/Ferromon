use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table, Wrap};

use crate::services::{filtered_service_rows, service_filter_label};
use crate::types::{AppState, ServiceHealth};
use crate::utils::trim_to;

pub fn render_services(frame: &mut ratatui::Frame, area: Rect, app: &mut AppState) {
    let state = app.service_state.inner.lock().unwrap();

    if let Some(msg) = &state.unsupported {
        frame.render_widget(
            Paragraph::new(vec![
                Line::from("Services"),
                Line::from(""),
                Line::from(msg.clone()),
                Line::from(""),
                Line::from("You can still ship this build and test the Linux path on a server."),
            ])
            .block(Block::default().title("Services").borders(Borders::ALL))
            .alignment(ratatui::prelude::Alignment::Center),
            area,
        );
        return;
    }

    let rows = filtered_service_rows(&state.rows, app.service_filter, &app.service_search);
    let error = state.error.clone();
    let failed = state
        .rows
        .iter()
        .filter(|row| row.health == ServiceHealth::Critical)
        .count();
    let unhealthy = state
        .rows
        .iter()
        .filter(|row| row.health == ServiceHealth::Warning)
        .count();
    let active = state
        .rows
        .iter()
        .filter(|row| row.active_state == "active")
        .count();
    let updated = state
        .last_updated_at
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| {
            format!(
                "updated {}s ago",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .ok()
                    .map(|now| now.as_secs().saturating_sub(d.as_secs()))
                    .unwrap_or(0)
            )
        })
        .unwrap_or_else(|| "not loaded yet".to_string());
    let selected = rows.get(app.service_scroll as usize).cloned();
    drop(state);

    let title = if app.service_search_active {
        format!(
            "Services ({})  /{}_",
            service_filter_label(app.service_filter),
            trim_to(&app.service_search, 18)
        )
    } else if app.service_search.is_empty() {
        format!("Services ({})", service_filter_label(app.service_filter))
    } else {
        format!(
            "Services ({})  /{}",
            service_filter_label(app.service_filter),
            trim_to(&app.service_search, 18)
        )
    };
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Blue));
    frame.render_widget(block.clone(), area);
    let inner = block.inner(area);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(6),
            Constraint::Length(5),
        ])
        .split(inner);

    let summary = Paragraph::new(vec![Line::from(vec![
        Span::styled(
            "Failed ",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ),
        Span::raw(failed.to_string()),
        Span::raw("  "),
        Span::styled(
            "Warning ",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(unhealthy.to_string()),
        Span::raw("  "),
        Span::styled(
            "Active ",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(active.to_string()),
        Span::raw("  •  "),
        Span::styled(updated, Style::default().fg(Color::Gray)),
        if app.service_search_active {
            Span::styled(
                format!("  •  typing /{}", trim_to(&app.service_search, 20)),
                Style::default().fg(Color::Gray),
            )
        } else if app.service_search.is_empty() {
            Span::raw("")
        } else {
            Span::styled(
                format!("  •  search /{}", trim_to(&app.service_search, 20)),
                Style::default().fg(Color::Gray),
            )
        },
        if rows.is_empty() {
            Span::styled("  •  no matching units", Style::default().fg(Color::Gray))
        } else {
            Span::styled(
                format!("  •  showing {}", rows.len()),
                Style::default().fg(Color::Gray),
            )
        },
    ])]);
    frame.render_widget(summary, chunks[0]);

    let visible = chunks[1].height.saturating_sub(3) as usize;
    let selected_idx = (app.service_scroll as usize).min(rows.len().saturating_sub(1));
    app.service_scroll = selected_idx as u16;
    let offset = selected_idx.saturating_sub(visible.saturating_sub(1));
    let slice = &rows[offset..rows.len().min(offset + visible.max(1))];

    let table_rows = slice.iter().enumerate().map(|(i, row)| {
        let absolute_idx = offset + i;
        let base_style = match row.health {
            ServiceHealth::Critical => Style::default().fg(Color::Red),
            ServiceHealth::Warning => Style::default().fg(Color::Yellow),
            ServiceHealth::Healthy => Style::default().fg(Color::Green),
        };
        let style = if absolute_idx == selected_idx {
            base_style
                .bg(Color::White)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD)
        } else {
            base_style
        };
        Row::new(vec![
            Cell::from(trim_to(&row.name, 26)),
            Cell::from(format!("{} ({})", row.active_state, row.sub_state)),
            Cell::from(row.restarts.to_string()),
            Cell::from(trim_to(&row.last_change, 24)),
            Cell::from(trim_to(&row.description, 28)),
        ])
        .style(style)
    });

    let table = Table::new(
        table_rows,
        [
            Constraint::Length(26),
            Constraint::Length(20),
            Constraint::Length(8),
            Constraint::Length(24),
            Constraint::Min(18),
        ],
    )
    .header(
        Row::new(vec!["UNIT", "STATE", "RESTARTS", "LAST CHANGE", "DESC"]).style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
    )
    .block(Block::default().borders(Borders::ALL).title("Units"));
    frame.render_widget(table, chunks[1]);

    let detail_lines = if let Some(error) = error {
        vec![
            Line::from(vec![
                Span::styled(
                    "Service refresh failed: ",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ),
                Span::raw(error),
            ]),
            Line::from("Press r to retry."),
        ]
    } else if let Some(row) = selected {
        vec![
            Line::from(vec![
                Span::styled("Selected: ", Style::default().fg(Color::Gray)),
                Span::raw(row.name),
                Span::raw("  •  "),
                Span::styled("Load ", Style::default().fg(Color::Gray)),
                Span::raw(row.load_state),
            ]),
            Line::from(vec![
                Span::styled("State: ", Style::default().fg(Color::Gray)),
                Span::raw(format!("{} ({})", row.active_state, row.sub_state)),
                Span::raw("  •  "),
                Span::styled("Restarts ", Style::default().fg(Color::Gray)),
                Span::raw(row.restarts.to_string()),
            ]),
            Line::from(vec![
                Span::styled("When: ", Style::default().fg(Color::Gray)),
                Span::raw(row.last_change),
            ]),
            Line::from(vec![
                Span::styled("Hint: ", Style::default().fg(Color::Gray)),
                Span::raw("press Enter or l to tail logs for this service"),
            ]),
        ]
    } else {
        let mut lines = vec![Line::from("No services match the current filter.")];
        if !app.service_search.is_empty() {
            lines.push(Line::from(format!("Search: /{}", app.service_search)));
            lines.push(Line::from("Press Backspace or Esc to clear the search."));
        } else {
            lines.push(Line::from(
                "Press / to search by service name or description.",
            ));
        }
        lines.push(Line::from("Press Tab to change the filter."));
        lines
    };
    frame.render_widget(
        Paragraph::new(detail_lines)
            .block(Block::default().borders(Borders::ALL).title("Detail"))
            .wrap(Wrap { trim: true }),
        chunks[2],
    );
}
