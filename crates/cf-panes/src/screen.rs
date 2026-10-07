//! A pane's screen, as a person at its terminal would see it. The pane host
//! streams a program's output on and forgets it; a window that fails to come
//! up says why on its screen, and the screen is gone with the window. This
//! keeps it: the cells the program's text, cursor movements and erasures left,
//! so the last lines it showed can be read when it ends, or while it is stuck.
//!
//! It is the text only (no colours, no styles) of one screen the size of the
//! pty, with the alternate screen full-screen programs draw on, and without
//! scrollback: what scrolled off is not kept. A resize moves nothing around.
//! Nothing here answers the program: a program that asks the terminal a
//! question is answered by the page, or by the daemon's `pane.reply`.

mod grid;
mod parser;
#[cfg(test)]
mod tests;

use std::sync::{Mutex, PoisonError};

use grid::Grid;
use parser::Parser;

/// One screen, fed the bytes a program prints.
#[derive(Debug)]
pub struct Screen {
    parser: Parser,
    grid: Grid,
}

impl Screen {
    pub fn new(rows: u16, cols: u16) -> Self {
        Self {
            parser: Parser::default(),
            grid: Grid::new(usize::from(rows), usize::from(cols)),
        }
    }

    /// The program printed `bytes`. They may end anywhere, even inside a
    /// character or an escape sequence: the rest is in the next chunk.
    pub fn feed(&mut self, bytes: &[u8]) {
        let grid = &mut self.grid;
        for &byte in bytes {
            self.parser
                .advance(byte, &mut |action| grid.perform(action));
        }
    }

    /// The pty has another size now.
    pub fn resize(&mut self, rows: u16, cols: u16) {
        self.grid.resize(usize::from(rows), usize::from(cols));
    }

    /// The last `max` lines the screen shows, the oldest first: no empty
    /// line, and no line that is only drawing (a border, a rule, a prompt's
    /// `$`) with no letter or digit in it, and each without the blanks that
    /// end it. A text that wrapped at the edge of the screen is one line.
    pub fn tail(&self, max: usize) -> Vec<String> {
        let mut lines: Vec<String> = self
            .grid
            .lines()
            .into_iter()
            .filter(|line| line.chars().any(char::is_alphanumeric))
            .collect();
        lines.drain(..lines.len().saturating_sub(max));
        lines
    }
}

/// A pane's [`Screen`], shared between the thread that feeds it, the table that
/// resizes it and the handlers that read it. Its lock is held for one chunk or
/// one read, and never while any other is.
#[derive(Debug)]
pub struct PaneScreen {
    screen: Mutex<Screen>,
}

impl PaneScreen {
    /// A blank screen of the pty's size.
    pub fn new(rows: u16, cols: u16) -> Self {
        Self {
            screen: Mutex::new(Screen::new(rows, cols)),
        }
    }

    /// The program printed `bytes` ([`Screen::feed`]).
    pub fn feed(&self, bytes: &[u8]) {
        self.locked().feed(bytes);
    }

    /// The pty has another size now ([`Screen::resize`]).
    pub fn resize(&self, rows: u16, cols: u16) {
        self.locked().resize(rows, cols);
    }

    /// The last `max` lines the screen shows ([`Screen::tail`]).
    pub fn tail(&self, max: usize) -> Vec<String> {
        self.locked().tail(max)
    }

    /// A panic in a thread that held the lock leaves a screen that was only
    /// partly written to, which reads as well as it can.
    fn locked(&self) -> std::sync::MutexGuard<'_, Screen> {
        self.screen.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
