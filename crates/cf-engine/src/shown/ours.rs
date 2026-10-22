//! The lines of a window's screen that ConsensFlow typed into it: its pastes.
//! They are no reason of the window's, and a note neither quotes nor compares
//! them (Calliope's reviews of the failure notes, T-272 and T-279).
//!
//! A paste is one run of lines, in the order of its text: it begins at a line
//! that begins with its header (`[ConsensFlow m-<id> …`, after whatever frame
//! the harness draws around a prompt), and goes on while each line with a word
//! in it is the next piece of what was pasted (its header, its words, the
//! labels of the rows it carried), however the window wrapped it. The first line
//! that is not ends it: that line, and every line after it, is the window's,
//! whatever words it repeats (a provider's error a note already quoted is still
//! the window's own). A line that is only a frame belongs to no one and ends
//! nothing.
//!
//! Compared as Windows' console carries a paste to Devin (`console_text`: its
//! `·` arrives as `|`, its dashes as `--`), and with blanks made one.
//!
//! A paste whose header has scrolled off the top of what a look reads (twelve
//! lines) still holds its words: where no header is in view, the run of the
//! newest paste may begin at the first line with a word, and only there, so a
//! line lower down that repeats a paste's words is still the window's.
//!
//! What was pasted is the window's five last pastes, on their way or arrived,
//! each with the rows it carried. A message taken back to its queue, or
//! cancelled while it was typed, is left out, though its text was on the screen:
//! it is told apart from a message never pasted by nothing the ledger keeps
//! (its attempt is given back where the harness refused it), and a queue's
//! backlog would crowd out what is shown. It meets a failed turn's look rarely:
//! a message is taken back when its window refused it for quota or went, and a
//! task cancelled mid-paste stops its window. Its header, where it shows, is
//! still the paste's.
//!
//! A harness may show a paste as a placeholder: `[Pasted text #1 +42 lines]` in
//! Claude Code 2.1.308 and Devin 3000.11.92, `[Pasted Content 1200 chars]` in
//! Codex 0.163.0. Whatever pasted it, a placeholder is no reason.

use cf_base::text::console_text;

use crate::delivery_text::HEADER_START;

/// What a harness writes in place of a paste it folds away.
const PLACEHOLDERS: [&str; 2] = ["[Pasted text #", "[Pasted Content "];

/// A message as it was pasted into the window: its id and its whole text.
pub(crate) struct Paste {
    pub(crate) id: i64,
    pub(crate) text: String,
}

/// Whose a line of the screen is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Owner {
    /// The window's own.
    Window,
    /// A run of the paste at this place of the pastes, the newest first.
    Paste(usize),
    /// ConsensFlow's, of no paste the pastes hold: a header of an older one, or a
    /// placeholder a harness wrote for one.
    Pasted,
}

/// Which of the screen's `lines` are ConsensFlow's, of the `pastes` it typed
/// into the window, the newest first: what the live suite holds a screen to.
#[cfg(any(test, feature = "test-support"))]
pub(crate) fn ours(lines: &[String], pastes: &[Paste]) -> Vec<bool> {
    owners(lines, pastes)
        .into_iter()
        .map(|owner| owner != Owner::Window)
        .collect()
}

/// Whose each of the screen's `lines` is, of the `pastes` ConsensFlow typed into
/// the window, the newest first.
///
/// Known limit (Calliope's re-check T-300, P3-R3): a line of the paste's body that
/// begins with another message's header counts as a header in view, so a newest
/// paste whose own header scrolled off and whose visible body quotes one at a
/// line's start is not run from the top line, and its lines are the window's.
pub(crate) fn owners(lines: &[String], pastes: &[Paste]) -> Vec<Owner> {
    let pasted: Vec<(i64, String)> = pastes
        .iter()
        .map(|paste| (paste.id, compared(&paste.text)))
        .collect();
    let unframed: Vec<&str> = lines.iter().map(|line| unframed(line)).collect();
    // The paste whose run the lines are in, and what is left of it. Where no header
    // is in view, the newest paste's may have scrolled off above its words: its run
    // may begin at the first line with a word, and nowhere else.
    let mut run: Option<(usize, &str)> = None;
    let mut top = !unframed
        .iter()
        .any(|line| starts_with_header(line).is_some());
    unframed
        .iter()
        .map(|&unframed| {
            let words = compared(unframed);
            if top && !words.is_empty() {
                top = false;
                run = pasted.first().map(|(_, newest)| (0, newest.as_str()));
            }
            // The next piece of the run is the run's, whatever it looks like.
            if let Some((at, rest)) = run.and_then(|(at, rest)| Some((at, after(rest, &words)?))) {
                run = Some((at, rest));
                return Owner::Paste(at);
            }
            if let Some(id) = starts_with_header(unframed) {
                let found = pasted.iter().position(|(pasted, _)| *pasted == id);
                run = found.map(|at| (at, rest_after(&pasted[at].1, &words)));
                return found.map_or(Owner::Pasted, Owner::Paste);
            }
            if PLACEHOLDERS.iter().any(|start| unframed.starts_with(start)) {
                // A placeholder stands for a paste whose words are not on the screen.
                run = None;
                return Owner::Pasted;
            }
            if !words.is_empty() {
                run = None;
            }
            Owner::Window
        })
        .collect()
}

/// What of `rest` comes after `words` where they are a piece of it: none where
/// they are not, or are no words.
fn after<'a>(rest: &'a str, words: &str) -> Option<&'a str> {
    if words.is_empty() {
        return None;
    }
    rest.find(words).map(|at| &rest[at + words.len()..])
}

/// `line` without the frame a harness draws around a prompt: its marks before
/// the first word or bracket, and after the last word, a box's edge drawn in
/// ASCII (` |`) among them.
fn unframed(line: &str) -> &str {
    let framed = |c: char| c.is_whitespace() || ('\u{2500}'..='\u{259F}').contains(&c);
    let line = line
        .trim_start_matches(|c: char| !(c.is_alphanumeric() || c == '[' || c == '('))
        .trim_end_matches(framed);
    match line.strip_suffix('|') {
        Some(edge) if edge.ends_with(char::is_whitespace) => edge.trim_end_matches(framed),
        _ => line,
    }
}

/// The id a line that begins with a paste's header names (one that only holds
/// a header elsewhere names none).
fn starts_with_header(line: &str) -> Option<i64> {
    let digits: String = line
        .strip_prefix(HEADER_START)?
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().ok()
}

/// What of `text` comes after its first `piece`: all of it where it has none.
fn rest_after<'a>(text: &'a str, piece: &str) -> &'a str {
    text.find(piece)
        .map_or(text, |at| &text[at + piece.len()..])
}

/// `text` as it is compared: as the console carries it, its blanks made one.
fn compared(text: &str) -> String {
    console_text(text)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests;
