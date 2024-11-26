use ratatui::crossterm::cursor::SetCursorStyle;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::crossterm::execute;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table};
use ratatui::{DefaultTerminal, Frame};
use std::fmt::Display;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio::sync::Mutex;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

mod ws;

struct App {
    url: String,
    input: String,
    /// Position of the cursor in the input string
    char_index: usize,
    result: QueryResult,
    latency: f32,
    input_mode: InputMode,
    // table_state: TableState,
    action_sender: mpsc::UnboundedSender<Action>,
    res_recv: mpsc::UnboundedReceiver<QueryResult>,
    latency_recv: mpsc::UnboundedReceiver<f32>,
}

impl App {
    pub fn run(mut self, mut terminal: DefaultTerminal) -> color_eyre::Result<()> {
        self.update_cursor_shape()?;

        let tick_rate = Duration::from_millis(250);
        let mut last_tick = Instant::now();
        loop {
            while let Ok(res) = self.res_recv.try_recv() {
                self.result = res;
            }
            while let Ok(latency) = self.latency_recv.try_recv() {
                self.latency = latency;
            }
            terminal.draw(|f| self.draw(f))?;

            let timeout = tick_rate.saturating_sub(last_tick.elapsed());
            if event::poll(timeout)? {
                if let Event::Key(key) = event::read()? {
                    match self.input_mode {
                        InputMode::Normal => match key.code {
                            KeyCode::Char('i') => {
                                self.input_mode = InputMode::Insert;
                                self.update_cursor_shape()?;
                            }
                            KeyCode::Char('a') => {
                                self.input_mode = InputMode::Insert;
                                self.update_cursor_shape()?;
                                if self.char_index < self.input.len() {
                                    self.char_index += 1;
                                }
                            }
                            KeyCode::Char('q') => {
                                return Ok(());
                            }
                            KeyCode::Char('0') => {
                                self.char_index = 0;
                            }
                            KeyCode::Char('$') => {
                                self.char_index = self.input.len() - 1;
                            }
                            KeyCode::Char('c') => self.clear_results(),
                            KeyCode::Char('r') => self.submit_query(),
                            KeyCode::Left | KeyCode::Char('h') => self.move_cursor_left(),
                            KeyCode::Right | KeyCode::Char('l') => self.move_cursor_right(),
                            KeyCode::Char('D') => self.delete_input(),
                            _ => {}
                        },
                        InputMode::Insert if key.kind == KeyEventKind::Press => match key.code {
                            KeyCode::Char(c) => self.add_char(c),
                            KeyCode::Left => self.move_cursor_left(),
                            KeyCode::Right => self.move_cursor_right(),
                            KeyCode::Backspace => self.delete_char(),
                            KeyCode::Esc => {
                                self.input_mode = InputMode::Normal;
                                self.update_cursor_shape()?;
                            }
                            _ => {}
                        },
                        InputMode::Insert => {}
                    }
                }
            }
            if last_tick.elapsed() >= tick_rate {
                last_tick = Instant::now();
            }
        }
    }

    fn draw(&self, f: &mut Frame) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints(
                [
                    Constraint::Length(3),  // Top box height
                    Constraint::Length(10), // Middle box height
                    Constraint::Min(0),     // Bottom box takes the rest
                ]
                .as_ref(),
            )
            .split(f.area());

        let top_container = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(32), Constraint::Min(0)].as_ref())
            .split(chunks[0]);

        // Top box
        let mode_span = Span::styled(
            self.input_mode.to_string(),
            Style::default().bold().bg(Color::Blue).fg(Color::Black),
        );
        let latency_span = Span::raw(format!(" | Latency: {}ms", self.latency));
        let misc_line = Line::from(vec![mode_span, latency_span]);
        let misc_block =
            Paragraph::new(misc_line).block(Block::default().borders(Borders::ALL).title("Misc"));
        f.render_widget(misc_block, top_container[0]);

        let url_block = Paragraph::new(format!("Connected to: {}", self.url))
            .block(Block::default().borders(Borders::ALL).title("Database URL"));
        f.render_widget(url_block, top_container[1]);

        // Middle box
        let query_block = Paragraph::new(self.input.to_string())
            .block(Block::default().borders(Borders::ALL).title("SQL Query"));
        f.render_widget(query_block, chunks[1]);

        {
            let input_area = chunks[1];
            let input_width = input_area.width - 2;
            let input_lines = wrap_text(&self.input, input_width);
            let (cursor_x, cursor_y) = calculate_cursor_position(&input_lines, self.char_index);

            // Set the cursor position
            f.set_cursor_position((input_area.x + cursor_x + 1, input_area.y + cursor_y + 1));
        }

        // Bottom box: Results
        let results_block = match &self.result {
            QueryResult::None => Paragraph::new("No results")
                .block(Block::default().borders(Borders::ALL).title("Results")),
            QueryResult::Table { columns, rows } => {
                let header_cells = columns
                    .iter()
                    .map(|h| Cell::from(Text::from(h.to_string())));
                let header = Row::new(header_cells).style(
                    ratatui::style::Style::default()
                        .fg(ratatui::style::Color::Yellow)
                        .bg(ratatui::style::Color::Black),
                );

                let rows = rows.iter().map(|item| {
                    let cells = item.iter().map(|c| Cell::from(Text::from(c.to_string())));
                    Row::new(cells)
                });

                let widths = [Constraint::Length(5), Constraint::Length(5)];
                let table = Table::new(rows, widths)
                    .header(header)
                    .block(Block::default().borders(Borders::ALL).title("Results"))
                    .widths(
                        columns
                            .iter()
                            .map(|_| Constraint::Min(10))
                            .collect::<Vec<_>>(),
                    );
                f.render_widget(table, chunks[2]);
                return;
            }
            QueryResult::Error(err) => {
                Paragraph::new(Text::from(err.to_string()).style(Style::default().fg(Color::Red)))
                    .block(Block::default().borders(Borders::ALL).title("Error"))
            }
        };
        f.render_widget(results_block, chunks[2]);
    }
    fn submit_query(&mut self) {
        if self.input.is_empty() {
            return;
        }

        let _ = self.action_sender.send(Action::Query(self.input.clone()));
    }

    fn update_cursor_shape(&self) -> color_eyre::Result<()> {
        let cursor = match self.input_mode {
            InputMode::Normal => SetCursorStyle::SteadyBlock,
            InputMode::Insert => SetCursorStyle::SteadyBar,
        };
        execute!(std::io::stdout(), cursor)?;

        Ok(())
    }

    fn clear_results(&mut self) {
        self.result = QueryResult::None;
    }

    fn delete_input(&mut self) {
        self.input.clear();
        self.char_index = 0;
    }

    fn add_char(&mut self, c: char) {
        self.input.insert(self.char_index, c);
        self.char_index += 1;
    }

    fn delete_char(&mut self) {
        if self.char_index > 0 {
            self.input.remove(self.char_index - 1);
            self.char_index -= 1;
        }
    }

    fn move_cursor_left(&mut self) {
        if self.char_index > 0 {
            self.char_index -= 1;
        }
    }
    fn move_cursor_right(&mut self) {
        if self.char_index < self.input.len() - 1 {
            self.char_index += 1;
        }
    }
}

