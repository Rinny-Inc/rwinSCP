use std::collections::VecDeque;

use egui::Color32;

pub const DEFAULT_COLS: usize = 80;
pub const DEFAULT_ROWS: usize = 24;
const MAX_SCROLLBACK: usize = 5_000;
const TAB_STOP: usize = 8;

#[derive(Clone, PartialEq, Default, Debug)]
pub struct Attrs {
    pub fg: Option<Color32>,
    pub bg: Option<Color32>,
    pub bold: bool,
    pub inverse: bool,
}

#[derive(Clone, PartialEq, Debug)]
pub struct Cell {
    pub ch: char,
    pub attrs: Attrs,
}
impl Default for Cell {
    fn default() -> Self {
        Self {
            ch: ' ',
            attrs: Attrs::default(),
        }
    }
}
impl Cell {
    fn is_blank(&self) -> bool {
        self.ch == ' ' && self.attrs == Attrs::default()
    }
}

fn blank_row(cols: usize) -> Vec<Cell> {
    vec![Cell::default(); cols]
}

#[derive(Clone)]
struct Screen {
    rows: Vec<Vec<Cell>>,
    row: usize,
    col: usize,
    pending_wrap: bool,
    scroll_top: usize,
    scroll_bot: usize,
    saved: Option<(usize, usize)>,
}
impl Screen {
    fn new(cols: usize, rows: usize) -> Self {
        Self {
            rows: vec![blank_row(cols); rows],
            row: 0,
            col: 0,
            pending_wrap: false,
            scroll_top: 0,
            scroll_bot: rows.saturating_sub(1),
            saved: None,
        }
    }
}

#[derive(Clone, PartialEq)]
enum State {
    Ground,
    Esc,
    Csi,
    Osc,
    OscEsc,
    CharSet,
}

pub struct Terminal {
    cols: usize,
    rows: usize,
    screen: Screen,
    parked: Option<Screen>,
    scroll_back: VecDeque<Vec<Cell>>,
    attrs: Attrs,
    cursor_visible: bool,
    app_cursor_keys: bool,
    state: State,
    params: String,
    pty_size: (u32, u32),
}
impl Default for Terminal {
    fn default() -> Self {
        Self::new(DEFAULT_COLS, DEFAULT_ROWS)
    }
}
impl Terminal {
    pub fn new(cols: usize, rows: usize) -> Self {
        Self {
            cols,
            rows,
            screen: Screen::new(cols, rows),
            parked: None,
            scroll_back: VecDeque::new(),
            attrs: Attrs::default(),
            cursor_visible: true,
            app_cursor_keys: false,
            state: State::Ground,
            params: String::new(),
            pty_size: (0, 0),
        }
    }

    pub fn pty_size(&self) -> (u32, u32) {
        self.pty_size
    }

    pub fn app_cursor_keys(&self) -> bool {
        self.app_cursor_keys
    }

    fn history_rows(&self) -> usize {
        if self.parked.is_some() {
            0
        } else {
            self.scroll_back.len()
        }
    }

    pub fn row_count(&self) -> usize {
        self.history_rows() + self.visible_rows()
    }

