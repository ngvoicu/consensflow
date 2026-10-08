//! The text a Devin message is compared by: its wire streams a file link as
//! `[name](file:///path)`, a quoted range as `[name:1-3](file:///path)`, and
//! its store keeps `<ref_file file="/path" />` and `<ref_snippet file="/path"
//! lines="1-3" />` (Devin 3000.11, 2026-09-26 and 10-03), so each is read as
//! its path; on Windows the wire's `file:///C:/Users/…` and the store's
//! `C:\Users\…` are one path. Nothing else is loosened, since the comparison is
//! what tells a final message from a half one.

use std::sync::LazyLock;

use regex::{Captures, Regex};

use crate::shared::pattern::compile;

/// A tag the store keeps for a file: `/<ref_\w+\s+file="([^"]*)"[^>]*\/>/g`.
static TAG: LazyLock<Regex> =
    LazyLock::new(|| compile(r#"<ref_[A-Za-z0-9_]+\s+file="([^"]*)"[^>]*/>"#));

/// A link the wire streams to a file: `/\[[^\]]*\]\(file:\/\/([^)\s]*)\)/g`.
static LINK: LazyLock<Regex> = LazyLock::new(|| compile(r"\[[^\]]*\]\(file://([^)\s]*)\)"));

/// The characters a URI reserves, whose escapes `decodeURI` keeps as written.
const RESERVED: &[u8] = b";/?:@&=+$,#";

/// `text` with each file tag and file link read as its path.
pub(super) fn comparable(text: &str) -> String {
    let tagged = TAG.replace_all(text, |found: &Captures<'_>| path(group(found)));
    LINK.replace_all(&tagged, |found: &Captures<'_>| path(group(found)))
        .into_owned()
}

/// The path a tag or a link names: the pattern's group, which always takes part.
fn group<'a>(found: &Captures<'a>) -> &'a str {
    found.get(1).map_or("", |group| group.as_str())
}

/// A file's path as the comparison reads it: its escapes decoded where they
/// are UTF-8's, its backslashes slashes, and a drive's path without the
/// slash a link puts before it.
fn path(value: &str) -> String {
    let decoded = decode_uri(value).unwrap_or_else(|| value.to_owned());
    let slashed = decoded.replace('\\', "/");
    // `/^\/[A-Za-z]:\//`.
    match slashed.as_bytes() {
        [b'/', drive, b':', b'/', ..] if drive.is_ascii_alphabetic() => slashed[1..].to_owned(),
        _ => slashed,
    }
}

/// `decodeURI(value)`: each escape decoded, but those of a character a URI
/// reserves, which stay as written; none where JavaScript throws a
/// `URIError`, on an escape that is no escape, or bytes that are no UTF-8.
fn decode_uri(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut decoded = String::with_capacity(value.len());
    // Where the text not copied yet starts: after the last escape.
    let mut plain = 0;
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] != b'%' {
            at += 1;
            continue;
        }
        decoded.push_str(&value[plain..at]);
        let first = escaped(bytes, at)?;
        let length = match first {
            0x00..=0x7F => 1,
            0xC0..=0xDF => 2,
            0xE0..=0xEF => 3,
            0xF0..=0xF7 => 4,
            _ => return None,
        };
        if length == 1 {
            if RESERVED.contains(&first) {
                decoded.push_str(&value[at..at + 3]);
            } else {
                decoded.push(char::from(first));
            }
        } else {
            let mut sequence = [first, 0, 0, 0];
            for (following, byte) in sequence.iter_mut().enumerate().take(length).skip(1) {
                *byte = escaped(bytes, at + 3 * following).filter(|byte| byte & 0xC0 == 0x80)?;
            }
            decoded.push_str(std::str::from_utf8(&sequence[..length]).ok()?);
        }
        at += 3 * length;
        plain = at;
    }
    decoded.push_str(&value[plain..]);
    Some(decoded)
}

/// The byte the escape `%XY` at `at` stands for: none when there is no such
/// escape there.
fn escaped(bytes: &[u8], at: usize) -> Option<u8> {
    let Some(&[b'%', high, low]) = bytes.get(at..at + 3) else {
        return None;
    };
    let digit = |byte: u8| char::from(byte).to_digit(16);
    u8::try_from(digit(high)? * 16 + digit(low)?).ok()
}

#[cfg(test)]
mod tests;
