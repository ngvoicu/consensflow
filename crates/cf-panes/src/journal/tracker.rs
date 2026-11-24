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

/// A scroll region the program set: the first and last row from 1 as it wrote them (`usize::MAX`
/// for no last row: the screen's), and the offset at which the sequence ended. The host does not
/// know the screen's rows, so it keeps the settings made since the screen's margins were last
/// the whole screen, and the page's xterm refuses at its own rows what the old one refused. A
/// size taken by the page resets the margins of both screens at its own offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Region {
    pub(crate) at: u64,
    pub(crate) top: usize,
    pub(crate) bottom: usize,
}

/// The most settings kept for a screen.
const MAX_REGIONS: usize = 16;

/// What the tracker follows: xterm keeps margins for each screen, normal and alternate.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Tracked {
    pub(crate) modes: Modes,
    pub(crate) normal: Vec<Region>,
    pub(crate) alt: Vec<Region>,
}

impl Tracked {
    fn shown(&mut self) -> &mut Vec<Region> {
        if self.modes.alternate {
            &mut self.alt
        } else {
            &mut self.normal
        }
    }
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
        } => *tracked = Tracked::default(),
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
            let was = tracked.modes.alternate;
            for &mode in params {
                tracked.modes.private(mode, last == b'h');
            }
            if was != tracked.modes.alternate {
                // The alternate screen starts with the whole screen and drops its margins as
                // it is left.
                tracked.alt.clear();
            }
        }
        (None, None, b'h' | b'l') => {
            if params.contains(&4) {
                tracked.modes.insert = last == b'h';
            }
        }
        (None, None, b'r') => {
            // A region whose top is not above its bottom is refused, and the margins stay, as
            // xterm keeps them; one with no bottom or past the screen is the screen's, which
            // only the screen's rows can tell (so it is kept as written).
            let (top, bottom) = match params {
                [] => (1, usize::MAX),
                [top] => (*top, usize::MAX),
                [top, bottom, ..] => (*top, if *bottom == 0 { usize::MAX } else { *bottom }),
            };
            let top = top.max(1);
            if top < bottom {
                let shown = tracked.shown();
                if top == 1 && bottom == usize::MAX {
                    shown.clear();
                } else {
                    if shown.len() == MAX_REGIONS {
                        shown.remove(0);
                    }
                    shown.push(Region {
                        at: offset,
                        top,
                        bottom,
                    });
                }
            }
        }
        (None, Some(b'!'), b'p') => {
            tracked.modes.soft_reset();
            tracked.shown().clear();
        }
        _ => {}
    }
}