#[derive(Default, PartialEq, Eq)]
enum InputMode {
    #[default]
    Normal,
    Insert,
}

impl Display for InputMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InputMode::Normal => write!(f, "Normal"),
            InputMode::Insert => write!(f, "Insert"),
        }
    }
}
#[derive(Default)]
enum QueryResult {
    #[default]
    None,
    Table {
        columns: Vec<ws::Column>,
        rows: Vec<Vec<ws::LibSqlValue>>,
    },
    Error(String),
}

#[derive(Debug, PartialEq, Eq)]
enum Action {
    Query(String),
}

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    dotenv::dotenv().ok();
    color_eyre::install()?;

    let token = std::env::var("LIBSQL_TOKEN").expect("LIBSQL_TOKEN not set");
    let url = "wss://todos-lpturmel.turso.io";

    let mut client = ws::LibSqlClient::connect(url, &token).await?;

    client.open_stream(1).await?;

    let client = Arc::new(Mutex::new(client));

    let (action_tx, mut action_rx) = mpsc::unbounded_channel::<Action>();
    let (result_tx, result_rx) = mpsc::unbounded_channel::<QueryResult>();
    let (latency_tx, latency_rx) = mpsc::unbounded_channel::<f32>();

    let app = App {
        char_index: 0,
        url: url.to_string(),
        input: String::new(),
        result: QueryResult::default(),
        input_mode: InputMode::default(),
        // table_state: TableState::default(),
        latency: 0.0,
        action_sender: action_tx,
        res_recv: result_rx,
        latency_recv: latency_rx,
    };
    let terminal = ratatui::init();

    let client_c = client.clone();
    tokio::spawn(async move {
        loop {
            if let Ok(latency) = client_c.lock().await.send_ping().await {
                let _ = latency_tx.send(latency);
            }
            tokio::time::sleep(Duration::from_millis(1000)).await;
        }
    });
    let client = client.clone();
    tokio::spawn(async move {
        while let Some(action) = action_rx.recv().await {
            match action {
                Action::Query(query) => {
                    let mut client = client.lock().await;
                    let result = client.execute_statement(1, &query).await;
                    let res = match result {
                        Ok(res) => QueryResult::Table {
                            columns: res.cols,
                            rows: res.rows,
                        },
                        Err(err) => QueryResult::Error(err.to_string()),
                    };
                    let _ = result_tx.send(res);
                }
            }
        }
    });

    let app_result = app.run(terminal);

    ratatui::restore();

    app_result
}

fn wrap_text(text: &str, max_width: u16) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current_line = String::new();

    for c in text.chars() {
        let cw = c.width().unwrap_or(0) as u16;
        let line_width = UnicodeWidthStr::width(current_line.as_str()) as u16;

        if line_width + cw > max_width {
            lines.push(current_line);
            current_line = String::new();
        }
        current_line.push(c);
    }

    lines.push(current_line);
    lines
}

fn calculate_cursor_position(lines: &[String], char_index: usize) -> (u16, u16) {
    let mut chars_remaining = char_index;
    for (y, line) in lines.iter().enumerate() {
        let line_length = line.chars().count();
        if chars_remaining <= line_length {
            let x = UnicodeWidthStr::width(&line[0..chars_remaining]) as u16;
            return (x, y as u16);
        } else {
            chars_remaining -= line_length;
        }
    }
    let last_line = match lines.last() {
        Some(line) => line,
        None => "",
    };
    let x = UnicodeWidthStr::width(last_line) as u16;
    let y = (lines.len() - 1) as u16;
    (x, y)
}
