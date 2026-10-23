//! Claude Code's input box, drawn as Claude Code 2.1.308 draws it, for the window
//! of a participant `CF_TEST_INPUT_BOX` names: a rule, `❯` and what is typed, a rule,
//! and the permission mode under it, drawn again whenever what is typed changes, the
//! cursor put back after what is typed and shown, as Claude Code 2.1.309 puts it
//! (`cargo xtask live escape-unsent`). `/model` sent opens its model picker, a list
//! whose filter takes what is typed; Escape closes it, and the box is drawn again,
//! empty, though the keys typed into the filter were never sent. Escape at the box
//! keeps what is typed in it, as Claude's does. Enter in the picker closes it too,
//! with no choice taken.
//!
//! The window of a participant `CF_TEST_SUGGESTION` names shows a suggestion in its
//! empty box, faint, the cursor before it, as Claude Code does in a conversation
//! with nothing said yet. That of a participant `CF_TEST_SLOW_DRAW` names draws its
//! box only [`SLOW_DRAW`] after it last changed: a key not drawn yet, as a harness
//! that lags draws it.

use std::io;
use std::time::{Duration, Instant};

use super::terminal::Keys;

/// What opens the picker.
const PICKER: &str = "/model";

/// The suggestion an empty box shows.
const SUGGESTION: &str = "Try \"fix lint errors\"";

/// How long a window that draws slowly takes to draw its box once it changed:
/// longer than the second ConsensFlow waits after the human's last key, shorter
/// than the five it waits for one the window draws nothing for.
const SLOW_DRAW: Duration = Duration::from_secs(3);

/// How many lines below the `❯` line the cursor is once the box is drawn: the rule,
/// the permission mode, and the line after it.
const BELOW: usize = 3;

/// A rule across the screen.
const RULE: &str = "────────────────────────────────────────────────────────────";

/// The box, and the picker that may be open over it.
#[derive(Debug, Default)]
pub struct InputBox {
    picker: bool,
    /// Whether an empty box shows the suggestion.
    suggests: bool,
    /// Whether the box is drawn [`SLOW_DRAW`] after it changed.
    slow: bool,
    /// What was drawn last: whether the picker was, and what was typed.
    drawn: Option<(bool, String)>,
    /// What is to be drawn, and since when, for a window that draws slowly.
    due: Option<((bool, String), Instant)>,
    /// Whether the cursor was put back on the `❯` line.
    parked: bool,
}

impl InputBox {
    /// A box that shows the suggestion when empty where `suggests`, and is drawn
    /// slowly where `slow`.
    pub fn new(suggests: bool, slow: bool) -> Self {
        Self {
            suggests,
            slow,
            ..Self::default()
        }
    }

    /// What is written before anything else the window prints: the cursor back
    /// below the box, where it was put back on the `❯` line.
    pub fn below(&mut self) -> &'static str {
        if std::mem::take(&mut self.parked) {
            "\x1b[3B\r"
        } else {
            ""
        }
    }

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

    /// Draws the box, or the picker, with `typed` in it, where it changed (and,
    /// for a window that draws slowly, [`SLOW_DRAW`] after it last did).
    pub fn draw(&mut self, typed: &str, say: impl Fn(&str) -> io::Result<()>) -> io::Result<()> {
        let now = (self.picker, typed.to_owned());
        if self.drawn.as_ref() == Some(&now) {
            self.due = None;
            return Ok(());
        }
        if self.slow {
            match &self.due {
                Some((due, since)) if *due == now && since.elapsed() >= SLOW_DRAW => {}
                Some((due, _)) if *due == now => return Ok(()),
                _ => {
                    self.due = Some((now, Instant::now()));
                    return Ok(());
                }
            }
        }
        let below = self.below();
        let shown = if self.picker {
            format!(
                "{below}\r\n{RULE}\r\n Select model\r\n / {typed}\r\n ❯ 1. Default (recommended)\r\n   2. Opus\r\n Enter to confirm · Esc to exit\r\n"
            )
        } else {
            // What is typed, or the suggestion, faint, in an empty box that shows one;
            // then the cursor put back after what is typed, and shown.
            let text = if typed.is_empty() && self.suggests {
                format!("\x1b[2m{SUGGESTION}\x1b[22m")
            } else {
                typed.to_owned()
            };
            self.parked = true;
            format!(
                "{below}\r\n{RULE}\r\n❯ {text}\r\n{RULE}\r\n  ⏵⏵ bypass permissions on (shift+tab to cycle)\r\n\x1b[{BELOW}A\x1b[{}G\x1b[?25h",
                typed.chars().count() + 3
            )
        };
        (self.drawn, self.due) = (Some(now), None);
        say(&shown)
    }
}
