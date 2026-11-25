//! The modes of a window's terminal that a page's xterm must have to send what the window's
//! program expects: a paste in brackets, an arrow in the program's own form, the mouse in its
//! protocol. Followed from the bytes, apart from the engine's screen, so that a replay that
//! starts mid-journal (a mark) can be given them.
//!
//! From `wt/d4-p5b` (`afdf855e8`, `screen/grid/modes.rs`), with the alternate screen, the
//! cursor shown and the scroll margins added.

use std::fmt::Write;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Modes {
    pub(crate) paste: bool,
    pub(crate) cursor_keys: bool,
    pub(crate) keypad: bool,
    pub(crate) focus: bool,
    pub(crate) mouse: Option<usize>,
    pub(crate) mouse_encoding: Option<usize>,
    pub(crate) mouse_utf8: bool,
    pub(crate) mouse_urxvt: bool,
    pub(crate) wrap: bool,
    pub(crate) origin: bool,
    pub(crate) insert: bool,
    pub(crate) cursor_shown: bool,
    pub(crate) alternate: bool,
}

impl Default for Modes {
    fn default() -> Self {
        Self {
            paste: false,
            cursor_keys: false,
            keypad: false,
            focus: false,
            mouse: None,
            mouse_encoding: None,
            mouse_utf8: false,
            mouse_urxvt: false,
            wrap: true,
            origin: false,
            insert: false,
            cursor_shown: true,
            alternate: false,
        }
    }
}

impl Modes {
    /// A soft reset (`CSI ! p`) as the shipped xterm does it: the mouse and the screen that
    /// shows stay.
    pub(crate) fn soft_reset(&mut self) {
        let kept = Self {
            mouse: self.mouse,
            mouse_encoding: self.mouse_encoding,
            mouse_utf8: self.mouse_utf8,
            mouse_urxvt: self.mouse_urxvt,
            alternate: self.alternate,
            ..Self::default()
        };
        *self = kept;
    }

    pub(crate) fn private(&mut self, mode: usize, on: bool) {
        match mode {
            1 => self.cursor_keys = on,
            6 => self.origin = on,
            7 => self.wrap = on,
            25 => self.cursor_shown = on,
            47 | 1047 | 1049 => self.alternate = on,
            66 => self.keypad = on,
            1004 => self.focus = on,
            2004 => self.paste = on,
            9 | 1000 | 1002 | 1003 => Self::exclusive(&mut self.mouse, mode, on),
            1005 => self.mouse_utf8 = on,
            1015 => self.mouse_urxvt = on,
            1006 | 1016 => Self::exclusive(&mut self.mouse_encoding, mode, on),
            _ => {}
        }
    }

    fn exclusive(current: &mut Option<usize>, mode: usize, on: bool) {
        *current = on.then_some(mode);
    }

    /// The modes that are not a fresh xterm's, as the bytes that set them. Not the screen
    /// that shows, the cursor, or the wrap (the seed's own order sets those).
    pub(crate) fn write_to(&self, out: &mut String) {
        for (mode, on) in [
            (1, self.cursor_keys),
            (66, self.keypad),
            (1004, self.focus),
            (2004, self.paste),
            (1005, self.mouse_utf8),
            (1015, self.mouse_urxvt),
        ] {
            if on {
                let _ = write!(out, "\x1b[?{mode}h");
            }
        }
        for mode in [self.mouse, self.mouse_encoding].into_iter().flatten() {
            let _ = write!(out, "\x1b[?{mode}h");
        }
    }
}
