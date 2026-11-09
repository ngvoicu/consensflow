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
//! - **A URL's user and password the wrap cut in two.** `https://alder:copper` / `willow@host`:
//!   the masker takes them out of a row that has all of them and a row that has half of them is
//!   none, so a row end in or beside the user and password of a URL withholds the screen
//!   ([`splits_url_credentials`]); a URL whole on a row is masked where it stands. The user of a
//!   URL with no password counts (`https://TOKEN@host`) unless its scheme names an account
//!   (`ssh://git@host`).
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
//!     a `+`, an `=`, a capital after a small letter, a `/` in it (`abcd/efghijkl`: a body of
//!     small letters and one `/`, which the rest of a path the window wrapped is as well:
//!     0.2% of 14,912 windows, mostly `ps` lines), or one word of letters (`redgreenblue`, a
//!     value whose name scrolled out: 0.05%); not a number of digits alone;
//!   - two rows in a row that both look encoded (a `/` is not enough for the short ones:
//!     `src/lib` / `tests/it` are two paths);
//!   - two rows in a row or more that are twelve characters or more each (the last may be
//!     shorter) and forty together, or three rows or more of eight each (a narrower window);
//!   - a row of one `=` or two, which is a key's last row whatever stands beside it.
//! - **A value in groups.** `YWJj-ZGVm-ZGhp-…`, `deadbeef cafebabe feedface …`: four groups or
//!   more of four characters or more of base64 or hex, each parted from the next by one blank
//!   or one `-`, in a row or over rows ([`grouped`]). A run of groups of one length, or of groups
//!   that each look encoded or are hex, is no sentence: a sentence of four words seldom has them
//!   all of one length, nor of letters and digits. Groups parted by a tab or by blanks of any
//!   number count where each is hex or looks encoded. A word of its own parted by `_`, `:` or `.`
//!   counts on the same terms ([`parted_word`]): `abcd_efgh_ijkl_mnop`, an IPv6 address. (Any
//!   four groups in a row parted by any one character were tried over 5,610 windows of real
//!   output: 4.8% were withheld, most of them the snake_case names of `cargo test`; as a word of
//!   its own they cost none of the 1,517 windows of `cargo test` output measured, and 6 of
//!   14,912 windows of git, `ls`, `ps` and prose.)
//! - **Words and nothing else.** A passphrase typed bare has no name, no digit and no length of
//!   its own: four words of four letters or more, two in a row at least, with nothing else in the
//!   row (`correct horse battery staple`, `abcd efghi jklmno pqrstuv`) are withheld
//!   ([`bare_words`]): 0.05% of those windows.
//! - **Text in percent signs.** `https%3A%2F%2Falice%3Aswordfish%40example.test` has no `://` and
//!   no `@` for the masker, and names nothing to a test of its words. What it spells out is looked
//!   at as well ([`percent_decoded`]): a name, a token's beginning or a URL's user and password;
//!   and what that spells, where it holds escapes again (`api%255Fkey%253D…`), as far as three
//!   passes ([`percent_levels`]). A word with two escapes or more in it, one for a letter, a
//!   digit or `-._~`, or an escaped sign with two hex digits after it, is written so for no
//!   reason a URL has ([`escapes_plainly`]): `cobalt%2Dviolet%2Dindigo`.
//! - **The window's own session id.** A harness may print the conversation's id as it quits
//!   (`claude --resume <uuid>`), and a UUID is a value in groups. ConsensFlow has each window's
//!   session: that id, whole and in any case, is taken out of the text the rule of a value in
//!   groups looks at ([`without_own_session`]), and of no other: every name, prefix, URL and
//!   decoding check looks at the rows as drawn. Any other UUID withholds the screen.
//!
//! What this does **not** close, and says so: a value with its name scrolled out of the tail
//! that is a word of eleven letters or fewer, a word with digits and symbols under twelve, or one
//! word to a row; a passphrase with a word of three letters in it; one escape alone
//! (`a%2Db`); a UUID the wrap cut in two. Nothing in their shape tells them from the words and
//! the paths around them. What would: the pane host saying the program had echo off, or a tail
//! long enough for the label to be in it.
//!
//! A false alarm (a hash, a long identifier, a sentence of four-letter words) costs the quote of
//! one screen; a miss costs a key.

