//! Claude Code's input box, drawn as Claude Code 2.1.308 draws it, for the window
//! of a participant `CF_TEST_INPUT_BOX` names: a rule, `❯` and what is typed, a rule,
//! and the permission mode under it, drawn again whenever what is typed changes.
//! `/model` sent opens its model picker, a list whose filter takes what is typed;
//! Escape closes it, and the box is drawn again, empty, though the keys typed into
//! the filter were never sent. Escape at the box keeps what is typed in it, as
//! Claude's does. Enter in the picker closes it too, with no choice taken.

use std::io;

use super::terminal::Keys;

/// What opens the picker.
const PICKER: &str = "/model";

/// A rule across the screen.
const RULE: &str = "────────────────────────────────────────────────────────────";

/// The box, and the picker that may be open over it.
#[derive(Debug, Default)]
pub struct InputBox {
    picker: bool,
    /// What was drawn last: whether the picker was, and what was typed.
    drawn: Option<(bool, String)>,
}

impl InputBox {
    /// Whether the box takes `sent` itself, so that it is no message: `/model`
    /// opens the picker, and what Enter sends while the picker is open closes it.
    pub fn takes(&mut self, sent: &str) -> bool {
        if self.picker {
            self.picker = false;
            return true;
        }
        self.picker = sent.trim() == PICKER;
        self.picker
    }

    /// Whether Escape closed the picker, dropping what its filter took; at the box,
    /// it is the key Claude's turn takes.
    pub fn escape(&mut self, keys: &mut Keys) -> bool {
        if !std::mem::take(&mut self.picker) {
            return false;
        }
        keys.clear();
        true
    }

    /// Draws the box, or the picker, with `typed` in it, where it changed.
    pub fn draw(&mut self, typed: &str, say: impl Fn(&str) -> io::Result<()>) -> io::Result<()> {
        let now = (self.picker, typed.to_owned());
        if self.drawn.as_ref() == Some(&now) {
            return Ok(());
        }
        let shown = if self.picker {
            format!(
                "\r\n{RULE}\r\n Select model\r\n / {typed}\r\n ❯ 1. Default (recommended)\r\n   2. Opus\r\n Enter to confirm · Esc to exit\r\n"
            )
        } else {
            format!("\r\n{RULE}\r\n❯ {typed}\r\n{RULE}\r\n  ⏵⏵ bypass permissions on (shift+tab to cycle)\r\n")
        };
        self.drawn = Some(now);
        say(&shown)
    }
}
