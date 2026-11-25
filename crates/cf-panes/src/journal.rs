//! A window's journal: the raw bytes its program printed and the sizes its page took, kept so
//! that a page that reloaded can write them into a new xterm, muted, and have the state xterm
//! itself would have had. The host does not emulate xterm for it, draws no screen text and
//! answers none of the program's terminal questions: the page's xterm does. The engine's
//! screen ([`crate::screen`]) stays what the engine reads.
//!
//! - **Offset:** the count of bytes a window's program has printed since it opened.
//! - **Rest point:** xterm's parser is in its ground state, between whole characters.
//! - **Mark:** an offset the journal may start at, with what a replay starting there needs
//!   (the modes and the margins, which the host tracks reliably).
//! - **Boundary (B):** where a page's subscription fixed what had been handed to a page: the
//!   history is the journal before it, and every byte from it on comes live
//!   ([`crate::output_hub::OutputHub`]).
//!
//! The bytes are copied into fixed 1 MiB segments, 8 at most for a window that runs. Past that
//! the journal starts again at its next mark. A window that has ended keeps its last 1 MiB.

mod modes;
mod parser;
mod seed;
#[cfg(test)]
mod tests;
mod tracker;

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

use tracker::{Region, Tracked, Tracker};

/// One segment of the journal's bytes.
const SEGMENT_BYTES: usize = 1 << 20;
/// How many segments a window that runs holds: the journal's 8 MiB.
const MAX_SEGMENTS: usize = 8;
/// The bytes after a mark from which the next message that ends at a rest point is one.
const MARK_AFTER: u64 = 1 << 20;
/// The bytes after a mark from which the next message's end is one though it is not at rest.
const FORCE_MARK_AFTER: u64 = 2 << 20;
/// What a window that has ended keeps at least: its last 1 MiB, from the last mark that
/// leaves that much (so a continuation from anywhere in it still has its bytes).
const ENDED_KEPT_BYTES: u64 = 1 << 20;
/// How many sizes the journal holds before it starts again at a mark at a rest point.
const MAX_RESIZES: usize = 256;
/// Past this many sizes the journal starts again at its end though it is not at rest (a page
/// that sends thousands of sizes inside one unfinished sequence).
const HARD_MAX_RESIZES: usize = 4096;
/// The most one piece of a pull holds.
pub const PIECE_BYTES: usize = 1 << 20;

/// A window's size, rows and columns.
pub type Size = (u16, u16);

#[derive(Clone)]
struct Mark {
    offset: u64,
    /// The modes at `offset`, and where the program last set the margins up to it.
    tracked: Tracked,
}

/// The size the page's emulator took, at the byte it had parsed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Resize {
    pub at: u64,
    pub rows: u16,
    pub cols: u16,
}

/// What a pull is answered ([`crate::pty::PaneTable::history`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Piece {
    /// The journal no longer holds where the page was, or the page had nothing: its emulator
    /// starts clean, takes `size`, and is written `seed` before `bytes`.
    pub reset: bool,
    /// The journal starts past the window's first byte: what scrolled off before `from` is
    /// not in it, and the page shows where history begins.
    pub truncated: bool,
    /// The offset of the first of `bytes`.
    pub from: u64,
    /// The emulator's size at `from`: for a piece that resets, at the journal's start, before
    /// the sizes in `resizes`, which include those at `from`.
    pub size: Size,
    /// The modes and margins at `from`; empty unless this piece resets and `truncated`.
    pub seed: Vec<u8>,
    /// The sizes the page's emulator takes within the piece, each at its offset (after
    /// `from`, up to the end of the piece).
    pub resizes: Vec<Resize>,
    pub bytes: Vec<u8>,
    /// The page's boundary: every byte from it on comes on the live path.
    pub boundary: u64,
    /// This piece reaches the boundary: the page has its history. No piece begins past the
    /// boundary, since the journal's start never passes what is acknowledged; a page given one
    /// all the same takes none of it.
    pub done: bool,
}

struct Inner {
    /// Segment `i` holds offsets from `(first_segment + i) * SEGMENT_BYTES`.
    segments: VecDeque<Vec<u8>>,
    first_segment: u64,
    spare: Option<Vec<u8>>,
    start: u64,
    end: u64,
    marks: VecDeque<Mark>,
    tracker: Tracker,
    /// The sizes taken after `start`, in order of their offsets. None is folded into another.
    resizes: Vec<Resize>,
    /// The size at `start`.
    base_size: Size,
    /// The offset of the last size before `start`, if any: every size resets the margins of both
    /// screens in xterm, so what the program set before it is the page's xterm's no longer.
    base_sized_at: Option<u64>,
    /// The boundary of the page now subscribed: a size is not journaled before it.
    floor: u64,
    /// The offset up to which the window's output has been acknowledged: drawn by a page, let go
    /// of by one, or settled at a subscription as handed to the page before. It never passes what
    /// a page was handed, so a page that subscribes gets a boundary at or past it. The journal's
    /// start never passes it, whatever asks (the cap, the sizes, the window's end): so no byte a
    /// page has yet to draw is below the start.
    acknowledged: u64,
}

