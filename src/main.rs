use self::config::select_database;
use ratatui::{
    crossterm::{
        cursor::SetCursorStyle,
        event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
        execute,
    },
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style, Stylize},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, Tabs, Wrap},
    DefaultTerminal, Frame,
};
use std::{
    fmt::Display,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::{mpsc, Mutex};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

mod config;
mod ws;

struct Tab {
    name: String,
    input: String,
    /// Position of the cursor in the input string
    char_index: usize,
    query_result: QueryResult,
}

impl Tab {
    fn new(name: String) -> Self {
        Self {
            name,
            input: String::new(),
            char_index: 0,
            query_result: QueryResult::default(),
        }
    }
}

struct App {
    url: String,
    latency: f32,
    input_mode: InputMode,
    // table_state: TableState,
    action_sender: mpsc::UnboundedSender<Action>,
    res_recv: mpsc::UnboundedReceiver<QueryResult>,
    latency_recv: mpsc::UnboundedReceiver<f32>,
    tabs: Vec<Tab>,
    selected_tab: usize,
}

impl App {
    pub fn run(mut self, mut terminal: DefaultTerminal) -> anyhow::Result<()> {
        self.update_cursor_shape()?;

        let tick_rate = Duration::from_millis(250);
        let mut last_tick = Instant::now();

        loop {
            while let Ok(res) = self.res_recv.try_recv() {
                let selected_tab = &mut self.tabs[self.selected_tab];
                selected_tab.query_result = res;
            }
            while let Ok(latency) = self.latency_recv.try_recv() {
                self.latency = latency;
            }
            terminal.draw(|f| self.draw(f))?;

            let timeout = tick_rate.saturating_sub(last_tick.elapsed());
            if event::poll(timeout)? {
                if let Event::Key(key) = event::read()? {
                    match self.input_mode {
                        InputMode::Normal => match (key.modifiers, key.code) {
                            (KeyModifiers::CONTROL, KeyCode::Char('n')) => self.new_tab(),
                            (KeyModifiers::CONTROL, KeyCode::Char('w')) => self.delete_tab(),
                            (KeyModifiers::CONTROL, KeyCode::Char('r')) => self.submit_query(),
                            (KeyModifiers::CONTROL, KeyCode::Char('t')) => self.get_tables(),
                            (_, KeyCode::Char('H')) => self.previous_tab(),
                            (_, KeyCode::Char('L')) => self.next_tab(),
                            (_, KeyCode::Char('i')) => {
                                self.input_mode = InputMode::Insert;
                                self.update_cursor_shape()?;
                            }
                            (_, KeyCode::Char('A')) => {
                                self.input_mode = InputMode::Insert;
                                self.update_cursor_shape()?;

                                let selected_tab = &mut self.tabs[self.selected_tab];
                                if selected_tab.char_index < selected_tab.input.len() {
                                    selected_tab.char_index = selected_tab.input.len();
                                }
                            }
                            (_, KeyCode::Char('b')) => self.move_last(),
                            (_, KeyCode::Char('w')) => self.move_next(),
                            (_, KeyCode::Char('x')) => self.delete_next_char(),
                            (_, KeyCode::Char('a')) => {
                                self.input_mode = InputMode::Insert;
                                self.update_cursor_shape()?;
                                let selected_tab = &mut self.tabs[self.selected_tab];
                                if selected_tab.char_index < selected_tab.input.len() {
                                    selected_tab.char_index += 1;
                                }
                            }
                            (_, KeyCode::Char('q')) => {
                                return Ok(());
                            }
                            (_, KeyCode::Char('0')) => {
                                let selected_tab = &mut self.tabs[self.selected_tab];
                                selected_tab.char_index = 0;
                            }
                            (_, KeyCode::Char('$')) => {
                                let selected_tab = &mut self.tabs[self.selected_tab];
                                selected_tab.char_index = selected_tab.input.len() - 1;
                            }
                            (_, KeyCode::Char('c')) => self.clear_results(),
                            (_, KeyCode::Left | KeyCode::Char('h')) => self.move_cursor_left(),
                            (_, KeyCode::Right | KeyCode::Char('l')) => self.move_cursor_right(),
                            (_, KeyCode::Char('D')) => self.delete_input(),
                            _ => {}
                        },
                        InputMode::Insert if key.kind == KeyEventKind::Press => match key.code {
                            KeyCode::Char(c) => self.append_char(c),
                            KeyCode::Left => self.move_cursor_left(),
                            KeyCode::Right => self.move_cursor_right(),
                            KeyCode::Backspace => self.delete_last_char(),
                            KeyCode::Enter => {
                                self.append_char('\n');
                            }
                            KeyCode::Esc => {
                                self.input_mode = InputMode::Normal;
                                self.update_cursor_shape()?;

                                let selected_tab = &mut self.tabs[self.selected_tab];
                                if selected_tab.char_index > 0 {
                                    selected_tab.char_index -= 1;
                                }
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

    fn is_word_char(c: char) -> bool {
        c.is_alphanumeric() || c == '_'
    }

    fn move_next(&mut self) {
        let selected_tab = &mut self.tabs[self.selected_tab];
        let input = &selected_tab.input;
        let input_len = input.len();

        if selected_tab.char_index >= input_len {
            return;
        }

        let chars: Vec<char> = input.chars().collect();
        let mut idx = selected_tab.char_index;

        while idx < chars.len() && chars[idx].is_whitespace() {
            idx += 1;
        }

        if idx >= chars.len() {
            selected_tab.char_index = idx;
            return;
        }

        if Self::is_word_char(chars[idx]) {
            while idx < chars.len() - 1 && Self::is_word_char(chars[idx]) {
                idx += 1;
            }
        } else {
            while idx < chars.len() - 1
                && !chars[idx].is_whitespace()
                && !Self::is_word_char(chars[idx])
            {
                idx += 1;
            }
        }

        while idx < chars.len() - 1 && chars[idx].is_whitespace() {
            idx += 1;
        }

        selected_tab.char_index = idx;
    }

    fn move_last(&mut self) {
        let selected_tab = &mut self.tabs[self.selected_tab];

        if selected_tab.char_index == 0 {
            return;
        }

        let chars: Vec<char> = selected_tab.input.chars().collect();
        let mut idx = selected_tab.char_index;

        idx = idx.saturating_sub(1);

        while idx > 0 && chars[idx].is_whitespace() {
            idx = idx.saturating_sub(1);
        }

        if idx == 0 && !chars[idx].is_whitespace() {
            selected_tab.char_index = idx;
            return;
        }

        if Self::is_word_char(chars[idx]) {
            while idx > 0 && Self::is_word_char(chars[idx]) {
                idx = idx.saturating_sub(1);
            }
            if !Self::is_word_char(chars[idx]) && idx < chars.len() - 1 {
                idx = idx.saturating_add(1);
            }
        } else {
            while idx > 0 && !chars[idx].is_whitespace() && !Self::is_word_char(chars[idx]) {
                idx = idx.saturating_sub(1);
            }
            if (chars[idx].is_whitespace() || Self::is_word_char(chars[idx]))
                && idx < chars.len() - 1
            {
                idx = idx.saturating_add(1);
            }
        }

        selected_tab.char_index = idx;
    }

    fn render_top_bar(&self, f: &mut Frame, chunks: Rect) {
        let top_container = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(32), Constraint::Min(0)].as_ref())
            .split(chunks);

        // Top box
        let mode_span = Span::styled(
            self.input_mode.to_string(),
            Style::default().bold().bg(Color::Blue).fg(Color::Black),
        );
        let latency_span = Span::raw(format!(" | Latency: {}ms", self.latency));
        let misc_line = Line::from(vec![mode_span, latency_span]);
        let misc_block =
            Paragraph::new(misc_line).block(Block::default().borders(Borders::ALL).title(" Misc "));
        f.render_widget(misc_block, top_container[0]);

        let url_block = Paragraph::new(format!("Connected to: {}", self.url))
            .block(Block::default().borders(Borders::ALL).title(" Database "));
        f.render_widget(url_block, top_container[1]);
    }

    fn render_tabs(&self, f: &mut Frame, chunks: Rect) {
        let titles = self
            .tabs
            .iter()
            .map(|t| format!(" {} ", t.name).bg(Color::Black));

        let hl_style = Style::default().bg(Color::White).fg(Color::Black);
        let tabs = Tabs::new(titles)
            .highlight_style(hl_style)
            .select(self.selected_tab)
            .padding("", "")
            .divider(" ");
        f.render_widget(tabs, chunks);
    }

    fn render_query(&self, f: &mut Frame, chunks: Rect) {
        let selected_tab = &self.tabs[self.selected_tab];

        let query_block = Paragraph::new(selected_tab.input.to_string())
            .block(Block::default().borders(Borders::ALL).title(" SQL "))
            .wrap(Wrap { trim: false });
        f.render_widget(query_block, chunks);

        {
            let input_width = chunks.width - 2;
            let input_lines = wrap_text(&selected_tab.input, input_width);
            let (cursor_x, cursor_y) =
                calculate_cursor_position(&input_lines, selected_tab.char_index);

            f.set_cursor_position((chunks.x + cursor_x + 1, chunks.y + cursor_y + 1));
        }
    }
    fn render_results(&self, f: &mut Frame, chunks: Rect) {
        let selected_tab = &self.tabs[self.selected_tab];
        let results_block = match &selected_tab.query_result {
            QueryResult::None => Paragraph::new(" No results")
                .block(Block::default().borders(Borders::ALL).title(" Results ")),
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
                f.render_widget(table, chunks);
                return;
            }
            QueryResult::Error(err) => {
                Paragraph::new(Text::from(err.to_string()).style(Style::default().fg(Color::Red)))
                    .block(Block::default().borders(Borders::ALL).title("Error"))
            }
        };
        f.render_widget(results_block, chunks);
    }
    fn draw(&self, f: &mut Frame) {
        let main_layout = Layout::vertical([
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Length(10),
            Constraint::Min(0),
        ]);

        let [top_area, tabs_area, query_area, results_area] = main_layout.areas(f.area());

        self.render_tabs(f, tabs_area);

        self.render_top_bar(f, top_area);

        self.render_query(f, query_area);

        self.render_results(f, results_area);
    }

    fn get_tables(&self) {
        let _ = self.action_sender.send(Action::Query(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'"
                .to_string(),
        ));
    }
    fn submit_query(&mut self) {
        let selected_tab = &self.tabs[self.selected_tab];

        if selected_tab.input.is_empty() {
            return;
        }

        let _ = self
            .action_sender
            .send(Action::Query(selected_tab.input.clone()));
    }

    fn update_cursor_shape(&self) -> anyhow::Result<()> {
        let cursor = match self.input_mode {
            InputMode::Normal => SetCursorStyle::SteadyBlock,
            InputMode::Insert => SetCursorStyle::SteadyBar,
        };
        execute!(std::io::stdout(), cursor)?;

        Ok(())
    }

    fn previous_tab(&mut self) {
        if self.selected_tab > 0 {
            self.selected_tab -= 1;
        }
    }
    fn next_tab(&mut self) {
        if self.selected_tab + 1 < self.tabs.len() {
            self.selected_tab += 1;
        }
    }

    fn new_tab(&mut self) {
        let tab_number = self.tabs.len() + 1;
        let name = format!("Query {}", tab_number);
        self.tabs.push(Tab::new(name));
        self.selected_tab = self.tabs.len() - 1;
    }

    fn delete_tab(&mut self) {
        if self.tabs.len() == 1 {
            return;
        }

        self.tabs.remove(self.selected_tab);

        if self.selected_tab == 0 {
            self.selected_tab += 1;
        } else {
            self.selected_tab -= 1;
        }

        self.update_tabs();
    }

    fn update_tabs(&mut self) {
        for (i, tab) in self.tabs.iter_mut().enumerate() {
            tab.name = format!("Query {}", i + 1);
        }
    }

    fn clear_results(&mut self) {
        let selected_tab = &mut self.tabs[self.selected_tab];
        selected_tab.query_result = QueryResult::None;
    }

    fn delete_input(&mut self) {
        let selected_tab = &mut self.tabs[self.selected_tab];
        selected_tab.input.clear();
        selected_tab.char_index = 0;
    }

    fn append_char(&mut self, c: char) {
        let selected_tab = &mut self.tabs[self.selected_tab];
        selected_tab.input.insert(selected_tab.char_index, c);
        selected_tab.char_index += 1;
    }

    fn delete_last_char(&mut self) {
        let selected_tab = &mut self.tabs[self.selected_tab];
        if selected_tab.char_index > 0 {
            selected_tab.input.remove(selected_tab.char_index - 1);
            selected_tab.char_index -= 1;
        }
    }

    fn delete_next_char(&mut self) {
        let selected_tab = &mut self.tabs[self.selected_tab];

        if selected_tab.char_index < selected_tab.input.len() {
            selected_tab.input.remove(selected_tab.char_index);

            if selected_tab.char_index >= selected_tab.input.len() && selected_tab.char_index > 0 {
                selected_tab.char_index -= 1;
            }
        }
    }

    fn move_cursor_left(&mut self) {
        let selected_tab = &mut self.tabs[self.selected_tab];

        if selected_tab.input.is_empty() {
            return;
        }

        if selected_tab.char_index > 0 {
            selected_tab.char_index -= 1;
        }
    }
    fn move_cursor_right(&mut self) {
        let selected_tab = &mut self.tabs[self.selected_tab];
        if selected_tab.input.is_empty() {
            return;
        }
        if selected_tab.char_index < selected_tab.input.len() - 1 {
            selected_tab.char_index += 1;
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
            InputMode::Normal => write!(f, " NORMAL "),
            InputMode::Insert => write!(f, " INSERT "),
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
async fn main() {
    if let Err(e) = run().await {
        eprintln!("\x1b[31m{}\x1b[0m", e);
        std::process::exit(1);
    }
}
async fn run() -> anyhow::Result<()> {
    dotenv::dotenv().ok();

    let config = config::load_config()?;

    let db = select_database(&config)?;

    let db_tokens = config.cache.database_token.as_ref().ok_or(anyhow::anyhow!(
        "No database tokens found in config, use `turso db shell DB_NAME` to populate the config",
    ))?;

    let db_token = db_tokens.get(db.db_id.as_str()).ok_or(anyhow::anyhow!(
        "No database token found for {}, use `turso db shell {}` to populate the config",
        db.name,
        db.name
    ))?;
    let url = format!("wss://{}", db.hostname);

    let mut client = ws::LibSqlClient::connect(&url, &db_token.data).await?;

    client.open_stream(1).await?;

    let client = Arc::new(Mutex::new(client));

    let (action_tx, mut action_rx) = mpsc::unbounded_channel::<Action>();
    let (result_tx, result_rx) = mpsc::unbounded_channel::<QueryResult>();
    let (latency_tx, latency_rx) = mpsc::unbounded_channel::<f32>();

    let mut app = App {
        url: url.to_string(),
        input_mode: InputMode::default(),
        // table_state: TableState::default(),
        latency: 0.0,
        action_sender: action_tx,
        res_recv: result_rx,
        latency_recv: latency_rx,
        tabs: vec![],
        selected_tab: 0,
    };
    app.new_tab();

    let terminal = ratatui::init();

    let client_c = client.clone();
    tokio::spawn(async move {
        loop {
            if let Ok(latency) = client_c.lock().await.send_ping().await {
                let _ = latency_tx.send(latency);
            }
            tokio::time::sleep(Duration::from_secs(5)).await;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn mock_app() -> App {
        let (action_tx, _) = mpsc::unbounded_channel::<Action>();
        let (_, result_rx) = mpsc::unbounded_channel::<QueryResult>();
        let (_, latency_rx) = mpsc::unbounded_channel::<f32>();

        App {
            url: "".to_string(),
            input_mode: InputMode::default(),
            // table_state: TableState::default(),
            latency: 0.0,
            action_sender: action_tx,
            res_recv: result_rx,
            latency_recv: latency_rx,
            tabs: vec![],
            selected_tab: 0,
        }
    }
    #[test]
    fn test_move_next_sql() {
        let mut app = mock_app();

        let input = "SELECT * FROM todos";
        let expected = ['S', '*', 'F', 't', 's'];
        let tab = Tab {
            name: "Query 1".to_string(),
            input: input.to_string(),
            char_index: 0,
            query_result: QueryResult::default(),
        };
        app.tabs.push(tab);
        let chars = input.chars().collect::<Vec<char>>();

        for (i, e) in expected.iter().enumerate() {
            let idx = app.tabs[0].char_index;
            assert_eq!(chars[idx], *e);
            if i < expected.len() - 1 {
                app.move_next();
            }
        }
    }
    #[test]
    fn test_move_next_code() {
        let mut app = mock_app();

        let input = ".map(|t| format!(\" {{}} \", t.name)";

        let expected = [
            '.', 'm', '(', 't', '|', 'f', '!', '{', '"', 't', '.', 'n', ')',
        ];
        let tab = Tab {
            name: "Query 1".to_string(),
            input: input.to_string(),
            char_index: 0,
            query_result: QueryResult::default(),
        };
        app.tabs.push(tab);
        let chars = input.chars().collect::<Vec<char>>();

        for (i, e) in expected.iter().enumerate() {
            let idx = app.tabs[0].char_index;
            assert_eq!(chars[idx], *e);
            if i < expected.len() - 1 {
                app.move_next();
            }
        }
    }
    #[test]
    fn test_move_back_code() {
        let mut app = mock_app();

        let input = ".map(|t| format!(\" {{}} \", t.name)";

        let expected = [
            ')', 'n', '.', 't', '"', '{', '!', 'f', '|', 't', '(', 'm', '.',
        ];

        let tab = Tab {
            name: "Query 1".to_string(),
            input: input.to_string(),
            char_index: input.len(),
            query_result: QueryResult::default(),
        };
        app.tabs.push(tab);
        let chars = input.chars().collect::<Vec<char>>();

        for e in expected.iter() {
            app.move_last();
            let idx = app.tabs[0].char_index;
            assert_eq!(chars[idx], *e);
        }
    }
}
