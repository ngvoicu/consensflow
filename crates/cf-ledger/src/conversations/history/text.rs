//! The text a read of the history handles: a value as text, a stretch of it by
//! characters, and what a search compares and finds in it.

use rusqlite::types::ValueRef;

use crate::text::text_of;

/// A text value as text, U+FFFD for what is no UTF-8 in it; none for null.
pub(super) fn text_or_none(value: ValueRef<'_>) -> Option<String> {
    match value {
        ValueRef::Text(bytes) => Some(text_of(bytes)),
        _ => None,
    }
}

/// `count` characters of `text` from character `from` (fewer at its end).
pub(super) fn chars(text: &str, from: usize, count: usize) -> &str {
    let mut at = text.char_indices().map(|(at, _)| at).skip(from);
    let start = at.next().unwrap_or(text.len());
    let end = at.nth(count.saturating_sub(1)).unwrap_or(text.len());
    &text[start..end.max(start)]
}

/// The last `count` characters of `text` (all of it, where it is shorter).
pub(super) fn last_chars(text: &str, count: usize) -> &str {
    if count == 0 {
        return "";
    }
    text.char_indices()
        .rev()
        .nth(count - 1)
        .map_or(text, |(at, _)| &text[at..])
}

/// `text` in lower case, a character at a time, by Unicode's own mapping: what
/// a search compares, so that "É" holds "é". It is no case folding: "ß" does
/// not hold "ss", a final "ς" is not "σ", and "İ" is "i̇".
pub(super) fn folded(text: &str) -> String {
    text.chars().flat_map(char::to_lowercase).collect()
}

/// Where `text` first holds `needle` (folded), in characters of `text`.
pub(super) fn first_match(text: &str, needle: &str) -> Option<usize> {
    // Each folded character's place in the text it came from.
    let mut lowered = String::with_capacity(text.len());
    let mut came_from = Vec::with_capacity(text.len());
    for (place, character) in text.chars().enumerate() {
        for lower in character.to_lowercase() {
            for _ in 0..lower.len_utf8() {
                came_from.push(place);
            }
            lowered.push(lower);
        }
    }
    lowered.find(needle).map(|at| came_from[at])
}