use std::sync::LazyLock;

use regex::Regex;

use super::mask::{begins_a_token, has_url_credentials, url_credential_spans};
use super::secret::names_a_secret;

/// A row this long, of base64's characters alone, is a token or a key's body by itself.
const LONG: usize = 40;

/// A row this long that looks encoded is a key's body cropped by the window; rows this long,
/// two or more in a row, are a key's body together once they reach [`LONG`]; and rows
/// [`NARROW_ROW`] long, three or more in a row, where the window was narrower.
const BODY_ROW: usize = 12;
const NARROW_ROW: usize = 8;

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

/// Why the rows of `tail` may not be quoted, or none where they may. `own_session` is the
/// conversation the window ran in, as ConsensFlow has it: a harness may print it when it quits
/// (`claude --resume <id>`), and that id, a UUID, is no secret ([`without_own_session`]).
pub(super) fn withheld(tail: &[String], own_session: Option<&str>) -> Option<Withheld> {
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
    if names_a_secret(&joined)
        || begins_a_token(&joined)
        || begins_a_token(&spaced)
        || splits_url_credentials(&rows, &joined)
    {
        return Some(Withheld::Secret);
    }
    // Text in percent signs says nothing to the tests above: what it spells is looked at too,
    // and what that spells where it holds escapes again (`%252D` is `%2D` is `-`), as far as
    // [`PERCENT_LEVELS`].
    for text in [&joined, &spaced] {
        for spelled in percent_levels(text) {
            if names_a_secret(&spelled) || begins_a_token(&spelled) || has_url_credentials(&spelled)
            {
                return Some(Withheld::Secret);
            }
        }
    }
    // The window's own session id is exempt from the rule of a value in groups and from no other:
    // every check above and below looks at the rows as they were drawn (the joint review T-1322,
    // blocker 1: with the id taken out of the rows first, `https://<id>:swo` / `rdfish@vault`
    // was no URL in either row).
    if holds_a_key(&rows)
        || grouped(&without_own_session(&spaced, own_session))
        || bare_words(&rows)
        || escapes_plainly(&joined)
        || escapes_plainly(&spaced)
    {
        return Some(Withheld::Key);
    }
    None
}

/// `text` with the window's own session id taken out, where it is whole in it and the session is
/// a UUID (Claude Code prints `claude --resume <uuid>` when it quits): the id is
/// ConsensFlow's own record and no secret, and the rule of a value in groups would take it for
/// one. Only the id as a word of its own, in any case, and only where ConsensFlow knows it for
/// this window's: any other UUID on the screen is still a value in groups. **Only the text of
/// the rule of a value in groups is rewritten with it**: no name, prefix, URL or decoding check
/// is given a text with the id gone.
fn without_own_session(row: &str, own_session: Option<&str>) -> String {
    let Some(id) = own_session.filter(|id| is_uuid(id)) else {
        return row.to_owned();
    };
    let (lower_row, lower_id) = (row.to_ascii_lowercase(), id.to_ascii_lowercase());
    let word_edge =
        |c: Option<char>| c.is_none_or(|c| !(c.is_ascii_alphanumeric() || "-_".contains(c)));
    let mut kept = String::with_capacity(row.len());
    let mut at = 0;
    while let Some(found) = lower_row[at..].find(&lower_id) {
        let (start, end) = (at + found, at + found + lower_id.len());
        if word_edge(row[..start].chars().next_back()) && word_edge(row[end..].chars().next()) {
            kept.push_str(&row[at..start]);
            kept.push(' ');
        } else {
            kept.push_str(&row[at..end]);
        }
        at = end;
    }
    kept.push_str(&row[at..]);
    kept
}

