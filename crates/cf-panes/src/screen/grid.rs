//! The cells of a pane's screen, and what the cursor and the program's
//! sequences do to them: text goes where the cursor is, lines scroll, erasures
//! blank cells, and the alternate screen (which full-screen programs draw on)
//! parks the main one and gives it back. Colours, styles and the strings a
//! terminal answers with change no text, so they are not here.

mod edit;

use super::parser::Action;

/// The most rows and columns the model keeps whatever the pty's size is: a
/// page may size a pty up to 4096 each way, and the model is for reading a few
/// lines.
const MAX_ROWS: usize = 400;
const MAX_COLS: usize = 1000;
/// Where the tab stops are: every eighth column.
const TAB: usize = 8;

/// A cell that is the second half of the wide character before it.
const SECOND_HALF: char = '\0';

#[derive(Debug, Clone)]
struct Row {
    cells: Vec<char>,
    /// The text went on in the row below, because it filled this one: both
    /// are one line.
    wrapped: bool,
}

impl Row {
    fn blank(cols: usize) -> Self {
        Self {
            cells: vec![' '; cols],
            wrapped: false,
        }
    }

    fn blank_cells(&mut self, from: usize, to: usize) {
        let to = to.min(self.cells.len());
        if from < to {
            self.cells[from..to].fill(' ');
        }
    }
}

/// Where the cursor was when it was saved.
#[derive(Debug, Clone, Copy)]
struct Saved {
    row: usize,
    col: usize,
    pending_wrap: bool,
}

#[derive(Debug)]
pub(super) struct Grid {
    rows: usize,
    cols: usize,
    cells: Vec<Row>,
    /// The main screen, while the alternate one shows.
    parked: Option<Vec<Row>>,
    row: usize,
    col: usize,
    /// The cursor is at the end of a full row and the next character wraps to
    /// the row below: it is not moved there until that character comes.
    pending_wrap: bool,
    saved: Option<Saved>,
    /// The rows (first and last) that scroll.
    top: usize,
    bottom: usize,
    autowrap: bool,
    /// The last character printed, which `CSI b` repeats.
    last: Option<char>,
}

impl Grid {
    /// A blank screen, its cursor at the top left, kept to the most the model keeps.
    pub(super) fn new(rows: usize, cols: usize) -> Self {
        let (rows, cols) = (rows.clamp(1, MAX_ROWS), cols.clamp(1, MAX_COLS));
        Self {
            rows,
            cols,
            cells: vec![Row::blank(cols); rows],
            parked: None,
            row: 0,
            col: 0,
            pending_wrap: false,
            saved: None,
            top: 0,
            bottom: rows - 1,
            autowrap: true,
            last: None,
        }
    }

    /// What the program's bytes said ([`Action`]) done to the screen.
    pub(super) fn perform(&mut self, action: Action) {
        match action {
            Action::Print(character) => self.print(character),
            Action::Control(byte) => self.control(byte),
            Action::Escape {
                intermediate: None,
                last,
            } => self.escape(last),
            // Character sets, double-width lines: no text changes with them.
            Action::Escape { .. } => {}
            Action::Csi {
                marker,
                params,
                intermediate: None,
                last,
            } => self.csi(marker, &params, last),
            Action::Csi { .. } => {}
        }
    }

    /// How many rows and columns the model keeps.
    #[cfg(test)]
    pub(super) fn size(&self) -> (usize, usize) {
        (self.rows, self.cols)
    }

