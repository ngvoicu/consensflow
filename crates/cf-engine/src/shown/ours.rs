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

/// Which of the screen's `lines` are ConsensFlow's, of the `pastes` it typed
/// into the window.
pub(crate) fn ours(lines: &[String], pastes: &[Paste]) -> Vec<bool> {
    let pasted: Vec<(i64, String)> = pastes
        .iter()
        .map(|paste| (paste.id, compared(&paste.text)))
        .collect();
    // What is left of the paste whose run the lines are in.
    let mut run: Option<&str> = None;
    lines
        .iter()
        .map(|line| {
            let unframed = unframed(line);
            if let Some(id) = header_id(unframed) {
                run = pasted
                    .iter()
                    .find(|(pasted, _)| *pasted == id)
                    .map(|(_, text)| rest_after(text, &compared(unframed)));
                return true;
            }
            if PLACEHOLDERS.iter().any(|start| unframed.starts_with(start)) {
                return true;
            }
            let words = compared(unframed);
            if words.is_empty() {
                return false;
            }
            match run.and_then(|rest| rest.find(&words).map(|at| &rest[at + words.len()..])) {
                Some(rest) => {
                    run = Some(rest);
                    true
                }
                None => {
                    run = None;
                    false
                }
            }
        })
        .collect()
}

/// `line` without the frame a harness draws around a prompt: its marks before
/// the first word or bracket, and after the last word.
fn unframed(line: &str) -> &str {
    line.trim_start_matches(|c: char| !(c.is_alphanumeric() || c == '[' || c == '('))
        .trim_end_matches(|c: char| c.is_whitespace() || ('\u{2500}'..='\u{259F}').contains(&c))
}

/// The id a line that begins with a paste's header names.
fn header_id(line: &str) -> Option<i64> {
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
