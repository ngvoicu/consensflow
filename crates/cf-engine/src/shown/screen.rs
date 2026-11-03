//! Which rows of a window's last screen a note may quote, when some of them name a secret.
//!
//! **The rule is: when in doubt, the whole screen is withheld**, as the first version of the
//! note did for any screen with a row that names a secret. The per-row version that followed
//! (T-823) took out the row and its neighbours and quoted the rest, and the review of
//! `a26e0848e` put a password in the ledger three ways with it: a label with its value below
//! (`Password:` / `Enter the value below` / `red green blue`), a YAML block scalar with a
//! modifier (`>-`), and a value hard-wrapped over rows. A row's neighbours are where a value
//! *usually* is; the rest is anywhere. So a row that names a secret lets the rest of the screen
//! be quoted only where its whole extent is known:
//!
//! - **An assignment** on one row, `NAME=value`, `NAME: value`, `"name": "value"`, with the
//!   value one word, or one quoted string that closes on the row: the extent is the row. Its
//!   neighbours are taken out as well, and any row after it that is one word with no blank in it
//!   (a token wrapped by hand).
//! - **Prose** that names a secret and ends otherwise than at the name, a word that says more
//!   follows (`The password is`), or a mark that opens something (`No API key found for the selected model. Use /login …`, a test's name): the
//!   extent is the row and two rows each way.
//!
//! The whole screen is withheld for every other shape:
//! - a row that is only a label, or ends at the name or on a word that says more follows
//!   (`Password:`, `Enter password`, `Invalid API key`, `The password is`);
//! - a value that opens something on the row: a YAML block scalar with any modifier (`|`, `>`,
//!   `>-`, `|+`, `>2`), `{`, `[`, `(`, a `\`, a heredoc, a quote that does not close;
//! - an unquoted value of more than one word (`password: correct horse battery staple`), or a
//!   value the next row goes on with, indented deeper;
//! - a private key's block with a delimiter missing: a `BEGIN` with no `END` in view, an `END`
//!   with no `BEGIN`; and a run of rows that are base64 and nothing else (a block cut at both
//!   ends, with no delimiter in view).
//!
//! A private key's block whole, `BEGIN` to `END`, is taken out with the rows beside it.

use std::sync::LazyLock;

use regex::Regex;

use super::secret::names_a_secret;
use super::QUOTE_LINES;

/// A row that is an assignment: the name (group 1) and what follows `:` or `=` (group 2).
#[allow(clippy::expect_used)]
static ASSIGNMENT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)^[\s\-*]*(?:(?:export|set|setenv)\s+)?(?:\$env:)?["']?([A-Za-z_][A-Za-z0-9_.\-]*)["']?\s*[:=]\s*(.*)$"#,
    )
    .expect("a pattern of screen")
});

/// The words a sentence ends on when what it says goes on below it: `The password is`.
const DANGLING: [&str; 26] = [
    "is",
    "are",
    "was",
    "were",
    "be",
    "to",
    "for",
    "with",
    "as",
    "of",
    "set",
    "equals",
    "equal",
    "contains",
    "has",
    "have",
    "use",
    "using",
    "named",
    "called",
    "here",
    "below",
    "follows",
    "following",
    "this",
    "that",
];

/// Rows the neighbours of prose that names a secret reach.
const PROSE_REACH: usize = 2;

/// What a row that names a secret is, as far as its extent is known.
enum Extent {
    /// An assignment whose value is on the row.
    Assignment,
    /// Prose that is whole on its row.
    Prose,
    /// Anything the extent of is not known.
    Unknown,
}

/// The rows a quote of `tail` may show, and how many of the rows it would have shown were
/// left out: none where the screen is withheld whole.
pub(super) fn without_secrets(tail: &[String]) -> Option<(Vec<String>, usize)> {
    let mut out = vec![false; tail.len()];
    let in_block = key_blocks(tail, &mut out)?;
    if key_fragment(tail, &in_block) {
        return None;
    }
    for (at, row) in tail.iter().enumerate() {
        if in_block[at] || !names_a_secret(row) {
            continue;
        }
        let next_deeper = tail
            .get(at + 1)
            .is_some_and(|next| !next.trim().is_empty() && indent(next) > indent(row));
        match extent(row, next_deeper) {
            Extent::Unknown => return None,
            Extent::Prose => reach(&mut out, at, PROSE_REACH),
            Extent::Assignment => {
                reach(&mut out, at, 1);
                // A token wrapped by hand: the rows after it that are one word.
                let mut after = at + 2;
                while tail.get(after).is_some_and(|row| is_a_fragment(row)) {
                    out[after] = true;
                    after += 1;
                }
            }
        }
    }
    let shown: Vec<usize> = (0..tail.len())
        .filter(|&at| tail[at].chars().any(char::is_alphanumeric))
        .rev()
        .take(QUOTE_LINES)
        .collect();
    let left_out = shown.iter().filter(|&&at| out[at]).count();
    let mut kept: Vec<String> = shown
        .into_iter()
        .filter(|&at| !out[at])
        .map(|at| tail[at].clone())
        .collect();
    kept.reverse();
    Some((kept, left_out))
}

/// Takes out the row `at` and `by` rows each way.
fn reach(out: &mut [bool], at: usize, by: usize) {
    let from = at.saturating_sub(by);
    let to = (at + by).min(out.len() - 1);
    out[from..=to].fill(true);
}