    pub fn row_cells(&self, index: usize) -> &[Cell] {
        const EMPTY: &[Cell] = &[];
        let history = self.history_rows();
        if index < history {
            return &self.scroll_back[index];
        }
        match self.screen.rows.get(index - history) {
            Some(row) => row,
            None => EMPTY,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.scroll_back.is_empty() && self.screen.rows.iter().flatten().all(Cell::is_blank)
    }

    pub fn is_alt_screen(&self) -> bool {
        self.parked.is_some()
    }

    pub fn cursor(&self) -> Option<(usize, usize)> {
        self.cursor_visible
            .then_some((self.history_rows() + self.screen.row, self.screen.col))
    }

    #[allow(dead_code)]
    pub fn row_text(&self, index: usize) -> String {
        let text: String = self.row_cells(index).iter().map(|c| c.ch).collect();
        text.trim_end().to_owned()
    }

    fn visible_rows(&self) -> usize {
        if self.parked.is_some() {
            return self.rows;
        }
        let used = self
            .screen
            .rows
            .iter()
            .rposition(|row| row.iter().any(|c| !c.is_blank()))
            .map_or(0, |i| i + 1);
        used.max(self.screen.row + 1)
    }

    pub fn resize(&mut self, cols: u32, rows: u32) {
        self.pty_size = (cols, rows);
        let (cols, rows) = (cols.max(2) as usize, rows.max(2) as usize);
        if (cols, rows) == (self.cols, self.rows) {
            return;
        }

        for screen in [Some(&mut self.screen), self.parked.as_mut()]
            .into_iter()
            .flatten()
        {
            for row in &mut screen.rows {
                row.resize(cols, Cell::default());
            }
            screen.col = screen.col.min(cols - 1);
            screen.pending_wrap = false;
            screen.scroll_top = 0;
            screen.scroll_bot = rows - 1;
        }

        while self.screen.rows.len() > rows {
            let last = self.screen.rows.len() - 1;
            let spare = last > self.screen.row && self.screen.rows[last].iter().all(Cell::is_blank);
            if spare {
                self.screen.rows.pop();
                continue;
            }
            let line = self.screen.rows.remove(0);
            if self.parked.is_none() {
                self.push_scrollback(line);
            }
            self.screen.row = self.screen.row.saturating_sub(1);
        }
        while self.screen.rows.len() < rows {
            self.screen.rows.push(blank_row(cols));
        }
        if let Some(parked) = &mut self.parked {
            parked.rows.resize(rows, blank_row(cols));
            parked.row = parked.row.min(rows - 1);
        }

        self.cols = cols;
        self.rows = rows;
        self.screen.row = self.screen.row.min(rows - 1);
    }

    fn push_scrollback(&mut self, line: Vec<Cell>) {
        self.scroll_back.push_back(line);
        while self.scroll_back.len() > MAX_SCROLLBACK {
            self.scroll_back.pop_front();
        }
    }
}
impl Terminal {
    fn put(&mut self, ch: char) {
        if self.screen.pending_wrap {
            self.screen.pending_wrap = false;
            self.screen.col = 0;
            self.index();
        }
        let (row, col) = (self.screen.row, self.screen.col);
        if let Some(cell) = self.screen.rows.get_mut(row).and_then(|r| r.get_mut(col)) {
            *cell = Cell {
                ch,
                attrs: self.attrs.clone(),
            };
        }
        if col + 1 >= self.cols {
            self.screen.pending_wrap = true;
        } else {
            self.screen.col = col + 1;
        }
    }

    fn index(&mut self) {
        if self.screen.row == self.screen.scroll_bot {
            self.scroll_up(1);
            return;
        }
        if self.screen.row + 1 < self.rows {
            self.screen.row += 1;
        }
    }

    fn reverse_index(&mut self) {
        if self.screen.row == self.screen.scroll_top {
            self.scroll_down(1);
        } else {
            self.screen.row = self.screen.row.saturating_sub(1);
        }
    }

    fn scroll_up(&mut self, n: usize) {
        let keep = self.parked.is_none()
            && self.screen.scroll_top == 0
            && self.screen.scroll_bot + 1 == self.rows;
        for _ in 0..n.min(self.rows) {
            let line = self.screen.rows.remove(self.screen.scroll_top);
            if keep {
                self.push_scrollback(line);
            }
            self.screen
                .rows
                .insert(self.screen.scroll_bot, blank_row(self.cols));
        }
    }

    fn scroll_down(&mut self, n: usize) {
        for _ in 0..n.min(self.rows) {
            self.screen.rows.remove(self.screen.scroll_bot);
            self.screen
                .rows
                .insert(self.screen.scroll_top, blank_row(self.cols));
        }
    }

    fn blank_at(&mut self, row: usize, cols: std::ops::Range<usize>) {
        let fill = Cell {
            ch: ' ',
            attrs: Attrs {
                bg: self.attrs.bg,
                ..Default::default()
            },
        };
        if let Some(line) = self.screen.rows.get_mut(row) {
            for col in cols {
                if let Some(cell) = line.get_mut(col) {
                    *cell = fill.clone();
                }
            }
        }
    }
}
impl Terminal {
    pub fn feed(&mut self, chunk: &str) {
        for ch in chunk.chars() {
            match self.state {
                State::Ground => self.ground(ch),
                State::Esc => self.escape(ch),
                State::Csi => {
                    if ('\u{40}'..='\u{7E}').contains(&ch) {
                        self.csi(ch);
                        self.state = State::Ground;
                    } else {
                        self.params.push(ch);
                    }
                }
                State::Osc => match ch {
                    '\u{7}' => self.state = State::Ground,
                    '\u{1B}' => self.state = State::OscEsc,
                    _ => {}
                },
                State::OscEsc => self.state = State::Ground,
                State::CharSet => self.state = State::Ground,
            }
        }
    }