/// Whether `text` is a UUID: 8-4-4-4-12 hex digits.
fn is_uuid(text: &str) -> bool {
    let parts: Vec<&str> = text.split('-').collect();
    parts.len() == 5
        && parts
            .iter()
            .zip([8, 4, 4, 4, 12])
            .all(|(part, len)| part.len() == len && part.chars().all(|c| c.is_ascii_hexdigit()))
}

/// Whether the end of a row falls in a URL's user and password, or at either side of them
/// (`https://alder:copper` / `willow@host`): the masker takes the user and password of a URL out
/// of a row that has all of them, and neither row of this has, so nothing is left to take. A URL
/// whole on a row is no concern of this: it is masked where it stands.
fn splits_url_credentials(rows: &[&str], joined: &str) -> bool {
    let mut ends = Vec::with_capacity(rows.len());
    let mut at = 0;
    for row in rows {
        at += row.len();
        ends.push(at);
    }
    url_credential_spans(joined)
        .into_iter()
        .any(|(from, to)| ends.iter().any(|end| (from..=to).contains(end)))
}

/// `text` with its `%XX` escapes spelled out, or none where it has none.
fn percent_decoded(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut spelled: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut found = false;
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'%' {
            if let Some(byte) = bytes.get(at + 1..at + 3).and_then(hex_byte) {
                spelled.push(byte);
                found = true;
                at += 3;
                continue;
            }
        }
        spelled.push(bytes[at]);
        at += 1;
    }
    found.then(|| String::from_utf8_lossy(&spelled).into_owned())
}

/// How many times escapes are spelled out again: `%252D` is `%2D` after one pass and `-` after
/// two. Text that needs more is not an ordinary URL.
const PERCENT_LEVELS: usize = 3;

/// `text` with its escapes spelled out, and what that spells with its escapes spelled out, as
/// far as [`PERCENT_LEVELS`]: none where it has no escape.
fn percent_levels(text: &str) -> Vec<String> {
    let mut levels: Vec<String> = Vec::new();
    while levels.len() < PERCENT_LEVELS {
        match percent_decoded(levels.last().map_or(text, String::as_str)) {
            Some(spelled) => levels.push(spelled),
            None => break,
        }
    }
    levels
}

/// Whether a word of `text` is written in percent signs for no reason a URL has: two escapes
/// or more in it, one of them for a letter, a digit or `-`, `.`, `_`, `~` (the characters a URL
/// never needs to escape: `cobalt%2Dviolet%2Dindigo`), or an escaped sign with a pair of hex
/// digits after it (`%252D`). `%20`, `%2F`, `%3A` and `caf%C3%A9` are what a URL has. One escape
/// alone is a printf's (`%4d` is `M`).
fn escapes_plainly(text: &str) -> bool {
    text.split_whitespace().any(|word| {
        let bytes = word.as_bytes();
        let escapes: Vec<u8> = (0..bytes.len().saturating_sub(2))
            .filter(|&at| bytes[at] == b'%')
            .filter_map(|at| hex_byte(&bytes[at + 1..at + 3]))
            .collect();
        let nested = word.contains("%25")
            && bytes
                .windows(5)
                .any(|five| &five[..3] == b"%25" && hex_byte(&five[3..5]).is_some());
        nested
            || (escapes.len() >= 2
                && escapes
                    .iter()
                    .any(|byte| byte.is_ascii_alphanumeric() || b"-._~".contains(byte)))
    })
}

/// A credential's word, the delimiter that gives it a value, and the value's first character:
/// `key: abc`, `"token" = "a b"`, `API_KEY=…`, `Authorization: …`. A name that has the word
/// anywhere in it counts (`ssh_key`, `x-api-key`), and so does one that has it as a piece of
/// another word (`monkey: 1`): a false alarm costs a block, a miss a credential.
#[allow(clippy::expect_used)]
static NAMED_VALUE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)(?:key|token|secret|passw(?:or)?d|authorization|bearer)s?["']?\s*[:=]\s*["']?[^\s"']"#,
    )
    .expect("a pattern")
});

