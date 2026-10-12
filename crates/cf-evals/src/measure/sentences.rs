//! What a chief asked the owner, counted from the words it ended its turns with:
//! the sentences that end in a question mark, and not a question mark in code,
//! in a link or in the marks around a word. The owner is asked in the chief's
//! terminal and nowhere else, so these texts are all there is to count; they stay
//! in the report for a person to check.

use std::sync::OnceLock;

use cf_e2e::pattern;
use regex::Regex;

use super::metrics::OwnerQuestions;
use crate::js;

/// A fenced block of code, from its opening fence to its closing one: an opening
/// fence with no closing one is no block.
fn fenced() -> &'static Regex {
    static FENCED: OnceLock<Regex> = OnceLock::new();
    pattern::once(&FENCED, r"(?s)```.*?```")
}

/// Code in a line.
fn inline() -> &'static Regex {
    static INLINE: OnceLock<Regex> = OnceLock::new();
    pattern::once(&INLINE, r"`[^`\n]*`")
}

/// A link: a scheme, `://` and what follows to the next white space, as
/// JavaScript's `\b[a-z][a-z0-9+.-]*:\/\/\S+` reads one (its word boundary and
/// its white space are ASCII's and its own).
fn link() -> &'static Regex {
    static LINK: OnceLock<Regex> = OnceLock::new();
    pattern::once(
        &LINK,
        r"(?-u:\b)[A-Za-z][A-Za-z0-9+.\-]*://[^\t\n\x0B\x0C\r \u{A0}\u{1680}\u{2000}-\u{200A}\u{2028}\u{2029}\u{202F}\u{205F}\u{3000}\u{FEFF}]+",
    )
}

/// What a sentence may be followed by without that being where it ends: a quote,
/// a bracket.
const CLOSING: [char; 6] = ['"', '\'', '”', '»', ')', ']'];

/// The question sentences in a text put to the owner: a sentence that ends in a
/// question mark, not one in fenced or inline code or in a link. A rhetorical
/// question counts too; the report keeps the texts for a person to check.
/// Emphasis marks around a question (`**Which one?**`) are not its end.
pub fn question_sentences(text: &str) -> Vec<String> {
    let prose = fenced().replace_all(text, " ");
    let prose = inline().replace_all(&prose, " ");
    let prose = link().replace_all(&prose, " ");
    let prose: String = prose
        .chars()
        .filter(|at| !matches!(at, '*' | '_'))
        .collect();
    prose
        .split('\n')
        .flat_map(sentences)
        .map(|sentence| js::trim(sentence).trim_end_matches(CLOSING).to_owned())
        .filter(|sentence| sentence.ends_with('?'))
        .collect()
}

/// The pieces of `line` between the white space that follows a full stop, an
/// exclamation mark or a question mark: JavaScript's `split(/(?<=[.!?])\s+/)`,
/// which the `regex` crate cannot write.
fn sentences(line: &str) -> Vec<&str> {
    let mut pieces = Vec::new();
    let mut start = 0;
    let mut before: Option<char> = None;
    let mut spaces: Option<usize> = None;
    for (at, found) in line.char_indices() {
        match (js::is_space(found), spaces) {
            // The first white space after a mark that ends a sentence begins what splits.
            (true, None) if matches!(before, Some('.' | '!' | '?')) => {
                spaces = Some(at);
            }
            (false, Some(from)) => {
                pieces.push(&line[start..from]);
                start = at;
                spaces = None;
            }
            _ => {}
        }
        // White space that did not split ends no run: what is before the next
        // character is white space, which is none of the marks.
        before = Some(found);
    }
    // A run of white space that reaches the end of the line is a separator too: the
    // piece before it is whole, and the empty one after it is kept, as `split` has it.
    match spaces {
        Some(from) => {
            pieces.push(&line[start..from]);
            pieces.push("");
        }
        None => pieces.push(&line[start..]),
    }
    pieces
}

/// How many question sentences a text has.
pub fn count_questions(text: &str) -> i64 {
    i64::try_from(question_sentences(text).len()).unwrap_or(i64::MAX)
}

/// What a chief asked the owner, all of it in its terminal: the question
/// sentences of each message that ended a turn, and the pickers (its harness's
/// own question dialog) the owner answered there. The texts stay for a person
/// to check.
pub fn owner_questions(turn_ends: &[String], pickers: i64) -> OwnerQuestions {
    let asking: Vec<&String> = turn_ends
        .iter()
        .filter(|text| count_questions(text) > 0)
        .collect();
    OwnerQuestions {
        questions: asking.iter().map(|text| count_questions(text)).sum(),
        turns_asking: i64::try_from(asking.len()).unwrap_or(i64::MAX),
        pickers,
        texts: asking.into_iter().cloned().collect(),
    }
}

#[cfg(test)]
mod tests;
