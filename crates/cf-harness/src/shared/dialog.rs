//! A dialog of a harness's own that waits for a key, read off a window's screen by its shape:
//! a list of choices, one of them marked by the harness's cursor (`❯`, `›`, `▶`, `→`, `>`), the
//! others aligned with it or numbered after it, and above them the words it opens with, a
//! question where it asks one. Claude Code 2.1.317 draws its offers so ("LSP plugin
//! recommendation", "Would you like to install this LSP plugin?", `❯ Yes, install` and three
//! more), as it draws its permission prompts, its usage-limit choices and its surveys; Codex
//! 0.166 draws its own with `›` and numbers. None of them is in the conversation's record, and
//! only the screen shows them (T-1183).
//!
//! The reader names what it found and leaves the judging to its caller: the lines below the
//! choices ([`Shown::below`]: where the harness's prompt is drawn there, the choices are a
//! conversation's words, or the box's own text), and whether it waits for an answer
//! ([`Dialog::asks`]): its words above ask, or a hint below the choices says how to answer them
//! (Devin's dialog of a call asks nothing above its list, T-1192). A screen with no cursor on a
//! line of choices, or with fewer than two choices, shows none.

use super::input_box::is_rule;
use crate::contract::Dialog;

/// The glyphs a harness marks the choice its Enter would take with: Devin's is `❭`, the mark
/// of its input box too (T-1192).
const CURSORS: [char; 7] = ['❯', '›', '❭', '▶', '►', '→', '>'];

/// The characters of the frame a harness draws round a dialog, taken off both ends of a line.
const FRAME: [char; 14] = [
    '│', '┃', '║', '╭', '╮', '╰', '╯', '─', '━', '═', '┌', '┐', '└', '┘',
];

/// How many lines above its choices a dialog's words are looked for.
const WORDS_ABOVE: usize = 8;

/// How many lines of what a dialog says above its words are kept ([`Dialog::detail`]).
const DETAIL_LINES: usize = 10;

/// What a screen shows of a dialog: the dialog, and what the reader's caller judges it by.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Shown {
    pub(crate) dialog: Dialog,
    /// The lines below the last choice: where the harness's prompt may be drawn.
    pub(crate) below: Vec<String>,
}

/// A line without its frame and the spaces inside it, as wide as it was drawn: the frame's
/// characters at its start become spaces, so a choice keeps the column it was drawn at.
fn unframed(line: &str) -> String {
    let start = line
        .char_indices()
        .find(|(_, at)| !at.is_whitespace() && !FRAME.contains(at))
        .map_or(line.len(), |(at, _)| at);
    let (frame, rest) = line.split_at(start);
    let rest = rest.trim_end_matches(|at: char| at.is_whitespace() || FRAME.contains(&at));
    " ".repeat(frame.chars().count()) + rest
}

/// The column of a line's first character that is not a space, in characters.
fn indent(line: &str) -> Option<usize> {
    line.chars().position(|at| !at.is_whitespace())
}

/// A choice's words without the number a harness draws before it (`1.`, `2)`, or Devin's `1`
/// and a space): the number, where there is one.
fn numbered(words: &str) -> (Option<u32>, &str) {
    let digits = words.chars().take_while(char::is_ascii_digit).count();
    if (1..=2).contains(&digits) {
        let rest = &words[digits..];
        let after = rest
            .strip_prefix(". ")
            .or_else(|| rest.strip_prefix(") "))
            .or_else(|| {
                rest.strip_prefix(' ')
                    .filter(|after| after.starts_with(char::is_alphanumeric))
            });
        if let Some(after) = after {
            return (words[..digits].parse().ok(), after.trim());
        }
    }
    (None, words)
}

/// Whether `line`, as drawn, is a rule across the screen: a run of four or more of a rule's
/// characters (`─`, Claude Code's dashed `╌` between a call and a hook's words) and no words.
fn rule_across(line: &str) -> bool {
    const RULES: [char; 7] = ['─', '━', '═', '╌', '┄', '┈', '-'];
    !line.chars().any(char::is_alphanumeric)
        && line.chars().filter(|at| RULES.contains(at)).count() >= 4
}

/// Whether `line` is a dialog's hint of how it is answered, drawn under its choices: Enter, or
/// `↵`, to confirm ("Enter to confirm · Esc to cancel", Devin's "↑↓ select · ↵ confirm · esc
/// cancel", Codex's "Press enter to confirm or esc to go back"). A dialog that draws one waits
/// for an answer, whatever its words above ask.
fn hints_an_answer(line: &str) -> bool {
    let line = line.to_lowercase();
    line.contains("confirm")
        && (line.contains("enter") || line.contains('↵') || line.contains("esc"))
}

/// The line `line` as the harness's cursor on a choice: the column its words start at, and
/// the words. None where it is no such line: a cursor with nothing after it is an empty box.
fn cursor_on(line: &str) -> Option<(usize, String)> {
    let at = indent(line)?;
    let rest: String = line.chars().skip(at).collect();
    let first = rest.chars().next()?;
    if !CURSORS.contains(&first) {
        return None;
    }
    let after = &rest[first.len_utf8()..];
    let words = after.trim_start();
    // A cursor glued to its words is no cursor (`>=`, `->`), and a lone one is a box.
    if words.is_empty() || after.len() == words.len() {
        return None;
    }
    let column = at + 1 + (after.chars().count() - words.chars().count());
    words
        .chars()
        .next()
        .is_some_and(char::is_alphanumeric)
        .then(|| (column, words.trim_end().to_owned()))
}