/// A size the journal will take ([`Journal::size_slot`]).
pub(crate) struct SizeSlot<'a>(MutexGuard<'a, Inner>);

impl SizeSlot<'_> {
    /// The window took `size`, which the page's emulator took at `at` (`None`: the journal's
    /// end). See [`Journal`]'s sizes.
    pub(crate) fn record(mut self, at: Option<u64>, size: Size) {
        self.0.resize(at, size);
    }
}

/// One window's journal. Appended to by the window's consumer, read by the page's pulls.
pub struct Journal {
    inner: Mutex<Inner>,
    /// How far the consumer has published (handed to a page or parked), after the hub took it.
    published: AtomicU64,
}

impl Journal {
    pub fn new(size: Size) -> Self {
        let mut marks = VecDeque::new();
        marks.push_back(Mark {
            offset: 0,
            tracked: Tracked::default(),
        });
        Self {
            published: AtomicU64::new(0),
            inner: Mutex::new(Inner {
                segments: VecDeque::new(),
                first_segment: 0,
                spare: None,
                start: 0,
                end: 0,
                marks,
                tracker: Tracker::default(),
                resizes: Vec::new(),
                base_size: size,
                base_sized_at: None,
                floor: 0,
                acknowledged: 0,
            }),
        }
    }

    fn locked(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The offset up to which the consumer has published: the hub has taken every byte before
    /// it, to a page or to the parked.
    pub fn published_to(&self) -> u64 {
        self.published.load(Ordering::Acquire)
    }

    pub(crate) fn note_published(&self, end: u64) {
        self.published.store(end, Ordering::Release);
    }

    /// The offset a read taken now starts at.
    pub fn end(&self) -> u64 {
        self.locked().end
    }

    /// Copy a read in and track it. Nothing is allocated unless it starts a segment.
    pub(crate) fn append(&self, bytes: &[u8]) {
        self.locked().append(bytes);
    }

    /// A message has been taken in whole (the reads joined into it, all appended): a mark is
    /// taken at its end if one is due, and the journal is cut to its cap. Every mark is at a
    /// message's end: one a size makes is taken at the journal's end only where everything up to
    /// it is acknowledged, so no message is on its way across it.
    pub(crate) fn settle(&self) {
        self.locked().settle();
    }

    /// The window took a size, which the page's emulator took at `at` (the bytes it had
    /// parsed): `None` takes the journal's end. One before the last size taken, before the page's
    /// boundary or past the journal's end takes the end too; one before the journal's start takes
    /// the start. A size is kept whole: none is folded into another, and the margins a size resets
    /// are worked out from them in byte order with the program's own settings. The same size as
    /// the last is not kept.
    #[cfg(test)]
    pub(crate) fn resize(&self, at: Option<u64>, size: Size) {
        self.locked().resize(at, size);
    }

    /// Room for one more size, held from the check to the record: `None` with 4,096 sizes held
    /// and output unacknowledged, which no new start may pass. The journal stays locked while the
    /// slot is held, so no read is taken in between the check and the record, and what the check
    /// said still holds when the size is written.
    pub(crate) fn size_slot(&self) -> Option<SizeSlot<'_>> {
        let inner = self.locked();
        (inner.resizes.len() < HARD_MAX_RESIZES || inner.acknowledged >= inner.end)
            .then_some(SizeSlot(inner))
    }

    /// A page subscribed with `boundary` bytes already handed to a page (the hub's). Pulls
    /// stop there; a size taken after is not before it.
    pub(crate) fn begin_page(&self, boundary: u64) {
        let mut inner = self.locked();
        inner.floor = boundary.min(inner.end);
    }

    /// The window's output is acknowledged up to `offset`: the start never passes it
    /// (`Inner::acknowledged`).
    pub(crate) fn acknowledged(&self, offset: u64) {
        let mut inner = self.locked();
        inner.acknowledged = inner.acknowledged.max(offset);
    }

    #[cfg(all(test, unix))]
    pub(crate) fn acknowledged_to(&self) -> u64 {
        self.locked().acknowledged
    }

