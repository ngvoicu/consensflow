//! The bytes that give a fresh emulator what the host tracks reliably of a window at a mark:
//! the modes and the scroll margins of each screen. No text and no cursor: the host does not
//! draw the screen.
//!
//! The order is the design's: the normal screen's margins; the alternate screen, if it shows,
//! and its margins; the modes that are not a fresh xterm's; the cursor shown or hidden. Wrap and
//! origin are written when they are not xterm's own.

use std::fmt::Write;

use super::tracker::{Region, Tracked};

fn regions(out: &mut String, set: &[Region]) {
    for region in set {
        if region.bottom == usize::MAX {
            let _ = write!(out, "\x1b[{}r", region.top);
        } else {
            let _ = write!(out, "\x1b[{};{}r", region.top, region.bottom);
        }
    }
}

pub(super) fn build(tracked: &Tracked, normal: &[Region], alt: &[Region]) -> Vec<u8> {
    let mut out = String::new();
    regions(&mut out, normal);
    if tracked.modes.alternate {
        out.push_str("\x1b[?47h");
        regions(&mut out, alt);
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
