use std::time::{Duration, Instant};

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Sparkline, Table, Wrap};
use ratatui::prelude::Alignment;
use sysinfo::{Disks, System};

use crate::system::{
    collect_mount_rows, dash_target_path, disks_table_filtered, format_memory_pressure,
    format_top_processes, format_uptime, scan_dir_quick,
};
use crate::types::{AppState, ProcSort, VmSnapshot};
use crate::utils::{
    color_for_pct, format_bytes, format_rate, history_average, history_peak, trim_to,
};
use crate::ui::common::render_detail_panel;

pub fn render_dashboard(
    frame: &mut ratatui::Frame,
    area: Rect,
    vm: &VmSnapshot,
    app: &mut AppState,
    system: &System,
    disks: &Disks,
) {
    let panels = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(34),
            Constraint::Percentage(33),
            Constraint::Percentage(33),
        ])
        .split(area);

    // --- dashboard quick-overview cache ---
    // Keep this screen cheap: do tiny scans occasionally, not every frame.
    let now = Instant::now();
    let need_fs = match app.dash_last_fs_at {
        Some(t) => t.elapsed() >= Duration::from_secs(5),
        None => true,
    };

    if need_fs {
        app.dash_top_cpu = format_top_processes(system, ProcSort::Cpu, 20);
        app.dash_top_mem = format_top_processes(system, ProcSort::Mem, 20);
        app.dash_mem_pressure = format_memory_pressure(system, 5);
        app.dash_mount_rows = collect_mount_rows(12, app.dash_show_all_mounts)
            .unwrap_or_else(|| disks_table_filtered(disks, 12, app.dash_show_all_mounts));
        let (label, path) = dash_target_path(app.dash_dir_target);
        app.dash_dir_sizes = scan_dir_quick(&path, 6);
        if !app.dash_dir_sizes.is_empty() {
            app.dash_dir_sizes.insert(0, label);
        } else {
            app.dash_dir_sizes = vec![label, "(no entries)".to_string()];
        }
        app.dash_last_fs_at = Some(now);
    }

    // CPU
    let cpu_block = Block::default()
        .title("CPU")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    frame.render_widget(cpu_block.clone(), panels[0]);

    let cpu_inner = cpu_block.inner(panels[0]);
    let cpu_sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(48), Constraint::Percentage(52)])
        .split(cpu_inner);

    let cpu_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(0)])
        .split(cpu_sections[0]);

    let cpu_pct_color = color_for_pct(vm.cpu_usage as f64);
    let cpu_lines = vec![
        Line::from(vec![
            Span::styled("CPU ", Style::default().fg(Color::Gray)),
            Span::styled(
                format!("{:.1}%", vm.cpu_usage),
                Style::default().fg(cpu_pct_color),
            ),
            Span::styled("  Load ", Style::default().fg(Color::Gray)),
            Span::styled(
                format!("{:.2}  {:.2}  {:.2}", vm.load_avg_one, vm.load_avg_five, vm.load_avg_fifteen),
                Style::default().fg(Color::White),
            ),
        ]),
        Line::from(vec![
            Span::styled("Cores ", Style::default().fg(Color::Gray)),
            Span::styled(
                format!("{}", vm.cpu_cores),
                Style::default().fg(Color::White),
            ),
            Span::styled("  Up ", Style::default().fg(Color::Gray)),
            Span::styled(
                format_uptime(vm.uptime_secs),
                Style::default().fg(Color::White),
            ),
        ]),
    ];
    let cpu_paragraph = Paragraph::new(cpu_lines).alignment(Alignment::Left);
    frame.render_widget(cpu_paragraph, cpu_chunks[0]);

    // blank(1) + header(1) = 2 overhead rows
    let cpu_top_show = (cpu_chunks[1].height as usize).saturating_sub(2);
    let cpu_bottom = if app.dash_top_cpu.is_empty() {
        vec![
            Line::from(""),
            Line::from(Span::styled(
                "Top CPU: (no data)",
                Style::default().fg(Color::Gray),
            )),
        ]
    } else {
        let mut lines = vec![
            Line::from(""),
            Line::from(vec![
                Span::styled(
                    "Top CPU",
                    Style::default()
                        .fg(Color::Gray)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(": "),
            ]),
        ];
        for (i, row) in app.dash_top_cpu.iter().take(cpu_top_show).enumerate() {
            lines.push(Line::from(vec![
                Span::styled(format!("{}. ", i + 1), Style::default().fg(Color::Gray)),
                Span::raw(row.clone()),
            ]));
        }
        lines
    };
    frame.render_widget(
        Paragraph::new(cpu_bottom).alignment(Alignment::Left),
        cpu_chunks[1],
    );

    {
        let block = Block::default().borders(Borders::ALL).title("Signals");
        frame.render_widget(block.clone(), cpu_sections[1]);
        let inner = block.inner(cpu_sections[1]);
        if inner.width > 0 && inner.height > 0 {
            let stat_strings = vec![
                format!("Now {:.1}%", vm.cpu_usage),
                format!("Peak {}%", history_peak(&app.dash_cpu_history)),
                format!("Recent avg {:.1}%", history_average(&app.dash_cpu_history)),
                format!("Headroom {:.1}%", (100.0 - vm.cpu_usage as f64).max(0.0)),
            ];
            let cpus = system.cpus();
            let mut core_lines: Vec<Line> = Vec::new();
            if !cpus.is_empty() {
                let bar_width = 16usize;
                core_lines.push(Line::from(""));
                core_lines.push(Line::from(Span::styled(
                    "Per-core",
                    Style::default().fg(Color::Gray).add_modifier(Modifier::BOLD),
                )));
                for (idx, cpu) in cpus.iter().enumerate().take(24) {
                    let pct = cpu.cpu_usage() as f64;
                    let filled = ((pct / 100.0) * bar_width as f64).round() as usize;
                    let empty = bar_width.saturating_sub(filled);
                    let bar = format!("{}{}", "█".repeat(filled), "░".repeat(empty));
                    let color = color_for_pct(pct);
                    core_lines.push(Line::from(vec![
                        Span::styled(format!("c{:<2} ", idx), Style::default().fg(Color::Gray)),
                        Span::styled(bar, Style::default().fg(color)),
                        Span::styled(format!(" {:>3.0}%", pct), Style::default().fg(color)),
                    ]));
                }
            }
            let stats_h = stat_strings.len() as u16;
            let cores_h = core_lines.len() as u16;
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(stats_h),
                    Constraint::Length(cores_h),
                    Constraint::Min(1),
                ])
                .split(inner);
            frame.render_widget(
                Paragraph::new(
                    stat_strings.into_iter().map(Line::from).collect::<Vec<_>>(),
                )
                .style(Style::default().fg(Color::Gray)),
                chunks[0],
            );
            if !core_lines.is_empty() {
                frame.render_widget(Paragraph::new(core_lines), chunks[1]);
            }
            let spark_data: Vec<u64> =
                app.dash_cpu_history.iter().map(|s| *s as u64).collect();
            if !spark_data.is_empty() && chunks[2].width > 0 && chunks[2].height > 0 {
                let spark_color = app
                    .dash_cpu_history
                    .back()
                    .map(|s| color_for_pct(*s as f64))
                    .unwrap_or(cpu_pct_color);
                let spark_width = chunks[2].width.min(spark_data.len() as u16);
                let spark_x =
                    chunks[2].x + (chunks[2].width.saturating_sub(spark_width)) / 2;
                let spark_area = Rect {
                    x: spark_x,
                    y: chunks[2].y,
                    width: spark_width,
                    height: chunks[2].height,
                };
                frame.render_widget(
                    Sparkline::default()
                        .data(&spark_data)
                        .max(100)
                        .style(Style::default().fg(spark_color)),
                    spark_area,
                );
            }
        }
    }

    // Memory
    let memory_block = Block::default()
        .title("Memory")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Magenta));
    frame.render_widget(memory_block.clone(), panels[1]);

    let memory_inner = memory_block.inner(panels[1]);
    let memory_sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(48), Constraint::Percentage(52)])
        .split(memory_inner);

    let memory_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(0)])
        .split(memory_sections[0]);

    let mem_pct_color = color_for_pct(vm.memory_percent);
    let swap_str = if vm.total_swap > 0 {
        format!(
            "{} / {}",
            format_bytes(vm.used_swap),
            format_bytes(vm.total_swap)
        )
    } else {
        "off".to_string()
    };
    let memory_lines = vec![
        Line::from(vec![
            Span::styled("Mem  ", Style::default().fg(Color::Gray)),
            Span::styled(
                format!(
                    "{} / {}",
                    format_bytes(vm.used_memory),
                    format_bytes(vm.total_memory)
                ),
                Style::default().fg(mem_pct_color),
            ),
            Span::raw("  "),
            Span::styled(
                format!("{:.1}%", vm.memory_percent),
                Style::default().fg(mem_pct_color),
            ),
        ]),
        Line::from(vec![
            Span::styled("Avail", Style::default().fg(Color::Gray)),
            Span::raw("  "),
            Span::styled(
                format_bytes(vm.available_memory),
                Style::default().fg(Color::White),
            ),
            Span::raw("   "),
            Span::styled("Swap ", Style::default().fg(Color::Gray)),
            Span::styled(swap_str, Style::default().fg(Color::White)),
        ]),
    ];
    frame.render_widget(
        Paragraph::new(memory_lines).alignment(Alignment::Left),
        memory_chunks[0],
    );

    // blank(1) + header(1) = 2 overhead rows
    let mem_top_show = (memory_chunks[1].height as usize).saturating_sub(2);
    let mem_list = if app.dash_top_mem.is_empty() {
        vec![
            Line::from(""),
            Line::from(Span::styled(
                "Top MEM: (no data)",
                Style::default().fg(Color::Gray),
            )),
        ]
    } else {
        let mut lines = vec![
            Line::from(""),
            Line::from(vec![
                Span::styled(
                    "Top MEM",
                    Style::default()
                        .fg(Color::Gray)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(": "),
            ]),
        ];
        for (i, row) in app.dash_top_mem.iter().take(mem_top_show).enumerate() {
            lines.push(Line::from(vec![
                Span::styled(format!("{}. ", i + 1), Style::default().fg(Color::Gray)),
                Span::raw(row.clone()),
            ]));
        }
        lines
    };
    frame.render_widget(
        Paragraph::new(mem_list).alignment(Alignment::Left),
        memory_chunks[1],
    );

    let mut memory_signals = vec![
        format!("Now {:.1}%", vm.memory_percent),
        format!("Peak {}%", history_peak(&app.dash_mem_history)),
        format!("Recent avg {:.1}%", history_average(&app.dash_mem_history)),
        format!("Headroom {}", format_bytes(vm.available_memory)),
    ];
    memory_signals.push(if vm.total_swap > 0 {
        format!(
            "Swap {:.0}% ({}/{})",
            crate::utils::percent(vm.used_swap, vm.total_swap),
            format_bytes(vm.used_swap),
            format_bytes(vm.total_swap)
        )
    } else {
        "Swap off".to_string()
    });
    render_detail_panel(
        frame,
        memory_sections[1],
        "Signals",
        memory_signals,
        &app.dash_mem_history,
        mem_pct_color,
    );

    // Disk
    let disk_block = Block::default()
        .title("Disk")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Green));
    frame.render_widget(disk_block.clone(), panels[2]);

    let disk_inner = disk_block.inner(panels[2]);
    let disk_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(48), Constraint::Percentage(52)])
        .split(disk_inner);

    let mounts_title = if app.dash_show_all_mounts {
        "Mounts (all)"
    } else {
        "Mounts (filtered)"
    };
    let df_rows = app.dash_mount_rows.iter().map(|r| {
        Row::new(vec![
            Cell::from(trim_to(&r.mount, 12)),
            Cell::from(Span::styled(
                format!("{:.0}%", r.use_pct),
                Style::default().fg(color_for_pct(r.use_pct)),
            )),
            Cell::from(format!("{}/{}", format_bytes(r.used), format_bytes(r.size))),
            Cell::from(Span::styled(
                format_rate(r.read_bps),
                Style::default().fg(Color::Cyan),
            )),
            Cell::from(Span::styled(
                format_rate(r.write_bps),
                Style::default().fg(Color::Magenta),
            )),
            Cell::from(trim_to(&r.fs, 10)),
        ])
    });

    let df = Table::new(
        df_rows,
        [
            Constraint::Length(12),
            Constraint::Length(5),
            Constraint::Length(16),
            Constraint::Length(7),
            Constraint::Length(7),
            Constraint::Min(8),
        ],
    )
    .header(
        Row::new(vec!["MOUNT", "USE", "USED/TOTAL", "R/s", "W/s", "FS"]).style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
    )
    .block(Block::default().borders(Borders::ALL).title(mounts_title));

    frame.render_widget(df, disk_chunks[0]);

    let dir_title = app.dash_dir_target.title();
    let mut dir_lines: Vec<Line> = Vec::new();
    if let Some((path, entries)) = app.dash_dir_sizes.split_first() {
        dir_lines.push(Line::from(vec![
            Span::styled("Path: ", Style::default().fg(Color::Gray)),
            Span::styled(path.clone(), Style::default().fg(Color::White)),
        ]));
        for row in entries {
            dir_lines.push(Line::from(Span::raw(row.clone())));
        }
    } else {
        dir_lines.push(Line::from(Span::styled(
            "Path: unavailable",
            Style::default().fg(Color::Gray),
        )));
        dir_lines.push(Line::from(Span::styled(
            "(no entries)",
            Style::default().fg(Color::Gray),
        )));
    }

    frame.render_widget(
        Paragraph::new(dir_lines)
            .block(Block::default().borders(Borders::ALL).title(dir_title))
            .alignment(Alignment::Left)
            .wrap(Wrap { trim: false }),
        disk_chunks[1],
    );
}