/// The dialog the bottom of `screen` shows, where it shows one: its lowest line of choices
/// with the harness's cursor on one, and the words above it.
pub(crate) fn read(screen: &[String]) -> Option<Shown> {
    let lines: Vec<String> = screen.iter().map(|line| unframed(line)).collect();
    let (at, (column, chosen)) = lines
        .iter()
        .enumerate()
        .rev()
        .find_map(|(at, line)| cursor_on(line).map(|cursor| (at, cursor)))?;
    // The choices aligned with the chosen one, above it and below, up to a line that is
    // neither a choice nor one of a choice's own (a description drawn further in). Where the
    // chosen one is numbered, a numbered choice drawn left of it is one too, as Devin draws its
    // list under its `❭`; the count in order below holds them to the list.
    let numbers_its_choices = numbered(&chosen).0.is_some();
    let choice = |line: &str| -> Option<Option<String>> {
        let start = indent(line)?;
        let words: String = line.chars().skip(start).collect();
        match start.cmp(&column) {
            // A line drawn where the choices are that says no words (a footer's `⏵⏵`, a
            // frame) ends them.
            std::cmp::Ordering::Equal if words.starts_with(char::is_alphanumeric) => {
                Some(Some(words.trim_end().to_owned()))
            }
            std::cmp::Ordering::Greater => Some(None),
            std::cmp::Ordering::Less if numbers_its_choices && numbered(&words).0.is_some() => {
                Some(Some(words.trim_end().to_owned()))
            }
            _ => None,
        }
    };
    let mut first = at;
    let mut above = Vec::new();
    while first > 0 {
        match choice(&lines[first - 1]) {
            Some(words) => {
                above.extend(words);
                first -= 1;
            }
            None => break,
        }
    }
    above.reverse();
    // An input box is drawn under a rule across the screen, its mark on its first line (Claude
    // Code's `❯`, Devin's `❭`): what it holds, a line or more, is no choice (`input_box`).
    if first == at && first > 0 && is_rule(&screen[first - 1]) {
        return None;
    }
    let mut last = at;
    let mut below = Vec::new();
    while last + 1 < lines.len() {
        match choice(&lines[last + 1]) {
            Some(words) => {
                below.extend(words);
                last += 1;
            }
            None => break,
        }
    }
    let mut options: Vec<String> = above;
    options.push(chosen);
    options.extend(below);
    // Choices that a harness numbers are numbered from one, in their order.
    let numbers: Vec<Option<u32>> = options.iter().map(|words| numbered(words).0).collect();
    if numbers.iter().any(Option::is_some) {
        let counted = numbers
            .iter()
            .enumerate()
            .all(|(at, number)| *number == u32::try_from(at + 1).ok());
        if !counted {
            return None;
        }
    }
    let options: Vec<String> = options
        .iter()
        .map(|words| numbered(words).1.to_owned())
        .collect();
    if options.len() < 2 {
        return None;
    }
    // The words it opens with: the nearest line above the choices that asks, or else the
    // nearest that says anything.
    let near: Vec<usize> = (first.saturating_sub(WORDS_ABOVE)..first)
        .rev()
        .filter(|at| !lines[*at].trim().is_empty())
        .collect();
    let asked = near.iter().copied().find(|at| asks(&lines[*at]));
    let hinted = lines[last + 1..]
        .iter()
        .filter(|line| !line.trim().is_empty())
        .take(2)
        .any(|line| hints_an_answer(line));
    let (said_at, asks) = match asked {
        Some(at) => (at, true),
        None => (*near.first()?, hinted),
    };
    let question = lines[said_at].trim().to_owned();
    // What it says right above its words, the lines of one paragraph, back to a blank line, a
    // rule across it or [`DETAIL_LINES`] lines: a hook's own words of why it asks (T-1198), the
    // plugin an offer is of. A conversation's lines above a dialog change as it works, and
    // never count.
    let mut detail: Vec<String> = Vec::new();
    for at in (0..said_at).rev() {
        if rule_across(&screen[at]) || detail.len() == DETAIL_LINES {
            break;
        }
        let words = lines[at].trim();
        if words.is_empty() {
            // The blank lines between it and its words are passed over; one above them ends
            // its paragraph, and what the conversation drew above is none of it.
            if detail.is_empty() {
                continue;
            }
            break;
        }
        if words.chars().any(char::is_alphanumeric) {
            detail.push(words.to_owned());
        }
    }
    detail.reverse();
    Some(Shown {
        dialog: Dialog {
            question,
            options,
            asks,
            detail,
            prompt_below: false,
            closes_after_s: None,
        },
        below: screen[last + 1..].to_vec(),
    })
}

/// Whether `line` asks: it ends in a question mark, or in one and a word in brackets
/// ("How is Claude doing this session? (optional)").
fn asks(line: &str) -> bool {
    let line = line.trim_end();
    line.ends_with('?')
        || line
            .rfind("? (")
            .is_some_and(|at| line.ends_with(')') && line.len() - at < 24)
}

#[cfg(test)]
mod tests;
