//! What tells one dialog of Claude Code's from another when its words are read off the pane
//! host's tail twice (T-1277): [`Window::dialog_identity`] for a Claude Code window. The engine
//! holds two reads to one dialog by plain equality of what this returns, and presses a decline
//! only into a read whose identity equals the one that was told of.
//!
//! Three things are taken out of the words, and nothing else:
//!
//! - **The time in its warning line.** A permission prompt, a hook's ask, a classifier's ask draws
//!   under the request's words a line that says when Claude Code will block the request by
//!   itself, and redraws it every second: `⚠ Claude Code will automatically block this request
//!   in 1:57, to avoid holding up an unattended session` (2.1.319's own string, the time as
//!   `m:ss`, or `about 2 minutes` / `about 57 seconds` in its other mode, which changes its unit
//!   as it runs down). Claude Code draws it as the **last block above its question**, a block of
//!   its own with a margin under it (`wPe`, rendered after the request's words and the reason, and
//!   before the question), so it is known by its **place**: the rows of what the dialog says above
//!   its question, from the nearest that begins with the warning's words to the last, are that
//!   sentence and nothing else, whole, whichever way the screen wrapped it (joined by the spaces a
//!   wrap broke them at). Then they are one row of the identity, the time blanked. Any other
//!   shape keeps the words as they were read (T-1282's blocker): a command that prints the
//!   warning (a here-document), a hook's reason that quotes it, a sentence that is not whole or
//!   has a row after it are the request's own words, and another time there is another request.
//!   The reader drops what would tell them apart by their frame (the `│` of a box's rows), their
//!   indent and the blank lines of the screen; the order of the rows above the question is what
//!   this uses. When in doubt the words are kept: telling a dialog twice is the side to err on.
//! - **What is drawn above the LSP offer.** The offer's top is its own title, "LSP plugin
//!   recommendation" (2.1.317 to 2.1.319, as the live runs drew it). What the pane host's tail
//!   shows above that line is the conversation's, and it slides: the line that closes a turn
//!   appears there when the turn ends (T-1261, the release check of 3.0.7-alpha.2). The offer's
//!   identity runs from its title down. A read in which the tail cut the title off is another
//!   read: it is told as another dialog, and since the offer is on the list of safe declines it is
//!   declined at the next look that shows it so (over-telling, which is the side to err on).
//!
//! - **What is drawn above the Computer Use permission prompt** (T-1300). Its top is its title,
//!   "Computer Use needs macOS permissions", and only where its choices are the prompt's own
//!   (`computer_use_prompt`): the conversation above slid in the field (the notice of the dialog
//!   quoted the model's last words above the tool line), and a decline is pressed only into the
//!   dialog that was told of.
//!
//! **No row is cut on words alone** (T-1300, after T-1316's review of a first attempt). The field's
//! pair (T-1306's window, 2026-11-07, m-7776 at 1:57 and m-7780 at 0:05) was one prompt of a dangerous
//! `rm` told twice: the pane host's tail is twelve rows, and it slid two as rows below the question
//! changed, so the first read began at the row of the tool that waits (`⎿  Waiting…`) and the
//! title, and the second at the request. A first attempt cut every row up to a title Claude Code
//! draws ("Bash command", …) in the first four rows. It was wrong: the engine reads the tail with
//! its blank rows and its rules dropped and the frame of each row taken off (`shared::dialog`), so
//! a title is a row of words, and a command that prints "Bash command", or a hook's reason that is
//! "Bash command\n" (which Claude Code 2.1.319's own formatter draws as a bare title row), is a
//! title as far as the words go. The cut erased the command above it, two `rm` of different paths
//! became one identity, the second was never told and the first's pending "No" was pressed into
//! it. A row is cut only where the boundary is read from something a request's words cannot be:
//! the warning's place among the last rows above the question, and the choices of the Computer Use
//! prompt, which are read from the cursor rows and not from what the dialog says, and which has no
//! request of its own. Otherwise every row is kept, and a prompt whose tail slid is told again: an
//! extra notice is safe, a merged identity is not. The route to the pair is a boundary read from the
//! raw rows (the blank row and the rule the tail drops), not from their words.
//!
//! Claude Code's other dialogs have no known top, and their words are compared as they were read.
//! The note that tells a dialog still quotes the read as it was, the time in it.