    fn ground(&mut self, ch: char) {
        match ch {
            '\u{1B}' => {
                self.state = State::Esc;
                self.params.clear();
            }
            '\n' | '\u{B}' | '\u{C}' => self.index(),
            '\r' => {
                self.screen.col = 0;
                self.screen.pending_wrap = false;
            }
            '\u{8}' => {
                self.screen.pending_wrap = false;
                self.screen.col = self.screen.col.saturating_sub(1);
            }
            '\t' => {
                let next = (self.screen.col / TAB_STOP + 1) * TAB_STOP;
                self.screen.col = next.min(self.cols - 1);
            }
            c if c.is_control() => {}
            c => self.put(c),
        }
    }

    fn escape(&mut self, ch: char) {
        self.state = State::Ground;
        match ch {
            '[' => {
                self.params.clear();
                self.state = State::Csi;
            }
            ']' => self.state = State::Osc,
            '(' | ')' | '*' | '+' => self.state = State::CharSet,
            'M' => self.reverse_index(),
            'D' => self.index(),
            'E' => {
                self.index();
                self.screen.col = 0;
            }
            '7' => self.screen.saved = Some((self.screen.row, self.screen.col)),
            '8' => {
                if let Some((row, col)) = self.screen.saved {
                    self.screen.row = row.min(self.rows - 1);
                    self.screen.col = col.min(self.cols - 1);
                }
            }
            'c' => self.reset(),
            _ => {}
        }
    }

    fn reset(&mut self) {
        let (cols, rows) = (self.cols, self.rows);
        self.screen = Screen::new(cols, rows);
        self.parked = None;
        self.attrs = Attrs::default();
        self.cursor_visible = true;
        self.app_cursor_keys = false;
    }

    fn numbers(&self) -> Vec<usize> {
        self.params
            .split(';')
            .map(|p| p.parse::<usize>().unwrap_or(0))
            .collect()
    }

    fn csi(&mut self, final_byte: char) {
        let params = std::mem::take(&mut self.params);

        if let Some(rest) = params.strip_prefix('?') {
            self.private_mode(rest, final_byte);
            return;
        }
        if params.starts_with(['>', '=', '<']) {
            return;
        }
        self.params = params;

        let args = self.numbers();
        let arg = |i: usize| args.get(i).copied().unwrap_or(0);
        let count = arg(0).max(1);
        let (row, col) = (self.screen.row, self.screen.col);

        match final_byte {
            'A' => self.screen.row = row.saturating_sub(count),
            'B' => self.screen.row = (row + count).min(self.rows - 1),
            'C' => {
                self.screen.col = (col + count).min(self.cols - 1);
                self.screen.pending_wrap = false;
            }
            'D' => {
                self.screen.col = col.saturating_sub(count);
                self.screen.pending_wrap = false;
            }
            'E' => {
                self.screen.row = (row + count).min(self.rows - 1);
                self.screen.col = 0;
            }
            'F' => {
                self.screen.row = row.saturating_sub(count);
                self.screen.col = 0;
            }
            'G' | '`' => self.screen.col = (count - 1).min(self.cols - 1),
            'd' => self.screen.row = (count - 1).min(self.rows - 1),
            'H' | 'f' => {
                self.screen.row = (arg(0).max(1) - 1).min(self.rows - 1);
                self.screen.col = (arg(1).max(1) - 1).min(self.cols - 1);
                self.screen.pending_wrap = false;
            }
            'J' => self.erase_display(arg(0)),
            'K' => self.erase_line(arg(0)),
            'L' => self.insert_lines(count),
            'M' => self.delete_lines(count),
            'P' => self.delete_chars(count),
            'X' => self.blank_at(row, col..(col + count).min(self.cols)),
            '@' => self.insert_blank(count),
            'S' => self.scroll_up(count),
            'T' => self.scroll_down(count),
            'r' => {
                let top = arg(0).max(1) - 1;
                let bot = if arg(1) == 0 { self.rows } else { arg(1) } - 1;
                if top < bot && bot < self.rows {
                    self.screen.scroll_top = top;
                    self.screen.scroll_bot = bot;
                }
                self.screen.row = 0;
                self.screen.col = 0;
            }
            's' => self.screen.saved = Some((row, col)),
            'u' => {
                if let Some((r, c)) = self.screen.saved {
                    self.screen.row = r.min(self.rows - 1);
                    self.screen.col = c.min(self.cols - 1);
                }
            }
            'm' => self.sgr(&args),
            _ => {}
        }
    }

    fn private_mode(&mut self, params: &str, final_byte: char) {
        let set = final_byte == 'h';
        if !matches!(final_byte, 'h' | 'l') {
            return;
        }
        for param in params.split(';') {
            match param.parse::<usize>().unwrap_or(0) {
                1 => self.app_cursor_keys = set,
                25 => self.cursor_visible = set,
                47 | 1047 | 1049 => {
                    if set {
                        self.enter_alt_screen();
                    } else {
                        self.leave_alt_screen();
                    }
                }
                _ => {}
            }
        }
    }

