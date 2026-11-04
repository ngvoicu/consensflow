//! Whether a window's last screen may hold a secret, and so is withheld whole.
//!
//! The rule is the base's: where any of the screen's rows names a secret, none of it is quoted,
//! and no guess is made of where a secret ends (the reviews T-1216, T-1222 and T-1224). What a
//! row can do to hide a secret from a test of the row alone is what this file looks at:
//!
//! - **A name or a token's prefix the window's wrap cut in two.** The rows are looked at as
//!   they were drawn, and joined with nothing between them: `{"api_` / `key": "red` / `green
//!   blue"}` is `api_key` at the join, and `sk-` / `abcd…` is a token's beginning. A token's
//!   prefix ([`begins_a_token`]) withholds the screen whatever follows it.
//! - **A frame.** A program that draws a box around its output puts `│`, `┃`, `|` and the box's
//!   edges at the ends of every row, and a row of a key between them is no row of base64. The
//!   frame and the blanks beside it are taken off before a row is looked at ([`undecorated`]).
//! - **A key with no word to say so.** What is left is a key (or a token, a certificate's body,
//!   a hash) cropped so that no delimiter and no name is in view: rows of base64 and nothing
//!   else, as narrow as the window was (`MIIEvQIBADANBgkq` / `hkiG9w0BAQEFAASC`), with a short
//!   ending (`YQ==`) or the padding alone (`=`). No masker holds those: each row is below any
//!   token's size. So the rows are looked at together, and the whole screen is withheld where
//!   they are a run of rows that are base64 characters and nothing else (letters, digits, `+`,
//!   `/`, no blank in them; `=` for padding):
//!   - a row of forty characters or more by itself, in any case (a hash, a token, a key's body);
//!   - a row of twelve characters or more that looks encoded by itself: a digit among letters,
//!     a `+`, a `/`, an `=`, or a capital after a small letter;
//!   - two rows in a row that both look encoded;
//!   - two rows in a row or more that are twelve characters or more each and forty together;
//!   - a row of one `=` or two, which is a key's last row whatever stands beside it.
//! - **A value in groups.** `YWJj-ZGVm-ZGhp-…`, `deadbeef cafebabe feedface …`: four groups or
//!   more of four characters or more of base64 or hex, each parted from the next by one blank,
//!   `-`, or any one character that is none of them, in a row or over rows ([`grouped`]). A run
//!   of groups of one length, or of groups that each look encoded or are hex, is no sentence: a
//!   sentence of four words does not have them all of one length, nor of letters and digits.
//!
//! A false alarm (a hash, a long identifier, a sentence of four-letter words) costs the quote of
//! one screen; a miss costs a key.

use super::mask::begins_a_token;
use super::secret::names_a_secret;

/// A row this long, of base64's characters alone, is a token or a key's body by itself.
const LONG: usize = 40;

/// A row this long that looks encoded is a key's body cropped by the window; rows this long,
/// two or more in a row, are a key's body together once they reach [`LONG`].
const BODY_ROW: usize = 12;

/// The groups of a value in groups: four characters or more each, four of them or more.
const GROUP: usize = 4;
const GROUPS: usize = 4;

/// What a program draws at the ends of a row: bars, in the light, heavy and double kinds, and
/// the ASCII one.
const BARS: &str = "│┃║|";

/// What a program draws at the corners and edges of a box, and as its rules.
const EDGES: &str = "┌┐└┘├┤┬┴┼╭╮╯╰╔╗╚╝╠╣╦╩╬─━═";

/// Why a screen is withheld.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Withheld {
    /// It names a secret, or begins a token of a known kind.
    Secret,
    /// It holds what may be a key, or a value in groups.
    Key,
}

/// Why the rows of `tail` may not be quoted, or none where they may.
pub(super) fn withheld(tail: &[String]) -> Option<Withheld> {
    // Every row the host sent is looked at, as drawn: a value that goes on over a row keeps no
    // name on the row it ends on.
    if tail.iter().any(|row| names_a_secret(row)) {
        return Some(Withheld::Secret);
    }
    let rows: Vec<&str> = tail
        .iter()
        .map(|row| undecorated(row))
        .filter(|row| row.chars().any(char::is_alphanumeric) || is_padding(row))
        .collect();
    // The rows again as one text: a name or a prefix the wrap broke is whole at the join. And
    // with a blank between them, where each row begins as it did: a token's prefix after a row
    // that ends in a letter, and a scheme and its credential (`Basic` / `YTpi`).
    let joined = rows.concat();
    let spaced = rows.join(" ");
    if names_a_secret(&joined) || begins_a_token(&joined) || begins_a_token(&spaced) {
        return Some(Withheld::Secret);
    }
    if holds_a_key(&rows) || grouped(&spaced) {
        return Some(Withheld::Key);
    }
    None
}

/// A row without the frame a program drew round it, and the blanks beside the frame.
fn undecorated(row: &str) -> &str {
    row.trim_matches(|c: char| c.is_whitespace() || BARS.contains(c) || EDGES.contains(c))
}