/// The leading blanks of a row.
fn indent(row: &str) -> usize {
    row.len() - row.trim_start().len()
}

/// A row that is one word: a piece of a token wrapped by hand.
fn is_a_fragment(row: &str) -> bool {
    let word = row.trim();
    word.len() >= 4 && !word.chars().any(char::is_whitespace)
}

/// The private keys' blocks of the screen, `BEGIN` to `END`, marked in `out` with the row
/// beside each; and which rows are in one. None where a block's delimiter is missing.
fn key_blocks(tail: &[String], out: &mut [bool]) -> Option<Vec<bool>> {
    let mut in_block = vec![false; tail.len()];
    let mut open: Option<usize> = None;
    for (at, row) in tail.iter().enumerate() {
        let upper = row.to_ascii_uppercase();
        if !upper.contains("PRIVATE KEY") {
            continue;
        }
        if upper.contains("BEGIN") {
            if open.is_some() {
                return None;
            }
            open = Some(at);
        } else if upper.contains("-----END") || upper.contains(" END ") {
            let begin = open.take()?;
            in_block[begin..=at].fill(true);
            reach(out, begin, 1);
            reach(out, at, 1);
            out[begin..=at].fill(true);
        }
    }
    if open.is_some() {
        return None;
    }
    Some(in_block)
}

/// Whether two rows in a row, outside a block, are base64 and nothing else: the body of a key
/// whose delimiters are out of view.
fn key_fragment(tail: &[String], in_block: &[bool]) -> bool {
    let body = |at: usize| {
        let row = tail[at].trim();
        let mixed = row.chars().any(|c| c.is_ascii_uppercase())
            && row
                .chars()
                .any(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
        !in_block[at]
            && row.len() >= 20
            && mixed
            && row
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '=')
    };
    (1..tail.len()).any(|at| body(at - 1) && body(at))
}

/// How far the extent of the secret `row` names is known; `next_deeper` is whether the row
/// below is indented more.
fn extent(row: &str, next_deeper: bool) -> Extent {
    let trimmed = row.trim_end();
    if let Some(found) = ASSIGNMENT.captures(trimmed) {
        let name = found.get(1).map_or("", |name| name.as_str());
        if names_a_secret(name) {
            let value = found.get(2).map_or("", |value| value.as_str());
            return if closed_value(value) && !next_deeper {
                Extent::Assignment
            } else {
                Extent::Unknown
            };
        }
    }
    if opens_something(trimmed) || ends_at_the_name(trimmed) || trails_off(trimmed) {
        Extent::Unknown
    } else {
        Extent::Prose
    }
}

/// Whether `value` is wholly on its row: one quoted string that closes, or one word.
fn closed_value(value: &str) -> bool {
    let value = value.trim();
    let Some(first) = value.chars().next() else {
        return false;
    };
    if first == '"' || first == '\'' {
        return match closing_quote(value, first) {
            Some(end) => value[end + 1..]
                .chars()
                .all(|c| c.is_whitespace() || ",;)]}".contains(c)),
            None => false,
        };
    }
    if "|>&*!{[(<\\".contains(first) || value.ends_with('\\') {
        return false;
    }
    value
        .trim_end_matches([',', ';'])
        .split_whitespace()
        .count()
        == 1
}

/// Where the quote `value` opens with ends: the byte of the closing `quote`. A backslash
/// escapes in a double quote, a doubled quote in a single one (YAML's `'it''s'`).
fn closing_quote(value: &str, quote: char) -> Option<usize> {
    let mut chars = value.char_indices().skip(1).peekable();
    while let Some((at, c)) = chars.next() {
        if c == '\\' && quote == '"' {
            chars.next();
        } else if c == quote {
            if quote == '\'' && chars.peek().is_some_and(|&(_, next)| next == '\'') {
                chars.next();
            } else {
                return Some(at);
            }
        }
    }
    None
}

/// Whether the row opens something its end does not close: a mark that goes on, or a quote
/// that begins a word and does not end.
fn opens_something(row: &str) -> bool {
    let end = row.trim_end();
    if end.ends_with([':', '=', '(', '[', '{', '\\', '|', '>']) || end.ends_with("->") {
        return true;
    }
    let mut before = ' ';
    let chars: Vec<char> = end.chars().collect();
    for (at, &c) in chars.iter().enumerate() {
        if (c == '"' || c == '\'') && (before.is_whitespace() || "=:([{,".contains(before)) {
            let closes = chars[at + 1..].contains(&c);
            if !closes {
                return true;
            }
        }
        before = c;
    }
    false
}

/// Whether the row ends on a word that says more follows (`The password is`).
fn trails_off(row: &str) -> bool {
    let last = row
        .rsplit(char::is_whitespace)
        .next()
        .unwrap_or_default()
        .trim_matches(|c: char| !c.is_alphanumeric())
        .to_lowercase();
    DANGLING.contains(&last.as_str())
}

/// Whether the name of the secret is the last word of the row: a label, a prompt, an error
/// that says only what was wrong with it.
fn ends_at_the_name(row: &str) -> bool {
    let row = row.trim_end_matches(|c: char| !c.is_alphanumeric());
    let Some((before, _)) = row.rsplit_once(char::is_whitespace) else {
        return true;
    };
    !names_a_secret(before)
}

#[cfg(test)]
mod tests;
