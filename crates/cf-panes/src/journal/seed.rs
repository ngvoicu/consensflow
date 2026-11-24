//! The bytes that give a fresh emulator what the host tracks reliably of a window at a mark:
//! the modes and the scroll margins. No text and no cursor: the host does not draw the screen.
//!
//! The order is the design's: the alternate screen if it shows; the margins; the modes that
//! are not a fresh xterm's; the cursor shown or hidden. Wrap and origin are written when they
//! are not xterm's own.

use std::fmt::Write;

use super::tracker::Tracked;

pub(super) fn build(tracked: &Tracked, margins: Option<(usize, usize)>) -> Vec<u8> {
    let mut out = String::new();
    if tracked.modes.alternate {
        out.push_str("\x1b[?47h");
    }
    if let Some((top, bottom)) = margins {
        if bottom == usize::MAX {
            let _ = write!(out, "\x1b[{}r", top.max(1));
        } else {
            let _ = write!(out, "\x1b[{};{bottom}r", top.max(1));
        }
    }
    tracked.modes.write_to(&mut out);
    if tracked.modes.origin {
        out.push_str("\x1b[?6h");
    }
    if !tracked.modes.wrap {
        out.push_str("\x1b[?7l");
    }
    if tracked.modes.insert {
        out.push_str("\x1b[4h");
    }
    if !tracked.modes.cursor_shown {
        out.push_str("\x1b[?25l");
    }
    out.into_bytes()
}