    fn enter_alt_screen(&mut self) {
        if self.parked.is_some() {
            return;
        }
        let fresh = Screen::new(self.cols, self.rows);
        self.parked = Some(std::mem::replace(&mut self.screen, fresh));
    }

    fn leave_alt_screen(&mut self) {
        if let Some(primary) = self.parked.take() {
            self.screen = primary;
            self.attrs = Attrs::default();
        }
    }

    fn erase_display(&mut self, mode: usize) {
        let (row, col) = (self.screen.row, self.screen.col);
        let cols = self.cols;
        match mode {
            0 => {
                self.blank_at(row, col..cols);
                for r in row + 1..self.rows {
                    self.blank_at(r, 0..cols);
                }
            }
            1 => {
                for r in 0..row {
                    self.blank_at(r, 0..cols);
                }
                self.blank_at(row, 0..(col + 1).min(cols));
            }
            _ => {
                for r in 0..self.rows {
                    self.blank_at(r, 0..cols);
                }
            }
        }
    }

    fn erase_line(&mut self, mode: usize) {
        let (row, col) = (self.screen.row, self.screen.col);
        let cols = self.cols;
        match mode {
            0 => self.blank_at(row, col..cols),
            1 => self.blank_at(row, 0..(col + 1).min(cols)),
            _ => self.blank_at(row, 0..cols),
        }
    }

    fn insert_lines(&mut self, n: usize) {
        let row = self.screen.row;
        if row < self.screen.scroll_top || row > self.screen.scroll_bot {
            return;
        }
        for _ in 0..n.min(self.rows) {
            self.screen.rows.remove(self.screen.scroll_bot);
            self.screen.rows.insert(row, blank_row(self.cols));
        }
    }

    fn delete_lines(&mut self, n: usize) {
        let row = self.screen.row;
        if row < self.screen.scroll_top || row > self.screen.scroll_bot {
            return;
        }
        for _ in 0..n.min(self.rows) {
            self.screen.rows.remove(row);
            self.screen
                .rows
                .insert(self.screen.scroll_bot, blank_row(self.cols));
        }
    }

    fn delete_chars(&mut self, n: usize) {
        let (row, col) = (self.screen.row, self.screen.col);
        let cols = self.cols;
        if let Some(line) = self.screen.rows.get_mut(row) {
            for _ in 0..n.min(cols) {
                if col >= line.len() {
                    continue;
                }
                line.remove(col);
                line.push(Cell::default());
            }
        }
    }

    fn insert_blank(&mut self, n: usize) {
        let (row, col) = (self.screen.row, self.screen.col);
        let cols = self.cols;
        if let Some(line) = self.screen.rows.get_mut(row) {
            for _ in 0..n.min(cols) {
                if col >= line.len() {
                    continue;
                }
                line.insert(col, Cell::default());
                line.truncate(cols);
            }
        }
    }