/// Whether `lines`, the rows of one screen, have a credential's name with a value after it, **whatever the value is**: `key: abc` is a key as much as
/// `key: abcdefghijkl` is, and a value too short for the masker to take for one is not a safe one
/// (the joint review T-1322, blockers 2 and 3). The lines are looked at as they stand, joined by
/// lines, joined with nothing and joined with a blank, so that a name the wrap cut in two is
/// whole at the join, and as what they spell where they hold escapes.
pub(super) fn names_a_value(lines: &[&str]) -> bool {
    [lines.join("\n"), lines.concat(), lines.join(" ")]
        .iter()
        .any(|text| {
            std::iter::once(text.clone())
                .chain(percent_levels(text))
                .any(|text| NAMED_VALUE.is_match(&text))
        })
}

/// The byte two hex digits spell.
fn hex_byte(pair: &[u8]) -> Option<u8> {
    let high = char::from(*pair.first()?).to_digit(16)?;
    let low = char::from(*pair.get(1)?).to_digit(16)?;
    u8::try_from(high * 16 + low).ok()
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
    if run.iter().any(|row| is_a_body(row)) {
        return true;
    }
    if run
        .windows(2)
        .any(|pair| pair.iter().all(|row| looks_encoded(row)))
    {
        return true;
    }
    // The rows but the last are as long as the window was wide; the last is what is left.
    let Some((_, body)) = run.split_last() else {
        return false;
    };
    let total: usize = run.iter().map(|row| row.len()).sum();
    total >= LONG
        && ((run.len() >= 2 && body.iter().all(|row| row.len() >= BODY_ROW))
            || (run.len() >= 3 && body.iter().all(|row| row.len() >= NARROW_ROW)))
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

/// Whether a row of base64's characters, [`BODY_ROW`] long or more, may be a key's body cropped
/// by the window: it looks encoded, or it has a `/` in it (a body of small letters and one
/// `/`, `abcd/efghijkl`, which a path the window wrapped is as well), or it is one word of
/// letters (`redgreenblue`, a value whose name scrolled out). A row of digits alone is a number.
fn is_a_body(row: &str) -> bool {
    row.len() >= BODY_ROW
        && (looks_encoded(row) || row.contains('/') || row.chars().all(|c| c.is_ascii_alphabetic()))
}

/// Whether a row of base64's characters says by them that it is not a word: it has a `+` or an
/// `=`, a digit among letters, or a capital after a small letter. Not a `/`: two rows that
/// have one each are two paths the window wrapped (`src/lib`, `tests/it`); [`is_a_body`] takes it
/// from twelve characters.
fn looks_encoded(row: &str) -> bool {
    if row.contains(['+', '=']) {
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

/// How a group of a value in groups is parted from the one before it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Gap {
    /// By one blank or one `-`.
    One,
    /// By blanks that are not one blank: a tab, or two blanks or more (a column).
    Blanks,
}

/// Whether `text` has a value in groups: a run of [`GROUPS`] groups or more of [`GROUP`]
/// characters or more of base64's, each parted from the next by one blank or one `-`, in a row or
/// over rows; or by blanks of any number (a tab, two blanks) where each group is hex or looks
/// encoded. A run counts where its groups are all of one length and parted by one character
/// each, or where each group is hex or looks encoded (a digit among letters, a `+`, a capital
/// after a small letter): the text of a sentence has neither. A word of its own parted by `_`,
/// `:` or `.` is a value in groups too ([`parted_word`]).
fn grouped(text: &str) -> bool {
    let mut run: Vec<(&str, Option<Gap>)> = Vec::new();
    for (token, parted_by) in tokens(text) {
        if token.len() < GROUP {
            run.clear();
            continue;
        }
        // A group goes on the run when one blank or `-`, or blanks, part it from the last; any
        // other gap (none at the start of the text, a word between, `_`, `, `) begins a run.
        if parted_by.is_none() {
            run.clear();
        }
        run.push((token, parted_by));
        if run.len() >= GROUPS {
            let last = &run[run.len() - GROUPS..];
            let one_each = last[1..].iter().all(|(_, gap)| *gap == Some(Gap::One));
            if last.iter().all(|(group, _)| is_grouped_value(group))
                || (one_each && last.iter().all(|(group, _)| group.len() == last[0].0.len()))
            {
                return true;
            }
        }
    }
    text.split_whitespace().any(parted_word)
}

/// Whether `word` (a run without a blank, the quotes, brackets, commas and points that stand
/// round it taken off) is a value in groups parted by `_`, `:`, `.` or `-` and nothing else:
/// [`GROUPS`] parts or more of [`GROUP`] characters or more of base64's, all of one length or
/// each hex or looking encoded (`abcd_efgh_ijkl_mnop`, `2001:0db8:85a3:0000:…`). The word is
/// the whole of the value: four short words in the middle of a longer name (`a_host_shut_down_with_
/// windows`, the names `cargo test` prints, 5.5% of the windows of its output when any four in a
/// row counted) are no run.
fn parted_word(word: &str) -> bool {
    let word = word.trim_matches(|c: char| !(is_base64_char(c) || "_:.-".contains(c)));
    let parts: Vec<&str> = word.split(['_', ':', '.', '-']).collect();
    parts.len() >= GROUPS
        && parts
            .iter()
            .all(|part| part.len() >= GROUP && part.chars().all(is_base64_char))
        && (parts.iter().all(|part| part.len() == parts[0].len())
            || parts.iter().all(|part| is_grouped_value(part)))
}

/// The tokens of `text`, the longest runs of base64's characters, each with how it is parted from
/// the token before ([`Gap`]; none at the start of the text, nor where something else parts them).
fn tokens(text: &str) -> Vec<(&str, Option<Gap>)> {
    let mut found = Vec::new();
    let mut open: Option<(usize, Option<Gap>)> = None;
    // The gap since the last token: how many characters, the last one, whether all are blank.
    let (mut count, mut last, mut blank) = (0usize, ' ', true);
    for (at, c) in text.char_indices() {
        if is_base64_char(c) {
            open.get_or_insert((
                at,
                match (count, last) {
                    (1, ' ' | '-') => Some(Gap::One),
                    (count, _) if count >= 1 && blank => Some(Gap::Blanks),
                    _ => None,
                },
            ));
            continue;
        }
        if let Some((from, parted_by)) = open.take() {
            found.push((&text[from..at], parted_by));
            (count, blank) = (0, true);
        }
        count += 1;
        last = c;
        blank &= c.is_whitespace();
    }
    if let Some((from, parted_by)) = open {
        found.push((&text[from..], parted_by));
    }
    found
}

/// Whether rows are words and nothing else: two words or more to a row, each of [`GROUP`]
/// letters or more and of letters alone, and [`GROUPS`] words in all over the rows in a row
/// (`correct horse` / `battery staple`): the value in groups of the rule above, whose groups
/// are of any length. A passphrase typed bare has no name, no digit and no length of its own to
/// tell it by; a row of words that has a point, a comma or a word of three letters in it is a
/// sentence (`and`, `the`, `for` are what the glue of one is made of), and the screens of
/// ordinary output have a row of long words and nothing else seldom (0.05% of 14,912 windows
/// of git, cargo, `ls` and prose; with words of three letters it was 0.5%).
fn bare_words(rows: &[&str]) -> bool {
    let mut words = 0;
    for row in rows {
        let row_words: Vec<&str> = row.split_whitespace().collect();
        if row_words.len() >= 2
            && row_words
                .iter()
                .all(|word| word.chars().count() >= GROUP && word.chars().all(char::is_alphabetic))
        {
            words += row_words.len();
            if words >= GROUPS {
                return true;
            }
        } else {
            words = 0;
        }
    }
    false
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
