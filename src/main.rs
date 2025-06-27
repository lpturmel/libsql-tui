use self::config::select_database;
use ratatui::{
    crossterm::{
        cursor::SetCursorStyle,
        event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
        execute,
    },
    layout::{Constraint, Direction, Flex, Layout, Rect},
    style::{Color, Style, Stylize},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, Tabs, Wrap},
    DefaultTerminal, Frame,
};
use std::{
    fmt::Display,
    time::{Duration, Instant},
};
use tokio::sync::mpsc;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

mod config;
mod db;

struct Tab {
    name: String,
    input: String,
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
    input_mode: InputMode,
    action_sender: mpsc::UnboundedSender<Action>,
    res_recv: mpsc::UnboundedReceiver<QueryResult>,
    tabs: Vec<Tab>,
    selected_tab: usize,
    show_help: bool,
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
                            (_, KeyCode::Char('?')) => {
                                self.show_help = !self.show_help;
                            }
                            (_, KeyCode::Esc) if self.show_help => self.show_help = false,
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

    fn render_top_bar(&self, f: &mut Frame, chunks: Rect) {
        let top_container = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(32), Constraint::Min(0)].as_ref())
            .split(chunks);

        let mode_span = Span::styled(
            self.input_mode.to_string(),
            Style::default().bold().bg(Color::Blue).fg(Color::Black),
        );
        let misc_line = Line::from(vec![mode_span]);
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
            QueryResult::Table(table) => {
                let rows = &table.rows;
                let columns = &table.columns;

                let header_cells = columns
                    .iter()
                    .map(|h| Cell::from(Text::from(h.to_uppercase())));
                let header = Row::new(header_cells).style(
                    ratatui::style::Style::default()
                        .bold()
                        .fg(ratatui::style::Color::Black)
                        .bg(ratatui::style::Color::White),
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

    fn render_help(&self, f: &mut Frame) {
        let area = App::popup_area(f.area(), 60, 70);
        f.render_widget(Clear, area);
        let block = Block::bordered().title(" Key-binds ");
        let para = Paragraph::new(App::help_lines())
            .block(block)
            .wrap(Wrap { trim: false });
        f.render_widget(para, area);
    }

    fn render_footer(&self, f: &mut Frame, area: Rect) {
        let style = Style::default().fg(Color::Indexed(246));
        let block = Block::default().border_style(style).borders(Borders::ALL);
        let para = Paragraph::new(Text::from("? for help | q to quit".to_string()))
            .block(block)
            .style(style)
            .wrap(Wrap { trim: false });
        f.render_widget(para, area);
    }

    fn draw(&self, f: &mut Frame) {
        let main_layout = Layout::vertical([
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Length(10),
            Constraint::Min(0),
            Constraint::Length(3),
        ]);

        let [top_area, tabs_area, query_area, results_area, footer_area] =
            main_layout.areas(f.area());

        self.render_tabs(f, tabs_area);

        self.render_top_bar(f, top_area);

        self.render_query(f, query_area);

        self.render_results(f, results_area);

        self.render_footer(f, footer_area);

        if self.show_help {
            self.render_help(f);
        }
    }
}

impl App {
    fn help_lines() -> Vec<Line<'static>> {
        vec![
            Line::from(vec![Span::styled(" NORMAL mode", Style::default().bold())]),
            Line::from(" i      → insert mode"),
            Line::from(" a      → append & insert mode"),
            Line::from(" w / b  → next / previous word"),
            Line::from(" h / l  → move cursor"),
            Line::from(" x      → delete char"),
            Line::from(" 0 / $  → begin / end of line"),
            Line::from(" D      → clear query"),
            Line::from(" c      → clear results"),
            Line::from(" Ctrl-r → run query"),
            Line::from(" Ctrl-n → new tab"),
            Line::from(" Ctrl-w → close tab"),
            Line::from(" Ctrl-t → list tables"),
            Line::from(" H / L  → prev / next tab"),
            Line::from(" q      → quit"),
            Line::from(" ?      → toggle this help"),
            Line::from(""),
            Line::from(" Press Esc or ? to close"),
            Line::from(""),
            Line::from(vec![Span::styled(" INSERT mode", Style::default().bold())]),
            Line::from(" Esc      → normal mode"),
        ]
    }
    fn popup_area(area: Rect, percent_x: u16, percent_y: u16) -> Rect {
        let vertical = Layout::vertical([Constraint::Percentage(percent_y)]).flex(Flex::Center);
        let horizontal = Layout::horizontal([Constraint::Percentage(percent_x)]).flex(Flex::Center);
        let [area] = vertical.areas(area);
        let [area] = horizontal.areas(area);
        area
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
    Table(db::Table),
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
    let url = format!("libsql://{}", db.hostname);

    let db = libsql::Builder::new_remote(url.clone(), db_token.data.clone())
        .build()
        .await?;
    let conn = db.connect()?;

    let client = db::LibSqlClient(conn);

    let (action_tx, mut action_rx) = mpsc::unbounded_channel::<Action>();
    let (result_tx, result_rx) = mpsc::unbounded_channel::<QueryResult>();

    let mut app = App {
        url: url.to_string(),
        input_mode: InputMode::default(),
        action_sender: action_tx,
        res_recv: result_rx,
        tabs: vec![],
        selected_tab: 0,
        show_help: false,
    };
    app.new_tab();

    let terminal = ratatui::init();

    let client = client.clone();
    tokio::spawn(async move {
        while let Some(action) = action_rx.recv().await {
            match action {
                Action::Query(query) => {
                    let res = client.query_owned(&query).await;
                    let res = match res {
                        Ok(table) => QueryResult::Table(table),
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

        App {
            url: "".to_string(),
            input_mode: InputMode::default(),
            action_sender: action_tx,
            res_recv: result_rx,
            tabs: vec![],
            selected_tab: 0,
            show_help: false,
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
