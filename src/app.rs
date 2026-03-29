use std::cmp::Reverse;
use std::io;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::Terminal;
use sysinfo::{Disks, System};

use crate::disk::{enter_selected_disk_dir, navigate_disk_up, start_disk_scan};
use crate::services::{
    handle_proc_search_key, handle_service_search_key, open_logs_for_selected_service,
    refresh_logs, refresh_services,
};
use crate::system::{format_snapshot, refresh, snapshot, update_disk_io_rates};
use crate::types::{AppState, DiskTarget, LogSeverity, LogUnitFilter, ProcRow, ProcSort, Screen, ServiceFilter};
use crate::ui::{
    render_dashboard, render_disk_dive, render_footer, render_header, render_help, render_logs,
    render_processes, render_services, render_too_small,
};
use crate::update::perform_self_update;
use crate::utils::push_history_sample;

pub fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    system: &mut System,
    disks: &mut Disks,
    app: &mut AppState,
    tick_rate: Duration,
    last_tick: &mut Instant,
) -> io::Result<Option<String>> {
    // Keep the dashboard cheap: refresh processes + fs scan on a slower cadence.
    let dash_proc_every = Duration::from_secs(3);
    let dash_history_every = tick_rate.max(Duration::from_millis(500));
    let mut tip_clock = Instant::now();

    loop {
        // Refresh data (keep it cheap; process refresh only when on the processes screen)
        if last_tick.elapsed() >= tick_rate {
            let refresh_processes = if matches!(app.screen, Screen::Processes) {
                true
            } else if matches!(app.screen, Screen::Dashboard) {
                // Only refresh process table occasionally; we just need top-N.
                match app.dash_last_proc_at {
                    Some(t) => t.elapsed() >= dash_proc_every,
                    None => true,
                }
            } else {
                false
            };
            refresh(system, disks, refresh_processes);
            if matches!(app.screen, Screen::Dashboard) && refresh_processes {
                // reuse this timestamp for both proc+fs scan cadence
                app.dash_last_proc_at = Some(Instant::now());
            }
            // Update disk I/O rates every tick — cheap, just reads sysinfo cached values.
            let io_elapsed = app
                .dash_last_io_at
                .map(|t| t.elapsed().as_secs_f64())
                .unwrap_or_else(|| tick_rate.as_secs_f64());
            app.dash_last_io_at = Some(Instant::now());
            if !app.dash_mount_rows.is_empty() {
                update_disk_io_rates(
                    &mut app.dash_mount_rows,
                    &mut app.dash_diskstats_prev,
                    io_elapsed,
                );
            }
            if matches!(app.screen, Screen::Services) {
                refresh_services(app, false);
            }
            if matches!(app.screen, Screen::Logs) {
                refresh_logs(app, false);
            }
            *last_tick = Instant::now();

            if tip_clock.elapsed() >= Duration::from_secs(12) {
                app.footer_tip_idx = app.footer_tip_idx.wrapping_add(1);
                tip_clock = Instant::now();
            }
        }

        let vm = snapshot(system);
        if matches!(app.screen, Screen::Dashboard) {
            let due = app
                .dash_last_history_at
                .map(|t| t.elapsed() >= dash_history_every)
                .unwrap_or(true);
            if due {
                push_history_sample(&mut app.dash_cpu_history, vm.cpu_usage as f64, 48);
                push_history_sample(&mut app.dash_mem_history, vm.memory_percent, 48);
                app.dash_last_history_at = Some(Instant::now());
            }
        }

        terminal.draw(|frame| {
            let size = frame.size();
            let rows = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1),
                    Constraint::Min(8),
                    Constraint::Length(if app.show_help { 7 } else { 1 }),
                ])
                .margin(1)
                .split(size);

            // Header
            frame.render_widget(render_header(app), rows[0]);

            // If terminal is too small, render a friendly message instead of a broken layout.
            if rows[1].width < 80 || rows[1].height < 14 {
                render_too_small(frame, rows[1]);
                // Footer/help still renders below.
                return;
            }

            // Main
            match app.screen {
                Screen::Dashboard => render_dashboard(frame, rows[1], &vm, app, system, disks),
                Screen::Processes => render_processes(frame, rows[1], app, system),
                Screen::DiskDive => render_disk_dive(frame, rows[1], app),
                Screen::Services => render_services(frame, rows[1], app),
                Screen::Logs => render_logs(frame, rows[1], app),
            }

            // Footer/help
            if app.show_help {
                frame.render_widget(render_help(app), rows[2]);
            } else {
                frame.render_widget(render_footer(app), rows[2]);
            }
        })?;

        if app.dump_snapshot {
            app.dump_snapshot = false;
            return Ok(Some(format_snapshot(&vm, app, system, disks)));
        }

        if app.do_update {
            app.do_update = false;
            let latest = app.update.latest_tag.clone().unwrap_or_default();
            if latest.is_empty() {
                return Ok(Some("No latest tag found".to_string()));
            }
            return Ok(Some(match perform_self_update(&latest) {
                Ok(msg) => msg,
                Err(e) => format!("Update failed: {e}"),
            }));
        }

        // Input
        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                // Avoid key-repeat spam on some terminals
                if key.kind != KeyEventKind::Press {
                    continue;
                }

                // Kill confirm mode intercepts all keys.
                if app.proc_kill_confirm.is_some() {
                    let pid = app.proc_kill_confirm.as_ref().map(|(p, _)| *p).unwrap();
                    match key.code {
                        KeyCode::Char('y') => {
                            if let Some(proc) = system.process(sysinfo::Pid::from_u32(pid as u32)) {
                                let _ = proc.kill_with(sysinfo::Signal::Term);
                            }
                            app.proc_kill_confirm = None;
                        }
                        KeyCode::Char('K') => {
                            if let Some(proc) = system.process(sysinfo::Pid::from_u32(pid as u32)) {
                                proc.kill();
                            }
                            app.proc_kill_confirm = None;
                        }
                        KeyCode::Char('n') | KeyCode::Esc => {
                            app.proc_kill_confirm = None;
                        }
                        _ => {}
                    }
                    continue;
                }

                if handle_proc_search_key(app, &key) {
                    continue;
                }

                if handle_service_search_key(app, &key) {
                    continue;
                }

                match key.code {
                    KeyCode::Char('q') => return Ok(None),
                    KeyCode::Char('?') => app.show_help = !app.show_help,
                    KeyCode::Esc => {
                        app.show_help = false;
                        app.screen = Screen::Dashboard;
                    }
                    KeyCode::Char('p') => {
                        app.show_help = false;
                        app.screen = Screen::Processes;
                    }
                    KeyCode::Char('d') => {
                        app.show_help = false;
                        app.screen = Screen::DiskDive;
                    }
                    KeyCode::Char('v') => {
                        app.show_help = false;
                        app.screen = Screen::Services;
                        refresh_services(app, true);
                    }
                    KeyCode::Char('l') => {
                        app.show_help = false;
                        if matches!(app.screen, Screen::Services) {
                            open_logs_for_selected_service(app);
                        } else {
                            app.screen = Screen::Logs;
                            refresh_logs(app, true);
                        }
                    }
                    KeyCode::Char('r') => {
                        // manual refresh, including processes if currently viewing them
                        let refresh_processes = if matches!(app.screen, Screen::Processes) {
                            true
                        } else if matches!(app.screen, Screen::Dashboard) {
                            // Only refresh process table occasionally; we just need top-N.
                            match app.dash_last_proc_at {
                                Some(t) => t.elapsed() >= dash_proc_every,
                                None => true,
                            }
                        } else {
                            false
                        };
                        refresh(system, disks, refresh_processes);
                        if matches!(app.screen, Screen::Dashboard) && refresh_processes {
                            // reuse this timestamp for both proc+fs scan cadence
                            app.dash_last_proc_at = Some(Instant::now());
                        }
                        if matches!(app.screen, Screen::Services) {
                            refresh_services(app, true);
                        }
                        if matches!(app.screen, Screen::Logs) {
                            refresh_logs(app, true);
                        }
                        *last_tick = Instant::now();

                        if tip_clock.elapsed() >= Duration::from_secs(12) {
                            app.footer_tip_idx = app.footer_tip_idx.wrapping_add(1);
                            tip_clock = Instant::now();
                        }
                    }

                    // Processes + DiskDive share Tab for mode/target.
                    KeyCode::Up => {
                        if matches!(app.screen, Screen::Processes) {
                            app.proc_scroll = app.proc_scroll.saturating_sub(1);
                        } else if matches!(app.screen, Screen::DiskDive) {
                            app.disk_scroll = app.disk_scroll.saturating_sub(1);
                        } else if matches!(app.screen, Screen::Services) {
                            app.service_scroll = app.service_scroll.saturating_sub(1);
                        } else if matches!(app.screen, Screen::Logs) {
                            app.logs_scroll = app.logs_scroll.saturating_sub(1);
                        }
                    }
                    KeyCode::Down => {
                        if matches!(app.screen, Screen::Processes) {
                            app.proc_scroll = app.proc_scroll.saturating_add(1);
                        } else if matches!(app.screen, Screen::DiskDive) {
                            app.disk_scroll = app.disk_scroll.saturating_add(1);
                        } else if matches!(app.screen, Screen::Services) {
                            app.service_scroll = app.service_scroll.saturating_add(1);
                        } else if matches!(app.screen, Screen::Logs) {
                            app.logs_scroll = app.logs_scroll.saturating_add(1);
                        }
                    }

                    // Tab is contextual.
                    KeyCode::Tab => {
                        if matches!(app.screen, Screen::Dashboard) {
                            app.dash_dir_target = app.dash_dir_target.next();
                            // Force refresh of quick scan.
                            app.dash_last_fs_at = None;
                        } else if matches!(app.screen, Screen::DiskDive) {
                            app.disk_target = match app.disk_target {
                                DiskTarget::Var => DiskTarget::Home,
                                DiskTarget::Home => DiskTarget::Root,
                                DiskTarget::Root => DiskTarget::Var,
                            };
                            app.disk_scroll = 0;
                            let mut state = app.disk_scan.inner.lock().unwrap();
                            state.current_path = None;
                            state.results.clear();
                            state.error = None;
                            state.progress.clear();
                        } else if matches!(app.screen, Screen::Processes) {
                            app.proc_sort = match app.proc_sort {
                                ProcSort::Cpu => ProcSort::Mem,
                                ProcSort::Mem => ProcSort::Cpu,
                            };
                            app.proc_scroll = 0;
                        } else if matches!(app.screen, Screen::Services) {
                            app.service_filter = match app.service_filter {
                                ServiceFilter::Failed => ServiceFilter::Unhealthy,
                                ServiceFilter::Unhealthy => ServiceFilter::Active,
                                ServiceFilter::Active => ServiceFilter::All,
                                ServiceFilter::All => ServiceFilter::Failed,
                            };
                            app.service_scroll = 0;
                        } else if matches!(app.screen, Screen::Logs) {
                            app.log_severity = match app.log_severity {
                                LogSeverity::Errors => LogSeverity::Warnings,
                                LogSeverity::Warnings => LogSeverity::Info,
                                LogSeverity::Info => LogSeverity::Debug,
                                LogSeverity::Debug => LogSeverity::Errors,
                            };
                            app.logs_scroll = 0;
                            refresh_logs(app, true);
                        }
                    }
                    KeyCode::Char('k') => {
                        if matches!(app.screen, Screen::Processes) && !app.proc_search_active {
                            // Rebuild filtered list to get the PID at the cursor.
                            let mut procs: Vec<ProcRow> = system
                                .processes()
                                .iter()
                                .map(|(pid, p)| ProcRow::from_process(*pid, p))
                                .collect();
                            match app.proc_sort {
                                ProcSort::Cpu => procs.sort_by_key(|p| {
                                    Reverse((p.cpu_x10 as i64, p.mem_bytes as i64))
                                }),
                                ProcSort::Mem => procs.sort_by_key(|p| {
                                    Reverse((p.mem_bytes as i64, p.cpu_x10 as i64))
                                }),
                            }
                            if procs.len() > 200 {
                                procs.truncate(200);
                            }
                            let procs = crate::services::filtered_proc_rows(procs, &app.proc_search);
                            let idx = (app.proc_scroll as usize).min(procs.len().saturating_sub(1));
                            if let Some(row) = procs.get(idx) {
                                app.proc_kill_confirm = Some((row.pid, row.name.clone()));
                            }
                        }
                    }
                    KeyCode::Char('s') => {
                        if matches!(app.screen, Screen::DiskDive) {
                            start_disk_scan(app);
                        }
                    }
                    KeyCode::Enter => {
                        if matches!(app.screen, Screen::DiskDive) {
                            enter_selected_disk_dir(app);
                        } else if matches!(app.screen, Screen::Services) {
                            open_logs_for_selected_service(app);
                        }
                    }
                    KeyCode::Left | KeyCode::Backspace => {
                        if matches!(app.screen, Screen::DiskDive) {
                            navigate_disk_up(app);
                        }
                    }
                    KeyCode::Char('f') => {
                        if matches!(app.screen, Screen::Dashboard) {
                            app.dash_show_all_mounts = !app.dash_show_all_mounts;
                            app.dash_last_fs_at = None;
                        }
                    }
                    KeyCode::Char('x') => {
                        if matches!(app.screen, Screen::Dashboard) {
                            app.dump_snapshot = true;
                        }
                    }
                    KeyCode::Char('u') => {
                        if matches!(app.screen, Screen::Dashboard) && app.update.available {
                            app.do_update = true;
                        } else if matches!(app.screen, Screen::Logs) {
                            app.log_unit_filter = match app.log_unit_filter {
                                LogUnitFilter::Selected => LogUnitFilter::All,
                                LogUnitFilter::All => LogUnitFilter::Selected,
                            };
                            app.logs_scroll = 0;
                            refresh_logs(app, true);
                        }
                    }

                    _ => {}
                }
            }
        }
    }
}
