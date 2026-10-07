//! What a program's sequences do to the cells already on the screen: erase
//! them, and insert or delete characters and lines, shifting the rest.

use super::{Grid, Row};

impl Grid {
    /// `CSI L`: blank lines at the cursor, within the scroll region, pushing
    /// the rest down.
    pub(super) fn insert_lines(&mut self, lines: usize) {
        if (self.top..=self.bottom).contains(&self.row) {
            for _ in 0..lines.min(self.bottom - self.row + 1) {
                self.cells.remove(self.bottom);
                self.cells.insert(self.row, Row::blank(self.cols));
            }
            self.move_to(self.row, 0);
        }
    }

    /// `CSI M`: lines at the cursor gone, within the scroll region, the rest
    /// moving up.
    pub(super) fn delete_lines(&mut self, lines: usize) {
        if (self.top..=self.bottom).contains(&self.row) {
            for _ in 0..lines.min(self.bottom - self.row + 1) {
                self.cells.remove(self.row);
                self.cells.insert(self.bottom, Row::blank(self.cols));
            }
            self.move_to(self.row, 0);
        }
    }

    /// `CSI @`: blanks at the cursor, the rest of the row moving right.
    pub(super) fn insert_chars(&mut self, count: usize) {
        let (col, cols) = (self.col, self.cols);
        let cells = &mut self.cells[self.row].cells;
        for _ in 0..count.min(cols - col) {
            cells.insert(col, ' ');
            cells.pop();
        }
    }

    /// `CSI P`: characters at the cursor gone, the rest of the row moving left.
    pub(super) fn delete_chars(&mut self, count: usize) {
        let (col, cols) = (self.col, self.cols);
        let cells = &mut self.cells[self.row].cells;
        let count = count.min(cols - col);
        cells.drain(col..col + count);
        cells.extend(std::iter::repeat_n(' ', count));
    }

    /// `CSI X`: characters at the cursor blanked where they are.
    pub(super) fn erase_chars(&mut self, count: usize) {
        let col = self.col;
        self.cells[self.row].blank_cells(col, col + count);
    }

    /// `CSI K`: the row from the cursor (0), to it (1), or all of it (2).
    pub(super) fn erase_line(&mut self, mode: usize) {
        let (col, cols) = (self.col, self.cols);
        let row = &mut self.cells[self.row];
        match mode {
            0 => {
                row.blank_cells(col, cols);
                row.wrapped = false;
            }
            1 => row.blank_cells(0, col + 1),
            2 => {
                row.blank_cells(0, cols);
                row.wrapped = false;
            }
            _ => {}
        }
    }

    /// `CSI J`: the screen from the cursor (0), to it (1), or all of it (2).
    pub(super) fn erase_display(&mut self, mode: usize) {
        let (row, cols) = (self.row, self.cols);
        let clear = |rows: &mut [Row]| {
            for row in rows {
                row.blank_cells(0, cols);
                row.wrapped = false;
            }
        };
        match mode {
            0 => {
                self.erase_line(0);
                clear(&mut self.cells[row + 1..]);
            }
            1 => {
                clear(&mut self.cells[..row]);
                self.erase_line(1);
            }
            2 => clear(&mut self.cells),
            _ => {}
        }
    }
}
