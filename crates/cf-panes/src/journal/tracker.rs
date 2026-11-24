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

/// A scroll region the program set that xterm may have taken: the first and last row from 1 as
/// it wrote them (`usize::MAX` for no last row: the screen's), and the offset at which the
/// sequence ended. xterm takes a region only when its top is above its bottom, the bottom being
/// the screen's last row where it is missing, zero or past it, and it keeps the region it had
/// when it refuses one. The host does not know the screen's rows when it reads a setting (a size
/// reaches it late), so a screen keeps the settings that some number of rows would take and no
/// later setting supersedes: their tops rise from the oldest to the newest. The one the page's
/// xterm keeps at a mark is the newest whose top is above the rows there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Region {
    pub(crate) at: u64,
    pub(crate) top: usize,
    pub(crate) bottom: usize,
}

/// The rows at which a setting is not taken whatever the screen: a top this far down is
/// refused by every screen the page can make (rows are a `u16`, and a window's sizes are
/// bounded far below it), so it is not kept.
pub(crate) const MAX_TOP: usize = 1024;

impl Region {
    /// The region xterm holds after these settings (of one screen, since its margins were last
    /// the whole screen) on a screen of `rows` rows: the newest it takes.
    pub(crate) fn kept_at(set: &[Region], rows: u16) -> Option<Region> {
        set.iter()
            .rev()
            .find(|region| region.top < usize::from(rows))
            .copied()
    }

    /// Take in a setting that xterm takes at some number of rows: the ones it supersedes
    /// (those it takes wherever it takes them) are dropped.
    fn push(set: &mut Vec<Region>, region: Region) {
        while set.last().is_some_and(|last| last.top >= region.top) {
            set.pop();
        }
        set.push(region);
    }
}

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
            // xterm's `setScrollRegion`: top is the first parameter or 1; the bottom is the
            // screen's last row where it is missing, zero or past the screen; the region is
            // taken only when the bottom is below the top. Whether a bottom is past the screen
            // only the screen's rows tell, so a setting is kept as written (see `Region`).
            let (top, bottom) = match params {
                [] => (1, usize::MAX),
                [top] => (*top, usize::MAX),
                [top, bottom, ..] => (*top, if *bottom == 0 { usize::MAX } else { *bottom }),
            };
            let top = top.max(1);
            // One whose top is not above its bottom is refused at every screen. One whose top is
            // as far down as `MAX_TOP` is refused at every screen a page makes.
            if top < bottom && top < MAX_TOP {
                let shown = tracked.shown();
                if top == 1 && bottom == usize::MAX {
                    // The whole screen: taken at every screen of two rows or more, and a screen
                    // of one row refuses every setting.
                    shown.clear();
                } else {
                    Region::push(
                        shown,
                        Region {
                            at: offset,
                            top,
                            bottom,
                        },
                    );
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
