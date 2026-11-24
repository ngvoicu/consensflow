//! One pass over each read of a window's output, apart from the engine's screen: whether the
//! read ends at a rest point (the parser of xterm in its ground state, between whole
//! characters), the modes the program has set, and where it last set the scroll margins.
//!
//! Its parser is `wt/d4-p5b`'s (`afdf855e8`, `screen/parser.rs`): xterm's states, UTF-8
//! decoded before the controls, C1 and ST as xterm takes them. It is not the engine's
//! parser, which stays main's. It answers nothing: the page's xterm answers the program's
//! questions.

use super::modes::Modes;
use super::parser::{Action, Parser};

/// The scroll margins as the program last set them, first and last row from 1, and the
/// offset at which that sequence ended. A size taken by the page resets the margins at its
/// own offset, so what the margins are at a point depends on which of the two came last
/// ([`super::Journal`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct MarginSet {
    pub(crate) at: u64,
    pub(crate) value: Option<(usize, usize)>,
}

/// What the tracker follows.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Tracked {
    pub(crate) modes: Modes,
    pub(crate) margins: MarginSet,
}

#[derive(Debug, Default)]
pub(crate) struct Tracker {
    parser: Parser,
    tracked: Tracked,
    /// The bytes taken in so far.
    offset: u64,
}

impl Tracker {
    /// Take in a read.
    pub(crate) fn feed(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.offset += 1;
            let (tracked, offset) = (&mut self.tracked, self.offset);
            self.parser.advance(byte, &mut |action| {
                perform(tracked, offset, &action);
            });
        }
    }

    /// Whether the bytes so far end between whole characters, outside every sequence.
    pub(crate) fn at_rest(&self) -> bool {
        self.parser.at_rest()
    }

    pub(crate) fn tracked(&self) -> &Tracked {
        &self.tracked
    }
}

fn perform(tracked: &mut Tracked, offset: u64, action: &Action) {
    match action {
        Action::Escape {
            intermediate: None,
            last: b'c',
        } => {
            *tracked = Tracked::default();
            tracked.margins.at = offset;
        }
        Action::Escape {
            intermediate: None,
            last: b'=',
        } => tracked.modes.keypad = true,
        Action::Escape {
            intermediate: None,
            last: b'>',
        } => tracked.modes.keypad = false,
        Action::Csi {
            marker,
            params,
            intermediate,
            last,
        } => csi(tracked, offset, *marker, params, *intermediate, *last),
        _ => {}
    }
}

fn csi(
    tracked: &mut Tracked,
    offset: u64,
    marker: Option<u8>,
    params: &[usize],
    intermediate: Option<u8>,
    last: u8,
) {
    match (marker, intermediate, last) {
        (Some(b'?'), None, b'h' | b'l') => {
            for &mode in params {
                tracked.modes.private(mode, last == b'h');
            }
        }
        (None, None, b'h' | b'l') => {
            if params.contains(&4) {
                tracked.modes.insert = last == b'h';
            }
        }
        (None, None, b'r') => {
            // A region that is not one (the top at or below the bottom) is refused, and the
            // margins stay, as xterm keeps them.
            let region = match params {
                [] => Some(None),
                [top] => Some(Some((*top, usize::MAX))),
                [top, bottom, ..] => {
                    let bottom = if *bottom == 0 { usize::MAX } else { *bottom };
                    (top.max(&1) < &bottom).then_some(Some((*top, bottom)))
                }
            };
            if let Some(value) = region {
                tracked.margins = MarginSet { at: offset, value };
            }
        }
        (None, Some(b'!'), b'p') => {
            tracked.modes.soft_reset();
            tracked.margins = MarginSet {
                at: offset,
                value: None,
            };
        }
        _ => {}
    }
}
