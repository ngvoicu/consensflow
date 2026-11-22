//! One row of the screen: its characters, whether each was drawn styled, and
//! whether the text wrapped on into the row below. Its digest is kept until a
//! cell changes, so the screen's digest after a read costs the rows that read
//! touched and not the whole grid (T-1941: hashing every row after every read of
//! a flood was most of a hidden window's intake time). Every write to a cell goes
//! through `cells_mut`, `styled_mut` or `blank_cells`, and drops the kept digest.

use std::cell::Cell;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

#[derive(Debug, Clone)]
pub(super) struct Row {
    cells: Vec<char>,
    /// Whether each cell was drawn styled.
    styled: Vec<bool>,
    /// The text went on in the row below, because it filled this one: both
    /// are one line.
    pub(super) wrapped: bool,
    /// The digest of `cells` and `styled`, once taken and until either is written.
    kept: Cell<Option<u64>>,
}

impl Row {
    pub(super) fn blank(cols: usize) -> Self {
        Self {
            cells: vec![' '; cols],
            styled: vec![false; cols],
            wrapped: false,
            kept: Cell::new(None),
        }
    }

    pub(super) fn cells(&self) -> &[char] {
        &self.cells
    }

    pub(super) fn styled(&self) -> &[bool] {
        &self.styled
    }

    pub(super) fn cells_mut(&mut self) -> &mut Vec<char> {
        self.kept.set(None);
        &mut self.cells
    }

    pub(super) fn styled_mut(&mut self) -> &mut Vec<bool> {
        self.kept.set(None);
        &mut self.styled
    }

    /// Blank the cells from `from` up to `to`, leaving the kept digest where nothing
    /// was there to blank.
    pub(super) fn blank_cells(&mut self, from: usize, to: usize) {
        let to = to.min(self.cells.len());
        if from < to
            && (self.cells[from..to].iter().any(|cell| *cell != ' ')
                || self.styled[from..to].iter().any(|styled| *styled))
        {
            self.kept.set(None);
            self.cells[from..to].fill(' ');
            self.styled[from..to].fill(false);
        }
    }

    /// A digest of the row's characters and styled marks.
    pub(super) fn digest(&self) -> u64 {
        if let Some(kept) = self.kept.get() {
            return kept;
        }
        let mut hasher = DefaultHasher::new();
        self.cells.hash(&mut hasher);
        self.styled.hash(&mut hasher);
        let digest = hasher.finish();
        self.kept.set(Some(digest));
        digest
    }

    /// The same digest, taken whatever is kept (for the test that the kept one is right).
    #[cfg(test)]
    pub(super) fn fresh_digest(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.cells.hash(&mut hasher);
        self.styled.hash(&mut hasher);
        hasher.finish()
    }
}
