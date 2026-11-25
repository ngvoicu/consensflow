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
//!
//! It also says whether a chunk changed what it shows ([`Screen::feed`]), which is how
//! the pane host tells a screen at rest from one at work: a harness can print and
//! change nothing a person sees (Muse Code 1.4.14, after a command it ran in the
//! background was stopped, writes colour resets and a move of the cursor to where it
//! is every 50 ms, T-1083), and a screen that only does that is at rest.

mod grid;
mod parser;
#[cfg(test)]
mod tests;

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::{Mutex, PoisonError};

use grid::Grid;
use parser::Parser;

use cf_proto::panes::BOTTOM_LINES;

use crate::arbiter::OutputClock;

/// The last lines of a screen as drawn ([`Screen::bottom`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Drawn {
    pub lines: Vec<String>,
    /// The cursor, where it is on one of `lines`.
    pub cursor: Option<Cursor>,
}

impl Drawn {
    /// The last `max` lines of these, with the cursor where it is among them (none where it
    /// is on a line that is cut).
    pub fn cut(mut self, max: usize) -> Self {
        let first = self.lines.len().saturating_sub(max);
        self.lines.drain(..first);
        self.cursor = self.cursor.and_then(|cursor| {
            cursor
                .line
                .checked_sub(first)
                .map(|line| Cursor { line, ..cursor })
        });
        self
    }
}

/// Names what a reader of the input box is given of the screen: a digest of the lines and the
/// cursor of the last [`BOTTOM_LINES`] lines, as `pane.snapshot` answers them. A harness's
/// reader uses nothing else of the screen, so two looks with one name read the same box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Seen(u64);

impl Seen {
    /// The name of `drawn`: its lines, then the cursor (line, column, shown, plain after it) or
    /// that there is none.
    pub fn of(drawn: &Drawn) -> Self {
        let mut hasher = DefaultHasher::new();
        drawn.lines.len().hash(&mut hasher);
        for line in &drawn.lines {
            line.hash(&mut hasher);
        }
        match drawn.cursor {
            Some(cursor) => {
                1u8.hash(&mut hasher);
                (cursor.line, cursor.col, cursor.shown, cursor.plain_after).hash(&mut hasher);
            }
            None => 0u8.hash(&mut hasher),
        }
        Self(hasher.finish())
    }

    /// Sixteen hex digits: a JSON number loses bits over 2^53.
    pub fn hex(self) -> String {
        format!("{:016x}", self.0)
    }

    pub fn parse(text: &str) -> Option<Self> {
        (text.len() == 16 && text.bytes().all(|byte| byte.is_ascii_hexdigit()))
            .then(|| u64::from_str_radix(text, 16).ok().map(Self))
            .flatten()
    }
}

/// Where the cursor is in [`Drawn`]'s lines: the line, by its place among them,
/// how many characters of it are before the cursor, whether it is shown, and how
/// many characters of the line from the cursor on are drawn plain, not styled
/// (a colour other than the terminal's own, or faint), blanks left out: words
/// typed after it, and not a harness's faint hint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    pub line: usize,
    pub col: usize,
    pub shown: bool,
    pub plain_after: usize,
}

/// One screen, fed the bytes a program prints.
#[derive(Debug)]
pub struct Screen {
    parser: Parser,
    grid: Grid,
    /// The digest of what the screen showed after the last chunk ([`Grid::digest`]).
    shows: u64,
}

impl Screen {
    pub fn new(rows: u16, cols: u16) -> Self {
        let grid = Grid::new(usize::from(rows), usize::from(cols));
        Self {
            parser: Parser::default(),
            shows: grid.digest(),
            grid,
        }
    }

    /// The program printed `bytes`. They may end anywhere, even inside a
    /// character or an escape sequence: the rest is in the next chunk. Says whether
    /// what the screen shows changed with them: a character, where it is, whether it
    /// is drawn styled, the alternate screen shown or put away. Colours, the cursor,
    /// and text drawn again over the same text change nothing shown. Limit: a frame
    /// that blanks a line and draws it again, read from the pty in two pieces, shows
    /// the blank line between them, and that is a change.
    pub fn feed(&mut self, bytes: &[u8]) -> bool {
        let grid = &mut self.grid;
        for &byte in bytes {
            self.parser
                .advance(byte, &mut |action| grid.perform(action));
        }
        let shows = self.grid.digest();
        let changed = shows != self.shows;
        self.shows = shows;
        changed
    }

    /// The pty has another size now. The resize is the host's and no change of the
    /// program's: what the program draws for the new size is.
    pub fn resize(&mut self, rows: u16, cols: u16) {
        self.grid.resize(usize::from(rows), usize::from(cols));
        self.shows = self.grid.digest();
    }

    /// The last `max` lines the screen shows, the oldest first: no empty
    /// line, and no line that is only drawing (a border, a rule, a prompt's
    /// `$`) with no letter or digit in it, and each without the blanks that
    /// end it. A text that wrapped at the edge of the screen is one line.
    pub fn tail(&self, max: usize) -> Vec<String> {
        cf_proto::panes::word_tail(self.grid.lines(), max)
    }

    /// The last `max` lines of the screen as it is drawn, the oldest first,
    /// from its last line with anything on it: its rules, its empty lines
    /// and a prompt with nothing typed at it too, which [`Screen::tail`]
    /// leaves out; and the cursor, where it is on one of them. A harness's
    /// input box is read here.
    pub fn bottom(&self, max: usize) -> Drawn {
        let mut lines = self.grid.lines();
        let drawn = lines
            .iter()
            .rposition(|line| !line.is_empty())
            .map_or(0, |last| last + 1);
        lines.truncate(drawn);
        let first = lines.len().saturating_sub(max);
        lines.drain(..first);
        let (line, col, shown, plain_after) = self.grid.cursor();
        let cursor = line
            .checked_sub(first)
            .filter(|line| *line < lines.len())
            .map(|line| Cursor {
                line,
                col,
                shown,
                plain_after,
            });
        Drawn { lines, cursor }
    }