    /// The program ended and everything it printed is taken in: the journal keeps what a page
    /// that was behind can continue from, its last 1 MiB at least.
    pub(crate) fn finish(&self) {
        self.locked().trim_ended();
    }

    /// One piece of the history, from `from` (`None`: the page has nothing) up to the page's
    /// boundary.
    pub fn piece(&self, from: Option<u64>) -> Piece {
        self.locked().piece(from)
    }

    #[cfg(test)]
    fn look<T>(&self, look: impl FnOnce(&Inner) -> T) -> T {
        look(&self.locked())
    }
}

impl Inner {
    fn last_mark(&self) -> u64 {
        self.marks.back().map_or(0, |mark| mark.offset)
    }

    fn append(&mut self, mut bytes: &[u8]) {
        self.tracker.feed(bytes);
        while !bytes.is_empty() {
            let wanted = self.end / SEGMENT_BYTES as u64;
            if wanted >= self.first_segment + self.segments.len() as u64 {
                let segment = self
                    .spare
                    .take()
                    .unwrap_or_else(|| Vec::with_capacity(SEGMENT_BYTES));
                self.segments.push_back(segment);
            }
            let Some(tail) = self.segments.back_mut() else {
                return;
            };
            let take = (SEGMENT_BYTES - tail.len()).min(bytes.len());
            tail.extend_from_slice(&bytes[..take]);
            self.end += take as u64;
            bytes = &bytes[take..];
        }
    }

    fn settle(&mut self) {
        let since = self.end - self.last_mark();
        if (self.tracker.at_rest() && since >= MARK_AFTER) || since >= FORCE_MARK_AFTER {
            self.mark_at_end();
        }
        self.cut_to_cap();
    }

    /// A mark at the end of the journal, with the modes and margins as the tracker has them.
    fn mark_at_end(&mut self) {
        if self.end == 0 {
            return;
        }
        if self.last_mark() == self.end {
            self.marks.pop_back();
        }
        self.marks.push_back(Mark {
            offset: self.end,
            tracked: self.tracker.tracked().clone(),
        });
    }

    /// Past 8 segments the journal starts again at its first mark that leaves 8.
    fn cut_to_cap(&mut self) {
        if self.segments.len() <= MAX_SEGMENTS {
            return;
        }
        let keep_from = (self.first_segment + (self.segments.len() - MAX_SEGMENTS) as u64)
            * SEGMENT_BYTES as u64;
        if let Some(offset) = self
            .marks
            .iter()
            .map(|mark| mark.offset)
            .find(|offset| *offset >= keep_from)
        {
            self.cut_to(offset);
        }
    }

    /// A window that has ended keeps what a page that was behind can still continue from: its
    /// last 1 MiB, from the last mark at or before that (the journal's start where none is).
    /// The cut is never to the end itself: a page that was near it would find nothing.
    fn trim_ended(&mut self) {
        let target = self.end.saturating_sub(ENDED_KEPT_BYTES);
        if let Some(offset) = self
            .marks
            .iter()
            .map(|mark| mark.offset)
            .take_while(|offset| *offset <= target)
            .last()
        {
            self.cut_to(offset);
        }
    }

    /// The size the page's emulator has at `offset`: the sizes taken at or before it.
    fn size_at(&self, offset: u64) -> Size {
        self.resizes
            .iter()
            .take_while(|r| r.at <= offset)
            .last()
            .map_or(self.base_size, |r| (r.rows, r.cols))
    }

    /// The offset of the last size at or before `offset`.
    fn sized_at(&self, offset: u64) -> Option<u64> {
        self.resizes
            .iter()
            .take_while(|r| r.at <= offset)
            .last()
            .map(|r| r.at)
            .or(self.base_sized_at)
    }

    /// The margins a fresh emulator needs at the mark, for each screen: what the program set
    /// since the last size at or before it, which resets them in xterm (both screens, whether
    /// it changes the rows or only the width).
    fn margins_at(&self, mark: &Mark) -> (Vec<Region>, Vec<Region>) {
        let sized = self.sized_at(mark.offset);
        let rows = self.size_at(mark.offset).0;
        let kept = |set: &[Region]| {
            let since = set
                .iter()
                .filter(|region| sized.is_none_or(|sized| region.at > sized))
                .copied()
                .collect::<Vec<_>>();
            Region::kept_at(&since, rows)
                .into_iter()
                .collect::<Vec<_>>()
        };
        (kept(&mark.tracked.normal), kept(&mark.tracked.alt))
    }

