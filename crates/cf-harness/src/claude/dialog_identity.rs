//! What tells one dialog of Claude Code's from another when its words are read off the pane
//! host's tail twice (T-1277): [`Window::dialog_identity`] for a Claude Code window. The engine
//! holds two reads to one dialog by plain equality of what this returns, and presses a decline
//! only into a read whose identity equals the one that was told of.
//!
//! Two things are taken out of the words, and nothing else:
//!
//! - **The time in its warning line.** A permission prompt, a hook's ask, a classifier's ask draws
//!   under the request's words a line that says when Claude Code will block the request by
//!   itself, and redraws it every second: `⚠ Claude Code will automatically block this request
//!   in 1:57, to avoid holding up an unattended session` (2.1.319's own string, the time as
//!   `m:ss`, or `about 2 minutes` / `about 57 seconds` in its other mode, which changes its unit
//!   as it runs down). Only the time in that line is blanked, and the line is known by the words
//!   it begins with, glyph and all. Where the screen wraps it before its time (a row that ends
//!   `… this request in`), the time is the first thing of the row under it, and that row is read
//!   as the same line, because the row above it is that warning. A row that merely looks like it
//!   (a hook's reason that begins `about 2 minutes, to avoid …`, T-1273's finding 7) is no such
//!   row when the warning is not the row above it. A command, a hook's reason, a question that
//!   says a time (`printf 'retry in 5s'`, `Retry in 5s?`) is the request, and another time there
//!   is another request.
//! - **What is drawn above the LSP offer.** The offer's top is its own title, "LSP plugin
//!   recommendation" (2.1.317 to 2.1.319, as the live runs drew it). What the pane host's tail
//!   shows above that line is the conversation's, and it slides: the line that closes a turn
//!   appears there when the turn ends (T-1261, the release check of 3.0.7-alpha.2). The offer's
//!   identity runs from its title down. A read in which the tail cut the title off is another
//!   read: it is told as another dialog, and since the offer is on the list of safe declines it is
//!   declined at the next look that shows it so (over-telling, which is the side to err on).
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

/// The length of the time left at the start of `text`, where one begins it and ends it, or goes
/// on with the comma that follows it in Claude Code's sentence: `m:ss`, or `about N minute(s)` /
/// `about N second(s)`.
fn time_at(text: &str) -> Option<usize> {
    let length = clock_at(text).or_else(|| spelled_at(text))?;
    matches!(text[length..].chars().next(), None | Some(',')).then_some(length)
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

/// `detail`, the lines a Claude Code dialog says above its question, with the time of its warning
/// line blanked wherever that line is.
fn blanked(detail: &[String]) -> Vec<String> {
    // Whether the row above is the warning, wrapped before its time.
    let mut wrapped = false;
    detail
        .iter()
        .map(|row| {
            let after_warning = std::mem::take(&mut wrapped);
            if after_warning {
                if let Some(length) = time_at(row) {
                    return format!("#{}", &row[length..]);
                }
            }
            let Some(rest) = row.strip_prefix(WARNING) else {
                return row.clone();
            };
            if rest.is_empty() {
                wrapped = true;
                return row.clone();
            }
            match rest
                .strip_prefix(' ')
                .and_then(|time| time_at(time).map(|length| (time, length)))
            {
                Some((time, length)) => format!("{WARNING} #{}", &time[length..]),
                None => row.clone(),
            }
        })
        .collect()
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
    Dialog {
        detail,
        ..dialog.clone()
    }
}

#[cfg(test)]
mod tests;