/// Whether rows (the screen's own, chrome left out) hold rows that may be a key's or a token's.
fn holds_a_key(rows: &[&str]) -> bool {
    // A row of one `=` or two is a key's last row, whatever stands beside it: the row before
    // it may be a body no shape of its own gives away (`abcdefghijklmnopqrstuvw`).
    if rows.iter().any(|row| is_padding(row)) {
        return true;
    }
    let mut run: Vec<&str> = Vec::new();
    for row in rows {
        if is_base64(row) {
            run.push(row);
        } else {
            if suspect(&run) {
                return true;
            }
            run.clear();
        }
    }
    suspect(&run)
}

/// Whether a run of rows of base64's characters (adjacent rows of the screen) may be a key.
fn suspect(run: &[&str]) -> bool {
    if run.iter().any(|row| row.len() >= LONG) {
        return true;
    }
    if run
        .iter()
        .any(|row| row.len() >= BODY_ROW && looks_encoded(row))
    {
        return true;
    }
    if run
        .windows(2)
        .any(|pair| pair.iter().all(|row| looks_encoded(row)))
    {
        return true;
    }
    run.len() >= 2
        && run.iter().all(|row| row.len() >= BODY_ROW)
        && run.iter().map(|row| row.len()).sum::<usize>() >= LONG
}

/// A row of the padding of base64 alone: one `=` or two. A longer row of them is a rule that
/// draws a line across the screen, which is chrome.
fn is_padding(row: &str) -> bool {
    row == "=" || row == "=="
}

/// A row of base64's characters and nothing else.
fn is_base64(row: &str) -> bool {
    !row.is_empty() && row.chars().all(is_base64_char)
}

fn is_base64_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '='
}

/// Whether a row of base64's characters says by them that it is not a word: it has a `+`, a
/// `/` or an `=`, a digit among letters, or a capital after a small letter.
fn looks_encoded(row: &str) -> bool {
    if row.contains(['+', '/', '=']) {
        return true;
    }
    let letters = row.chars().any(|c| c.is_ascii_alphabetic());
    if letters && row.chars().any(|c| c.is_ascii_digit()) {
        return true;
    }
    let chars: Vec<char> = row.chars().collect();
    chars
        .windows(2)
        .any(|pair| pair[0].is_ascii_lowercase() && pair[1].is_ascii_uppercase())
}

/// Whether `text` has a value in groups: a run of [`GROUPS`] groups or more of [`GROUP`]
/// characters or more of base64's, each parted from the next by exactly one character that is
/// none of them. A run counts where its groups are all of one length, or each is hex or looks
/// encoded (a digit among letters, a `+`, a capital after a small letter): the text of a
/// sentence has neither.
fn grouped(text: &str) -> bool {
    let mut run: Vec<&str> = Vec::new();
    for (token, gap) in tokens(text) {
        if token.len() < GROUP {
            run.clear();
            continue;
        }
        // A group goes on the run when one character parts it from the last; any other gap
        // (none at the start of the text, two blanks, a word between) begins a run again.
        if gap != 1 {
            run.clear();
        }
        run.push(token);
        if run.len() >= GROUPS {
            let last = &run[run.len() - GROUPS..];
            if last.iter().all(|group| group.len() == last[0].len())
                || last.iter().all(|group| is_grouped_value(group))
            {
                return true;
            }
        }
    }
    false
}

/// The tokens of `text`, the longest runs of base64's characters, each with how many characters
/// that are none of them lie before it (since the last token, or the start of the text).
fn tokens(text: &str) -> Vec<(&str, usize)> {
    let mut found = Vec::new();
    let mut open: Option<(usize, usize)> = None;
    let mut gap = 0;
    for (at, c) in text.char_indices() {
        if is_base64_char(c) {
            open.get_or_insert((at, gap));
            continue;
        }
        if let Some((from, before)) = open.take() {
            found.push((&text[from..at], before));
            gap = 0;
        }
        gap += 1;
    }
    if let Some((from, before)) = open {
        found.push((&text[from..], before));
    }
    found
}

/// Whether a group of a value in groups is hex, or looks encoded by a digit among letters, a
/// `+` or a capital after a small letter (not by a `/` or an `=`, which paths and `NAME=value`
/// words have).
fn is_grouped_value(group: &str) -> bool {
    if group.chars().all(|c| c.is_ascii_hexdigit()) {
        return true;
    }
    if group.contains('+') {
        return true;
    }
    let letters = group.chars().any(|c| c.is_ascii_alphabetic());
    if letters && group.chars().any(|c| c.is_ascii_digit()) {
        return true;
    }
    let chars: Vec<char> = group.chars().collect();
    chars
        .windows(2)
        .any(|pair| pair[0].is_ascii_lowercase() && pair[1].is_ascii_uppercase())
}

#[cfg(test)]
mod tests;