    /// Start the journal at `offset`, a mark: the sizes before it are the size at it, and the
    /// margins it holds are worked out once, with every size that came before. The one place the
    /// start moves, and never past what is acknowledged.
    fn cut_to(&mut self, offset: u64) {
        if offset < self.start || offset > self.acknowledged {
            return;
        }
        if let Some(mark) = self.marks.iter().find(|mark| mark.offset == offset) {
            let (normal, alt) = self.margins_at(mark);
            if let Some(mark) = self.marks.iter_mut().find(|mark| mark.offset == offset) {
                mark.tracked.normal = normal;
                mark.tracked.alt = alt;
            }
        }
        while self.marks.front().is_some_and(|mark| mark.offset < offset) {
            self.marks.pop_front();
        }
        let folded = self.resizes.iter().take_while(|r| r.at <= offset).count();
        if folded > 0 {
            let last = self.resizes[folded - 1];
            self.base_sized_at = Some(last.at);
            self.base_size = (last.rows, last.cols);
        }
        self.resizes.drain(..folded);
        self.start = offset;
        while self.segments.len() > 1 && (self.first_segment + 1) * SEGMENT_BYTES as u64 <= offset {
            if let Some(mut freed) = self.segments.pop_front() {
                freed.clear();
                self.spare.get_or_insert(freed);
            }
            self.first_segment += 1;
        }
    }

    fn resize(&mut self, at: Option<u64>, size: Size) {
        let last = self.resizes.last().map_or(0, |r| r.at);
        let floor = self.floor.min(self.end);
        let at = match at {
            Some(at) if (last..=self.end).contains(&at) => at.max(self.start),
            _ => self.end,
        }
        .max(floor);
        if self.size_at(self.end) == size {
            return;
        }
        self.resizes.push(Resize {
            at,
            rows: size.0,
            cols: size.1,
        });
        if self.resizes.len() > MAX_RESIZES {
            self.start_again();
        }
    }

    /// Too many sizes to keep: the journal starts again at a mark at a rest point, never past
    /// what is acknowledged. Where everything is acknowledged that mark is the end, if it is at
    /// rest (past the hard bound, rest or not); else the last mark at or before what is
    /// acknowledged, which may be the start: then it keeps its sizes, and the next one tries
    /// again.
    fn start_again(&mut self) {
        if self.acknowledged >= self.end
            && (self.tracker.at_rest() || self.resizes.len() > HARD_MAX_RESIZES)
        {
            self.mark_at_end();
            self.cut_to(self.end);
        } else if let Some(offset) = self
            .marks
            .iter()
            .map(|mark| mark.offset)
            .take_while(|offset| *offset <= self.acknowledged)
            .last()
            .filter(|offset| *offset > self.start)
        {
            self.cut_to(offset);
        }
    }

    fn copy(&self, from: u64, to: u64) -> Vec<u8> {
        let mut bytes = Vec::with_capacity((to - from) as usize);
        let mut at = from;
        while at < to {
            let segment = (at / SEGMENT_BYTES as u64 - self.first_segment) as usize;
            let within = (at % SEGMENT_BYTES as u64) as usize;
            let data = &self.segments[segment];
            let take = (data.len() - within).min((to - at) as usize);
            bytes.extend_from_slice(&data[within..within + take]);
            at += take as u64;
        }
        bytes
    }

    fn piece(&mut self, from: Option<u64>) -> Piece {
        let boundary = self.floor.min(self.end);
        let held = from.filter(|from| (self.start..=self.end).contains(from));
        let reset = held.is_none();
        let from = held.unwrap_or(self.start);
        let to = boundary.min(from + PIECE_BYTES as u64).max(from);
        let truncated = self.start > 0;
        let seed = if reset && truncated {
            self.marks
                .iter()
                .find(|mark| mark.offset == self.start)
                .map(|mark| {
                    let (normal, alt) = self.margins_at(mark);
                    seed::build(&mark.tracked, &normal, &alt)
                })
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        Piece {
            reset,
            truncated,
            from,
            // A piece that starts the emulator clean is given the size at the journal's start
            // and every size taken since, those at its first byte too: xterm's tab stops
            // depend on the widths it has been (T-2126's gate). One that continues is given
            // the sizes after where the page is.
            size: if reset {
                self.base_size
            } else {
                self.size_at(from)
            },
            seed,
            resizes: self
                .resizes
                .iter()
                .filter(|r| (if reset { r.at >= from } else { r.at > from }) && r.at <= to)
                .copied()
                .collect(),
            bytes: self.copy(from, to),
            boundary,
            done: to >= boundary,
        }
    }
}
