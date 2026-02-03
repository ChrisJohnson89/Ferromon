use std::io::{self, Stdout};
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::ExecutableCommand;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Alignment, Constraint, Layout};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Terminal;

fn main() -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    stdout.execute(EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();
    let result = run_app(&mut terminal, &mut app);

    disable_raw_mode()?;
    terminal.backend_mut().execute(LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

struct App {
    started_at: Instant,
}

impl App {
    fn new() -> Self {
        Self {
            started_at: Instant::now(),
        }
    }
}

fn run_app(terminal: &mut Terminal<CrosstermBackend<Stdout>>, app: &mut App) -> io::Result<()> {
    loop {
        terminal.draw(|frame| {
            let layout = Layout::vertical([Constraint::Percentage(100)]).split(frame.size());
            let elapsed = app.started_at.elapsed().as_secs();
            let title = Line::from(vec![
                Span::styled("Ferromon", Style::default().fg(Color::Cyan)),
                Span::raw(" · TUI skeleton"),
            ]);

            let body = Paragraph::new(vec![
                Line::from("Welcome to Ferromon."),
                Line::from(format!("Uptime: {elapsed}s")),
                Line::from("Press q to quit."),
            ])
            .block(Block::default().borders(Borders::ALL).title(title))
            .alignment(Alignment::Left);

            frame.render_widget(body, layout[0]);
        })?;

        if event::poll(Duration::from_millis(250))? {
            if let Event::Key(key) = event::read()? {
                if key.code == KeyCode::Char('q') {
                    return Ok(());
                }
            }
        }
    }
}