    /// The screen's lines as a person reads them, the oldest first: a row the
    /// text wrapped from joins the row below it, and a line ends where its
    /// text does.
    pub(super) fn lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        let mut line = String::new();
        for row in &self.cells {
            line.extend(row.cells.iter().filter(|cell| **cell != SECOND_HALF));
            if !row.wrapped {
                lines.push(line.trim_end().to_owned());
                line.clear();
            }
        }
        if !line.is_empty() {
            lines.push(line.trim_end().to_owned());
        }
        lines
    }

    /// The pty took another size. Text keeps its place from the top left, and
    /// the cursor stays on the screen; nothing is reflowed.
    pub(super) fn resize(&mut self, rows: usize, cols: usize) {
        let (rows, cols) = (rows.clamp(1, MAX_ROWS), cols.clamp(1, MAX_COLS));
        // A cursor below the new last row takes the top rows with it.
        let dropped = (self.row + 1).saturating_sub(rows);
        self.cells.drain(..dropped.min(self.cells.len()));
        self.row -= dropped.min(self.row);
        for screen in std::iter::once(&mut self.cells).chain(self.parked.as_mut()) {
            screen.resize(rows, Row::blank(cols));
            for row in screen.iter_mut() {
                row.cells.resize(cols, ' ');
            }
        }
        self.rows = rows;
        self.cols = cols;
        self.top = 0;
        self.bottom = rows - 1;
        self.row = self.row.min(rows - 1);
        self.col = self.col.min(cols - 1);
    }

    fn control(&mut self, byte: u8) {
        match byte {
            0x08 => self.move_to(self.row, self.col.saturating_sub(1)),
            0x09 => self.tab(1),
            0x0A..=0x0C => self.line_feed(),
            0x0D => self.move_to(self.row, 0),
            // Bell, shift in and out, and the rest: nothing to see.
            _ => {}
        }
    }

    fn escape(&mut self, last: u8) {
        match last {
            b'7' => self.save_cursor(),
            b'8' => self.restore_cursor(),
            b'D' => self.line_feed(),
            b'E' => {
                self.move_to(self.row, 0);
                self.line_feed();
            }
            b'M' => self.reverse_index(),
            b'c' => *self = Self::new(self.rows, self.cols),
            _ => {}
        }
    }

    fn csi(&mut self, marker: Option<u8>, params: &[usize], last: u8) {
        // A count, which a missing or zero parameter makes 1.
        let count = |at: usize| {
            params
                .get(at)
                .copied()
                .filter(|value| *value > 0)
                .unwrap_or(1)
        };
        let first = params.first().copied().unwrap_or(0);
        if marker == Some(b'?') {
            match last {
                b'h' => self.set_modes(params, true),
                b'l' => self.set_modes(params, false),
                _ => {}
            }
            return;
        }
        if marker.is_some() {
            return;
        }
        match last {
            b'A' => self.move_to(
                self.row.saturating_sub(count(0)).max(self.up_limit()),
                self.col,
            ),
            b'B' | b'e' => self.move_to((self.row + count(0)).min(self.down_limit()), self.col),
            b'C' | b'a' => self.move_to(self.row, self.col + count(0)),
            b'D' => self.move_to(self.row, self.col.saturating_sub(count(0))),
            b'E' => self.move_to((self.row + count(0)).min(self.down_limit()), 0),
            b'F' => self.move_to(self.row.saturating_sub(count(0)).max(self.up_limit()), 0),
            b'G' | b'`' => self.move_to(self.row, count(0) - 1),
            b'd' => self.move_to(count(0) - 1, self.col),
            b'H' | b'f' => self.move_to(count(0) - 1, count(1) - 1),
            b'I' => self.tab(count(0)),
            b'Z' => self.back_tab(count(0)),
            b'J' => self.erase_display(first),
            b'K' => self.erase_line(first),
            b'L' => self.insert_lines(count(0)),
            b'M' => self.delete_lines(count(0)),
            b'@' => self.insert_chars(count(0)),
            b'P' => self.delete_chars(count(0)),
            b'X' => self.erase_chars(count(0)),
            b'S' => self.scroll_up(count(0)),
            b'T' => self.scroll_down(count(0)),
            b'b' => self.repeat(count(0)),
            b'r' => self.set_region(params),
            b's' if params.is_empty() => self.save_cursor(),
            b'u' => self.restore_cursor(),
            // Colours and styles, status reports, window operations.
            _ => {}
        }
    }

    fn set_modes(&mut self, params: &[usize], on: bool) {
        for &mode in params {
            match mode {
                7 => self.autowrap = on,
                47 | 1047 => self.alternate(on),
                1049 => {
                    if on {
                        self.save_cursor();
                    }
                    self.alternate(on);
                    if !on {
                        self.restore_cursor();
                    }
                }
                _ => {}
            }
        }
    }

    /// The alternate screen shows, blank, with the main one parked; or the
    /// main one comes back.
    fn alternate(&mut self, on: bool) {
        if on && self.parked.is_none() {
            let alternate = vec![Row::blank(self.cols); self.rows];
            self.parked = Some(std::mem::replace(&mut self.cells, alternate));
        } else if !on {
            if let Some(main) = self.parked.take() {
                self.cells = main;
            }
        }
    }

    fn print(&mut self, character: char) {
        let mut width = cell_width(character);
        if width == 0 {
            return;
        }
        if self.pending_wrap && self.autowrap {
            self.wrap();
        }
        self.pending_wrap = false;
        if width == 2 && self.col + 2 > self.cols {
            // A wide character that does not fit at the end of a row goes to
            // the next whole, and leaves a cell with no text in it behind.
            if self.autowrap && self.cols > 1 {
                self.cells[self.row].cells[self.col] = SECOND_HALF;
                self.wrap();
            } else {
                width = 1;
            }
        }
        let row = &mut self.cells[self.row];
        row.cells[self.col] = character;
        if width == 2 {
            row.cells[self.col + 1] = SECOND_HALF;
        }
        self.last = Some(character);
        if self.col + width >= self.cols {
            self.col = self.cols - 1;
            self.pending_wrap = true;
        } else {
            self.col += width;
        }
    }

    /// The text filled the row: it goes on in the one below.
    fn wrap(&mut self) {
        self.cells[self.row].wrapped = true;
        self.line_feed();
        self.col = 0;
        self.pending_wrap = false;
    }

    fn line_feed(&mut self) {
        self.pending_wrap = false;
        if self.row == self.bottom {
            self.scroll_up(1);
        } else if self.row + 1 < self.rows {
            self.row += 1;
        }
    }

    fn reverse_index(&mut self) {
        self.pending_wrap = false;
        if self.row == self.top {
            self.scroll_down(1);
        } else {
            self.row = self.row.saturating_sub(1);
        }
    }

    /// The top row a cursor moving up stops at: the region's, from inside it.
    fn up_limit(&self) -> usize {
        if self.row >= self.top {
            self.top
        } else {
            0
        }
    }

    fn down_limit(&self) -> usize {
        if self.row <= self.bottom {
            self.bottom
        } else {
            self.rows - 1
        }
    }

    fn move_to(&mut self, row: usize, col: usize) {
        self.row = row.min(self.rows - 1);
        self.col = col.min(self.cols - 1);
        self.pending_wrap = false;
    }

    fn tab(&mut self, times: usize) {
        let mut col = self.col;
        for _ in 0..times.min(self.cols) {
            col = (col / TAB + 1) * TAB;
        }
        self.move_to(self.row, col);
    }

    fn back_tab(&mut self, times: usize) {
        let mut col = self.col;
        for _ in 0..times.min(self.cols) {
            col = (col.saturating_sub(1) / TAB) * TAB;
        }
        self.move_to(self.row, col);
    }

    fn save_cursor(&mut self) {
        self.saved = Some(Saved {
            row: self.row,
            col: self.col,
            pending_wrap: self.pending_wrap,
        });
    }

    fn restore_cursor(&mut self) {
        let saved = self.saved.unwrap_or(Saved {
            row: 0,
            col: 0,
            pending_wrap: false,
        });
        self.move_to(saved.row, saved.col);
        self.pending_wrap = saved.pending_wrap;
    }

    fn set_region(&mut self, params: &[usize]) {
        let top = params.first().copied().filter(|v| *v > 0).unwrap_or(1) - 1;
        let bottom = params
            .get(1)
            .copied()
            .filter(|v| *v > 0)
            .unwrap_or(self.rows)
            .min(self.rows)
            - 1;
        if top < bottom {
            self.top = top;
            self.bottom = bottom;
        }
        self.move_to(0, 0);
    }

    fn scroll_up(&mut self, lines: usize) {
        for _ in 0..lines.min(self.bottom - self.top + 1) {
            self.cells.remove(self.top);
            self.cells.insert(self.bottom, Row::blank(self.cols));
        }
    }

    fn scroll_down(&mut self, lines: usize) {
        for _ in 0..lines.min(self.bottom - self.top + 1) {
            self.cells.remove(self.bottom);
            self.cells.insert(self.top, Row::blank(self.cols));
        }
    }

    fn repeat(&mut self, times: usize) {
        if let Some(character) = self.last {
            for _ in 0..times.min(self.rows * self.cols) {
                self.print(character);
            }
        }
    }
}

/// How many cells a character takes: none for a combining mark or a
/// zero-width character, two for what East Asian scripts and emoji draw wide,
/// one otherwise. The ranges are the blocks, not every code point of them.
fn cell_width(character: char) -> usize {
    match u32::from(character) {
        0x00..=0x1F | 0x7F..=0x9F => 0,
        0x300..=0x36F
        | 0x200B..=0x200F
        | 0x2028..=0x202E
        | 0x2060..=0x2064
        | 0xFE00..=0xFE0F
        | 0xFEFF => 0,
        0x1100..=0x115F
        | 0x2E80..=0x303E
        | 0x3041..=0x33FF
        | 0x3400..=0x4DBF
        | 0x4E00..=0x9FFF
        | 0xA000..=0xA4CF
        | 0xAC00..=0xD7A3
        | 0xF900..=0xFAFF
        | 0xFE30..=0xFE6F
        | 0xFF00..=0xFF60
        | 0xFFE0..=0xFFE6
        | 0x1F300..=0x1F64F
        | 0x1F900..=0x1F9FF
        | 0x20000..=0x3FFFD => 2,
        _ => 1,
    }
}
