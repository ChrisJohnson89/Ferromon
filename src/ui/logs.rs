use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::services::{log_severity_label, log_unit_filter_label};
use crate::types::AppState;

pub fn render_logs(frame: &mut ratatui::Frame, area: Rect, app: &mut AppState) {
    let state = app.log_state.inner.lock().unwrap();

    if let Some(msg) = &state.unsupported {
        frame.render_widget(
            Paragraph::new(vec![
                Line::from("Logs"),
                Line::from(""),
                Line::from(msg.clone()),
                Line::from(""),
                Line::from("On Linux this uses journalctl first and falls back to syslog."),
            ])
            .block(Block::default().title("Logs").borders(Borders::ALL))
            .alignment(ratatui::prelude::Alignment::Center),
            area,
        );
        return;
    }

    let source = if state.source.is_empty() {
        "journalctl".to_string()
    } else {
        state.source.clone()
    };
    let lines = state.lines.clone();
    let err = state.error.clone();
    let running = state.running;
    drop(state);

    let block = Block::default()
        .title("Logs")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Magenta));
    frame.render_widget(block.clone(), area);
    let inner = block.inner(area);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(4)])
        .split(inner);

    let header = Paragraph::new(vec![Line::from(vec![
        Span::styled("Severity ", Style::default().fg(Color::Gray)),
        Span::raw(log_severity_label(app.log_severity)),
        Span::raw("  •  "),
        Span::styled("Unit ", Style::default().fg(Color::Gray)),
        Span::raw(log_unit_filter_label(
            app.log_unit_filter,
            app.log_selected_unit.as_deref(),
        )),
        Span::raw("  •  "),
        Span::styled("Source ", Style::default().fg(Color::Gray)),
        Span::raw(source),
        if running {
            Span::styled("  •  refreshing", Style::default().fg(Color::Yellow))
        } else {
            Span::raw("")
        },
    ])]);
    frame.render_widget(header, chunks[0]);

    let body_lines = if let Some(err) = err {
        vec![
            Line::from(vec![
                Span::styled("Error: ", Style::default().fg(Color::Red)),
                Span::raw(err),
            ]),
            Line::from("Try switching to all units with `u` or refreshing with `r`."),
        ]
    } else if lines.is_empty() {
        vec![
            Line::from("No log lines matched the current filters."),
            Line::from("Try `Tab` for severity or `u` for all units."),
        ]
    } else {
        lines.into_iter().map(Line::from).collect::<Vec<Line>>()
    };

    let scroll = app
        .logs_scroll
        .min((body_lines.len().saturating_sub(chunks[1].height as usize)) as u16);
    app.logs_scroll = scroll;
    frame.render_widget(
        Paragraph::new(body_lines)
            .block(Block::default().borders(Borders::ALL).title("Recent lines"))
            .scroll((scroll, 0))
            .wrap(Wrap { trim: false }),
        chunks[1],
    );
}