use crate::contract::Dialog;

use super::adapter::LSP_OFFER;

/// The title that begins the offer to install an LSP plugin, as it is drawn.
const LSP_TITLE: &str = "LSP plugin recommendation";

/// The words a warning line begins with, as Claude Code 2.1.319 draws them: its warning sign
/// (U+26A0, no selector) and a space, then the sentence up to the time left, which follows after a
/// space.
const WARNING: &str = "\u{26a0} Claude Code will automatically block this request in";

/// What the sentence says after the time.
const AFTER_TIME: &str = ", to avoid holding up an unattended session";

/// The length of the time left at the start of `text`: `m:ss`, or `about N minute(s)` /
/// `about N second(s)`.
fn time_at(text: &str) -> Option<usize> {
    clock_at(text).or_else(|| spelled_at(text))
}

/// The length of a clock, `m:ss`, at the start of `text`.
fn clock_at(text: &str) -> Option<usize> {
    let minutes = text.bytes().take_while(u8::is_ascii_digit).count();
    let seconds = text[minutes..].strip_prefix(':')?.as_bytes();
    let two = seconds.len() >= 2 && seconds[..2].iter().all(u8::is_ascii_digit);
    (minutes > 0 && two).then_some(minutes + 3)
}

/// The length of `about N minute(s)` or `about N second(s)` at the start of `text`.
fn spelled_at(text: &str) -> Option<usize> {
    let after = text.strip_prefix("about ")?;
    let digits = after.bytes().take_while(u8::is_ascii_digit).count();
    let unit = after[digits..].strip_prefix(' ')?;
    let word = ["minutes", "minute", "seconds", "second"]
        .into_iter()
        .find(|word| unit.starts_with(word))?;
    (digits > 0).then_some("about ".len() + digits + 1 + word.len())
}

/// `detail`, the rows a Claude Code dialog says above its question, with its own warning, where
/// it is, as one row and its time blanked. The warning is the last block above the question: the
/// rows from the nearest that begins with its words to the end, joined by the spaces a wrap broke
/// them at, are its whole sentence and nothing else. Any other rows are returned as they were.
fn blanked(detail: &[String]) -> Vec<String> {
    let Some(start) = detail.iter().rposition(|row| row.starts_with(WARNING)) else {
        return detail.to_vec();
    };
    let line = detail[start..].join(" ");
    let whole = line
        .strip_prefix(WARNING)
        .and_then(|rest| rest.strip_prefix(' '))
        .and_then(|rest| time_at(rest).map(|length| &rest[length..]))
        .is_some_and(|after| after == AFTER_TIME);
    if !whole {
        return detail.to_vec();
    }
    let mut rows = detail[..start].to_vec();
    rows.push(format!("{WARNING} #{AFTER_TIME}"));
    rows
}

/// `dialog` as Claude Code's own window tells it from another read of its screen: the time in its
/// warning line blanked, and the offer to install an LSP plugin cut off above its title.
pub(crate) fn identity(dialog: &Dialog) -> Dialog {
    let mut detail = blanked(&dialog.detail);
    if dialog.question == LSP_OFFER {
        // The title nearest the question is the offer's own.
        if let Some(top) = detail.iter().rposition(|line| line == LSP_TITLE) {
            detail.drain(..top);
        }
    }
    // The Computer Use permission prompt begins at its title, and only where its choices are its
    // own (T-1300): what the tail shows above it is the conversation's, and it slides.
    if let Some(top) = super::computer_use_prompt::top(dialog) {
        detail.drain(..top.min(detail.len()));
    }
    Dialog {
        detail,
        ..dialog.clone()
    }
}

#[cfg(test)]
mod tests;