    /// What a reader of the input box is given of the screen now ([`Seen`]).
    pub fn seen(&self) -> Seen {
        Seen::of(&self.bottom(BOTTOM_LINES))
    }
}

/// A pane's [`Screen`], shared between the thread that feeds it, the table that
/// resizes it and the handlers that read it. Its lock is held for one chunk or
/// one read, and never while any other is.
#[derive(Debug)]
pub struct PaneScreen {
    screen: Mutex<Screen>,
    #[cfg(test)]
    pub(crate) hook: FeedHook,
}

/// Where a test holds a read on its way into the screen.
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FeedStage {
    /// The host has the read; the screen's lock is not yet taken.
    Read,
    /// The screen's lock is held; the read is not yet fed.
    Locked,
    /// The screen has been fed the read, and the clock not yet noted.
    Fed,
    /// A reader has the screen's lines and not yet the clocks.
    Snapshot,
}

/// A test's hold on [`FeedStage`]s: called at each, on the feeding thread.
#[cfg(test)]
#[derive(Default)]
pub(crate) struct FeedHook(Mutex<Option<std::sync::Arc<dyn Fn(FeedStage) + Send + Sync>>>);

#[cfg(test)]
impl FeedHook {
    pub(crate) fn set(&self, hold: impl Fn(FeedStage) + Send + Sync + 'static) {
        *self.0.lock().unwrap() = Some(std::sync::Arc::new(hold));
    }

    fn at(&self, stage: FeedStage) {
        let hold = self.0.lock().unwrap().clone();
        if let Some(hold) = hold {
            hold(stage);
        }
    }
}

#[cfg(test)]
impl std::fmt::Debug for FeedHook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("FeedHook")
    }
}

impl PaneScreen {
    /// A blank screen of the pty's size.
    pub fn new(rows: u16, cols: u16) -> Self {
        Self {
            screen: Mutex::new(Screen::new(rows, cols)),
            #[cfg(test)]
            hook: FeedHook::default(),
        }
    }

    /// The program printed `bytes` ([`Screen::feed`]): whether what the screen
    /// shows changed with them.
    pub fn feed(&self, bytes: &[u8]) -> bool {
        self.locked().feed(bytes)
    }

    /// The one place a read of the window's output is taken in, in this order and under the
    /// screen's lock: `kept` takes it (the journal), the screen is fed ([`Screen::feed`]), the
    /// output clock says so, and it is counted drawn. No reader under the lock
    /// ([`PaneScreen::observe`]) sees the clock or the count past a read the screen does not
    /// show. The journal takes it first, so that a feed that panics half-way leaves the kept
    /// journal whole and the screen part of it, never the screen ahead of the journal (T-2230).
    /// Whether the screen changed.
    pub(crate) fn feed_noting(
        &self,
        bytes: &[u8],
        printed: &OutputClock,
        kept: impl FnOnce(&[u8]),
    ) -> bool {
        #[cfg(test)]
        self.hook.at(FeedStage::Read);
        crate::sync::point("feed.start");
        let mut screen = self.locked();
        #[cfg(test)]
        self.hook.at(FeedStage::Locked);
        crate::sync::point("feed.locked");
        kept(bytes);
        let changed = screen.feed(bytes);
        #[cfg(test)]
        self.hook.at(FeedStage::Fed);
        crate::sync::point("feed.fed");
        // The clock first: between the two the window still reads as having output pending.
        printed.note(changed);
        crate::sync::point("feed.noted");
        printed.counts().drew(bytes.len());
        crate::sync::point("feed.drew");
        changed
    }

    /// What `read` makes of the screen and what `clocks` reads, both with the screen's lock
    /// held: the output clock and the screen of one moment (a read that is being fed is in
    /// both or in neither).
    pub(crate) fn observe<R, C>(
        &self,
        read: impl FnOnce(&Screen) -> R,
        clocks: impl FnOnce() -> C,
    ) -> (R, C) {
        crate::sync::point("look.start");
        let screen = self.locked();
        let drawn = read(&screen);
        #[cfg(test)]
        self.hook.at(FeedStage::Snapshot);
        crate::sync::point("look.rows");
        let clocked = clocks();
        crate::sync::point("look.counts");
        (drawn, clocked)
    }

    /// Runs `under` with the screen's lock held, given the screen at that moment.
    pub(crate) fn with_screen(&self, under: &mut dyn FnMut(&Screen)) {
        let screen = self.locked();
        crate::sync::point("seen.locked");
        under(&screen);
    }

    /// The pty has another size now ([`Screen::resize`]).
    pub fn resize(&self, rows: u16, cols: u16) {
        self.locked().resize(rows, cols);
    }

    /// The last `max` lines the screen shows ([`Screen::tail`]).
    pub fn tail(&self, max: usize) -> Vec<String> {
        self.locked().tail(max)
    }

    /// The last `max` lines of the screen as it is drawn ([`Screen::bottom`]).
    pub fn bottom(&self, max: usize) -> Drawn {
        self.locked().bottom(max)
    }

    /// A panic in a thread that held the lock leaves a screen that was only
    /// partly written to, which reads as well as it can.
    fn locked(&self) -> std::sync::MutexGuard<'_, Screen> {
        crate::sync::lock(&self.screen).unwrap_or_else(PoisonError::into_inner)
    }
}