    fn sgr(&mut self, args: &[usize]) {
        if args.is_empty() {
            self.attrs = Attrs::default();
            return;
        }
        let mut i = 0;
        while i < args.len() {
            match args[i] {
                0 => self.attrs = Attrs::default(),
                1 => self.attrs.bold = true,
                7 => self.attrs.inverse = true,
                22 => self.attrs.bold = false,
                27 => self.attrs.inverse = false,
                30..=37 => self.attrs.fg = Some(ansi_color(args[i] - 30, false)),
                90..=97 => self.attrs.fg = Some(ansi_color(args[i] - 90, true)),
                40..=47 => self.attrs.bg = Some(ansi_color(args[i] - 40, false)),
                100..=107 => self.attrs.bg = Some(ansi_color(args[i] - 100, true)),
                39 => self.attrs.fg = None,
                49 => self.attrs.bg = None,
                38 | 48 => {
                    let fg = args[i] == 38;
                    let (color, used) = extended_color(&args[i + 1..]);
                    if let Some(color) = color {
                        if fg {
                            self.attrs.fg = Some(color);
                        } else {
                            self.attrs.bg = Some(color);
                        }
                    }
                    i += used;
                }
                _ => {}
            }
            i += 1;
        }
    }
}

pub fn append_terminal_output(terminal: &mut Terminal, chunk: &str) {
    terminal.feed(chunk);
}

fn ansi_color(index: usize, bright: bool) -> Color32 {
    const NORMAL: [Color32; 8] = [
        Color32::from_rgb(0x2E, 0x34, 0x40),
        Color32::from_rgb(0xBF, 0x61, 0x6A),
        Color32::from_rgb(0xA3, 0xBE, 0x8C),
        Color32::from_rgb(0xEB, 0xCB, 0x8B),
        Color32::from_rgb(0x81, 0xA1, 0xC1),
        Color32::from_rgb(0xB4, 0x8E, 0xAD),
        Color32::from_rgb(0x88, 0xC0, 0xD0),
        Color32::from_rgb(0xE5, 0xE9, 0xF0),
    ];
    const BRIGHT: [Color32; 8] = [
        Color32::from_rgb(0x4C, 0x56, 0x6A),
        Color32::from_rgb(0xD0, 0x77, 0x80),
        Color32::from_rgb(0xB5, 0xCE, 0xA0),
        Color32::from_rgb(0xF2, 0xD9, 0xA0),
        Color32::from_rgb(0x96, 0xB5, 0xD4),
        Color32::from_rgb(0xC8, 0xA2, 0xC1),
        Color32::from_rgb(0x9C, 0xD4, 0xE4),
        Color32::from_rgb(0xFF, 0xFF, 0xFF),
    ];
    let table = if bright { &BRIGHT } else { &NORMAL };
    table[index.min(7)]
}

fn extended_color(rest: &[usize]) -> (Option<Color32>, usize) {
    match rest.first() {
        Some(5) => {
            let index = rest.get(1).copied().unwrap_or(0);
            (Some(xterm256(index)), 2)
        }
        Some(2) => {
            let r = rest.get(1).copied().unwrap_or(0) as u8;
            let g = rest.get(2).copied().unwrap_or(0) as u8;
            let b = rest.get(3).copied().unwrap_or(0) as u8;
            (Some(Color32::from_rgb(r, g, b)), 4)
        }
        _ => (None, 0),
    }
}

fn xterm256(index: usize) -> Color32 {
    match index {
        0..=7 => ansi_color(index, false),
        8..=15 => ansi_color(index - 8, true),
        16..=231 => {
            const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
            let i = index - 16;
            Color32::from_rgb(LEVELS[i / 36], LEVELS[(i / 6) % 6], LEVELS[i % 6])
        }
        _ => {
            let level = 8 + 10 * (index.min(255) - 232) as u8;
            Color32::from_rgb(level, level, level)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(chunk: &str) -> Terminal {
        let mut t = Terminal::new(20, 6);
        t.feed(chunk);
        t
    }

    fn screen(t: &Terminal) -> Vec<String> {
        (0..t.row_count()).map(|i| t.row_text(i)).collect()
    }

    #[test]
    fn plain_text_lands_on_the_first_row() {
        let t = feed("hello");
        assert_eq!(screen(&t), ["hello"]);
        assert_eq!(t.cursor(), Some((0, 5)));
    }

    #[test]
    fn crlf_is_a_line_break_not_a_rewind() {
        let t = feed("hello\r\nworld\r\n");
        assert_eq!(screen(&t), ["hello", "world", ""]);
    }

    #[test]
    fn lone_cr_rewrites_the_current_line() {
        let t = feed("50%\r100%");
        assert_eq!(screen(&t), ["100%"]);
    }

    #[test]
    fn colour_sequences_are_stripped_from_the_text() {
        assert_eq!(
            feed("\u{1b}[32mgreen\u{1b}[0m text").row_text(0),
            "green text"
        );
        assert_eq!(feed("\u{1b}]0;window title\u{7}shell").row_text(0), "shell");
    }

    #[test]
    fn colour_sequences_set_attributes() {
        let t = feed("\u{1b}[31mred\u{1b}[0m!");
        let row = t.row_cells(0);
        assert_eq!(row[0].attrs.fg, Some(ansi_color(1, false)));
        assert_eq!(row[3].attrs.fg, None);
    }

    #[test]
    fn reverse_video_is_recorded() {
        let t = feed("\u{1b}[0;7m^X Exit\u{1b}[0m");
        assert!(t.row_cells(0)[0].attrs.inverse, "nano's key bar is inverse");
        assert!(!t.row_cells(0)[7].attrs.inverse);
    }

    #[test]
    fn arrow_left_does_not_delete_text() {
        let mut t = feed("hello");
        t.feed("\u{8}\u{8}");
        assert_eq!(t.row_text(0), "hello");
        assert_eq!(t.cursor(), Some((0, 3)));
        t.feed("X");
        assert_eq!(t.row_text(0), "helXo");
    }

    #[test]
    fn history_recall_keeps_the_prompt() {
        let mut t = Terminal::new(60, 6);
        t.feed("tester@canada:/var/www/html$ cd /tmp");
        t.feed(&"\u{8}".repeat(7));
        t.feed("ls -la /var/log/nginx");
        assert_eq!(
            t.row_text(0),
            "tester@canada:/var/www/html$ ls -la /var/log/nginx"
        );
    }

    #[test]
    fn history_recall_to_a_shorter_line_uses_delete_chars() {
        let mut t = Terminal::new(60, 6);
        t.feed("tester@canada:/var/www/html$ ls -la /var/log/nginx");
        t.feed(&"\u{8}".repeat(21));
        t.feed("\u{1b}[14Pcd /tmp");
        assert_eq!(t.row_text(0), "tester@canada:/var/www/html$ cd /tmp");
    }

    #[test]
    fn absolute_addressing_puts_text_where_asked() {
        let t = feed("\u{1b}[3;5Hmiddle");
        assert_eq!(screen(&t), ["", "", "    middle"]);
    }

    #[test]
    fn erase_display_clears_the_whole_grid() {
        let t = feed("one\r\ntwo\r\n\u{1b}[2J\u{1b}[Hfresh");
        assert_eq!(screen(&t), ["fresh"]);
    }

    #[test]
    fn output_past_the_last_row_scrolls_into_scrollback() {
        let mut t = Terminal::new(20, 3);
        for i in 0..6 {
            t.feed(&format!("line {i}\r\n"));
        }
        assert_eq!(
            screen(&t),
            [
                "line 0", "line 1", "line 2", "line 3", "line 4", "line 5", ""
            ]
        );
    }

    #[test]
    fn a_line_that_fills_the_width_wraps_once() {
        let mut t = Terminal::new(5, 4);
        t.feed("abcde");
        assert_eq!(t.cursor(), Some((0, 4)), "cursor rests on the last column");
        t.feed("f");
        assert_eq!(screen(&t), ["abcde", "f"]);
    }

    #[test]
    fn scroll_region_keeps_the_rows_outside_it() {
        let mut t = Terminal::new(10, 4);
        t.feed("\u{1b}[1;1Htop");
        t.feed("\u{1b}[2;4r"); // region = rows 2..4
        t.feed("\u{1b}[4;1Hlast\n");
        assert_eq!(t.row_text(0), "top", "the header row must not scroll");
    }
    #[test]
    fn alternate_screen_is_restored_on_exit() {
        let mut t = Terminal::new(20, 4);
        t.feed("shell output\r\n");
        t.feed("\u{1b}[?1049h\u{1b}[2J\u{1b}[1;1Hfull screen app");
        assert_eq!(t.row_text(0), "full screen app");
        assert_eq!(
            t.row_count(),
            4,
            "the alternate screen is exactly the viewport: no history above it"
        );
        t.feed("\u{1b}[?1049l");
        assert_eq!(
            screen(&t),
            ["shell output", ""],
            "the shell transcript comes back untouched"
        );
    }

    #[test]
    fn the_alternate_screen_does_not_leak_into_scrollback() {
        let mut t = Terminal::new(10, 3);
        t.feed("\u{1b}[?1049h");
        for i in 0..10 {
            t.feed(&format!("row {i}\r\n"));
        }
        t.feed("\u{1b}[?1049l");
        assert!(t.is_empty(), "a full-screen app leaves no history behind");
    }

    #[test]
    fn escape_sequences_split_across_chunks_still_parse() {
        let mut t = Terminal::new(20, 4);
        t.feed("a\u{1b}");
        t.feed("[3;1Hb");
        assert_eq!(screen(&t), ["a", "", "b"]);
    }

    #[test]
    fn cursor_can_be_hidden_and_shown() {
        let mut t = feed("\u{1b}[?25l");
        assert_eq!(t.cursor(), None);
        t.feed("\u{1b}[?25h");
        assert!(t.cursor().is_some());
    }

    #[test]
    fn application_cursor_keys_are_tracked() {
        let mut t = feed("\u{1b}[?1h");
        assert!(t.app_cursor_keys());
        t.feed("\u{1b}[?1l");
        assert!(!t.app_cursor_keys());
    }

    #[test]
    fn every_256_colour_index_resolves() {
        for index in 0..=255 {
            let mut t = Terminal::new(12, 2);
            t.feed(&format!("\u{1b}[38;5;{index}mx\u{1b}[0m"));
            assert_eq!(t.row_text(0), "x", "index {index} did not render");
        }
    }

    #[test]
    fn the_colour_cube_maps_to_the_right_corners() {
        // 16 is the cube's black corner, 231 its white one
        assert_eq!(xterm256(16), Color32::from_rgb(0, 0, 0));
        assert_eq!(xterm256(231), Color32::from_rgb(255, 255, 255));
        // 196 is pure red: index 180 -> (5, 0, 0)
        assert_eq!(xterm256(196), Color32::from_rgb(255, 0, 0));
    }

    #[test]
    fn extended_colour_parameters_are_consumed() {
        let t = feed("\u{1b}[38;2;1;7;0mplain");
        let cell = &t.row_cells(0)[0];
        assert_eq!(cell.attrs.fg, Some(Color32::from_rgb(1, 7, 0)));
        assert!(!cell.attrs.bold, "the 1 is a colour channel, not bold");
        assert!(
            !cell.attrs.inverse,
            "the 7 is a colour channel, not inverse"
        );
    }

    #[test]
    fn insert_lines_pushes_the_rest_of_the_region_down() {
        let mut t = Terminal::new(10, 4);
        t.feed("a\r\nb\r\nc\r\nd");
        t.feed("\u{1b}[2;1H\u{1b}[1L"); // open a line at row 2
        assert_eq!(
            screen(&t),
            ["a", "", "b", "c"],
            "the rows below must move down, not be overwritten"
        );
    }

    #[test]
    fn insert_and_delete_lines_respect_the_region() {
        let mut t = Terminal::new(10, 4);
        t.feed("a\r\nb\r\nc\r\nd");
        t.feed("\u{1b}[2;1H\u{1b}[1M"); // delete row 2
        assert_eq!(screen(&t), ["a", "c", "d"]);
    }

    #[test]
    fn erase_chars_blanks_without_shifting() {
        let mut t = Terminal::new(10, 2);
        t.feed("abcdef\u{1b}[1;2H\u{1b}[2X");
        assert_eq!(t.row_text(0), "a  def");
    }

    #[test]
    fn resize_keeps_the_text_and_the_cursor_on_screen() {
        let mut t = Terminal::new(20, 4);
        t.feed("hello\r\nworld");
        t.resize(10, 2);
        assert_eq!(
            screen(&t),
            ["hello", "world"],
            "shrinking gives up blank rows before it gives up text"
        );
        let (row, _) = t.cursor().unwrap();
        assert!(row < t.row_count());
    }

    #[test]
    fn scrollback_stays_bounded() {
        let mut t = Terminal::new(10, 3);
        for i in 0..(MAX_SCROLLBACK + 100) {
            t.feed(&format!("l{i}\r\n"));
        }
        assert!(t.row_count() <= MAX_SCROLLBACK + 3);
    }

    #[test]
    fn progress_bars_overwrite_instead_of_accumulating() {
        let mut t = Terminal::new(30, 4);
        for pct in [10, 50, 100] {
            t.feed(&format!("\r\u{1b}[2KProgress: {pct}%"));
        }
        assert_eq!(screen(&t), ["Progress: 100%"]);
    }

    #[test]
    fn multi_line_progress_bars_update_in_place() {
        let mut t = Terminal::new(30, 5);
        t.feed("a: 0%\r\nb: 0%\r\n");
        for pct in [50, 100] {
            t.feed(&format!("\u{1b}[2A\ra: {pct}%\u{1b}[K\r\n"));
            t.feed(&format!("\rb: {pct}%\u{1b}[K\r\n"));
        }
        assert_eq!(screen(&t), ["a: 100%", "b: 100%", ""]);
    }
}

/// A real GNU nano session
#[cfg(test)]
mod nano_golden {
    use super::*;

    const SESSION: &str = "nano /tmp/demo.txt\r\n\u{1b}[?2004l\r\u{1b}[?2004h\u{1b}[?1049h\u{1b}[22;0;0t\u{1b}[\
     1;24r\u{1b}(B\u{1b}[m\u{1b}[4l\u{1b}[?7h\u{1b}[39;49m\u{1b}[?1h\u{1b}=\u{1b}[?1h\
     \u{1b}=\u{1b}[?25l\u{1b}[39;49m\u{1b}(B\u{1b}[m\u{1b}[H\u{1b}[2J\u{1b}[22;34H\u{1b}(\
     B\u{1b}[0;7m[ Reading... ]\u{1b}(B\u{1b}[m\u{1b}[22;33H\u{1b}(B\u{1b}[0;7m[ Read 3 l\
     ines ]\u{1b}(B\u{1b}[m\u{1b}[H\u{1b}(B\u{1b}[0;7m  GNU nano 7.2                     \
      /tmp/demo.txt                               \u{1b}[1;79H\u{1b}(B\u{1b}[m\r\u{1b}[23\
     d\u{1b}(B\u{1b}[0;7m^G\u{1b}(B\u{1b}[m Help\u{1b}[14G\u{1b}(B\u{1b}[0;7m^O\u{1b}(B\
     \u{1b}[m Write Out \u{1b}(B\u{1b}[0;7m^W\u{1b}(B\u{1b}[m Where Is  \u{1b}(B\u{1b}[0;\
     7m^K\u{1b}(B\u{1b}[m Cut\u{1b}[53G\u{1b}(B\u{1b}[0;7m^T\u{1b}(B\u{1b}[m Execute   \
     \u{1b}(B\u{1b}[0;7m^C\u{1b}(B\u{1b}[m Location\r\u{1b}[24d\u{1b}(B\u{1b}[0;7m^X\
     \u{1b}(B\u{1b}[m Exit\u{1b}[14G\u{1b}(B\u{1b}[0;7m^R\u{1b}(B\u{1b}[m Read File \
     \u{1b}(B\u{1b}[0;7m^\\\u{1b}(B\u{1b}[m Replace   \u{1b}(B\u{1b}[0;7m^U\u{1b}(B\u{1b}\
     [m Paste     \u{1b}(B\u{1b}[0;7m^J\u{1b}(B\u{1b}[m Justify   \u{1b}(B\u{1b}[0;7m^/\
     \u{1b}(B\u{1b}[m Go To Line\r\u{1b}[2dalpha\r\u{1b}[3dbeta\r\u{1b}[4dgamma\r\u{1b}[2\
     d\u{1b}[?12l\u{1b}[?25h\u{1b}[?25l\u{1b}[1;51H\u{1b}(B\u{1b}[0;7m*\u{1b}[79G\u{1b}(B\
     \u{1b}[m\u{1b}[?12l\u{1b}[?25h\r\u{1b}[2ddeltaalpha\u{1b}[6G\u{1b}[?25l\u{1b}[23;14H\
            \u{1b}(B\u{1b}[0;7mM-D\u{1b}(B\u{1b}[m DOS Format      \u{1b}(B\u{1b}[0;7mM-A\
     \u{1b}(B\u{1b}[m Append          \u{1b}(B\u{1b}[0;7mM-B\u{1b}(B\u{1b}[m Backup File\
     \u{1b}[K\u{1b}[24;2H\u{1b}(B\u{1b}[0;7mC\u{1b}(B\u{1b}[m Cancel           \u{1b}(B\
     \u{1b}[0;7mM-M\u{1b}(B\u{1b}[m Mac Format      \u{1b}(B\u{1b}[0;7mM-P\u{1b}(B\u{1b}[\
     m Prepend         \u{1b}(B\u{1b}[0;7m^T\u{1b}(B\u{1b}[m Browse\u{1b}[K\r\u{1b}[22d\
     \u{1b}(B\u{1b}[0;7mFile Name to Write: /tmp/demo.txt                                \
                    \u{1b}[22;34H\u{1b}(B\u{1b}[m\u{1b}[?12l\u{1b}[?25h";

    fn render() -> Terminal {
        let mut t = Terminal::new(80, 24);
        for chunk in SESSION.as_bytes().chunks(37) {
            t.feed(&String::from_utf8_lossy(chunk));
        }
        t
    }

    #[test]
    fn nano_draws_its_title_bar() {
        let t = render();
        assert!(
            t.row_text(0).contains("GNU nano"),
            "got {:?}",
            t.row_text(0)
        );
        let inverse = t.row_cells(0).iter().filter(|c| c.attrs.inverse).count();
        assert_eq!(
            inverse, 79,
            "the title bar is a reverse-video band across the width"
        );
    }

    #[test]
    fn nano_draws_the_file_contents() {
        let t = render();
        assert_eq!(t.row_text(1), "deltaalpha");
        assert_eq!(t.row_text(2), "beta");
        assert_eq!(t.row_text(3), "gamma");
    }

    #[test]
    fn nano_draws_the_shortcut_bars() {
        let t = render();
        assert_eq!(t.row_text(21), "File Name to Write: /tmp/demo.txt");
        assert!(
            t.row_text(22).starts_with("^G Help"),
            "got {:?}",
            t.row_text(22)
        );
        assert!(
            t.row_text(23).starts_with("^C Cancel"),
            "the Ctrl-C/Ctrl-X options must be on screen, got {:?}",
            t.row_text(23)
        );
    }

    #[test]
    fn nano_puts_the_cursor_in_the_prompt() {
        assert_eq!(render().cursor(), Some((21, 33)));
    }

    #[test]
    fn nano_occupies_exactly_one_screen() {
        assert_eq!(
            render().row_count(),
            24,
            "a full-screen app must not spill into scrollback"
        );
    }
}
