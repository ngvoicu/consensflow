//! Text measured as JavaScript measures it, in UTF-16 code units: a length
//! or a cut the Node code made with `.length` or `.slice` is made here the
//! same way, so a port says what Node said.

use std::borrow::Cow;

/// The length of `text` in UTF-16 code units: JavaScript's `.length`.
pub fn utf16_len(text: &str) -> usize {
    text.chars().map(char::len_utf16).sum()
}

/// The first `units` UTF-16 code units of `text`: JavaScript's
/// `.slice(0, units)` as Node prints it, a character cut in half printed as
/// U+FFFD.
pub fn utf16_prefix(text: &str, units: usize) -> Cow<'_, str> {
    let mut taken = 0;
    for (at, character) in text.char_indices() {
        let width = character.len_utf16();
        if taken + width > units {
            return if taken < units {
                Cow::Owned(format!("{}\u{FFFD}", &text[..at]))
            } else {
                Cow::Borrowed(&text[..at])
            };
        }
        taken += width;
    }
    Cow::Borrowed(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measures_as_javascript_does_an_emoji_counting_two() {
        assert_eq!(utf16_len("abc"), 3);
        assert_eq!(utf16_len("ăîș"), 3);
        assert_eq!(utf16_len("a😀"), 3);
    }

    #[test]
    fn cuts_where_javascript_cuts_half_an_emoji_printed_as_a_replacement_character() {
        assert_eq!(utf16_prefix("abcdef", 3), "abc");
        assert_eq!(utf16_prefix("ab", 3), "ab");
        assert_eq!(utf16_prefix("ab😀c", 4), "ab😀");
        assert_eq!(utf16_prefix("ab😀c", 3), "ab\u{FFFD}");
        assert_eq!(utf16_prefix("😀", 0), "");
    }
}
