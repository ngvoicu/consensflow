//! Text measured as JavaScript measures it, in UTF-16 code units: a length
//! or a cut the Node code made with `.length` or `.slice` is made here the
//! same way, so a port says what Node said. And text as a window takes it
//! ([`window_text`]), and as Windows' console carries it ([`console_text`]).

mod console;

use std::borrow::Cow;

pub use console::console_text;

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

/// Text as a window can take it, for a first message and every later one
/// (`windowText`, `src/adapters/shared.js`). The pane host refuses a paste
/// with a control character other than tab and newline, and a harness's own
/// API refuses some too, so a CR before a newline goes, as the host would
/// drop it, and every other control character is shown: as its Unicode
/// picture (ESC as \u{241B}, a lone CR as \u{240D}, DEL as \u{2421}), or as
/// U+FFFD for the C1 ones, which have none.
///
/// Node also dropped half a surrogate pair, which the dispatcher's cut can
/// leave; a Rust text holds none, and a cut here writes U+FFFD
/// ([`utf16_prefix`]).
pub fn window_text(text: &str) -> String {
    text.replace("\r\n", "\n")
        .chars()
        .map(|character| match character {
            '\t' | '\n' => character,
            '\u{0}'..='\u{1F}' => {
                char::from_u32(0x2400 + u32::from(character)).unwrap_or(character)
            }
            '\u{7F}' => '\u{2421}',
            '\u{80}'..='\u{9F}' => '\u{FFFD}',
            other => other,
        })
        .collect()
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
    fn a_window_takes_tab_and_newline_and_is_shown_every_other_control_character() {
        assert_eq!(window_text("a\r\nb\tc"), "a\nb\tc");
        assert_eq!(window_text("a\rb"), "a\u{240D}b");
        assert_eq!(window_text("a\r\r\nb"), "a\u{240D}\nb");
        assert_eq!(window_text("\u{1B}[0m\u{0}"), "\u{241B}[0m\u{2400}");
        assert_eq!(
            window_text("\u{7F}\u{85}\u{9F}\u{A0}"),
            "\u{2421}\u{FFFD}\u{FFFD}\u{A0}"
        );
        assert_eq!(window_text("caf\u{E9} \u{1F600}"), "caf\u{E9} \u{1F600}");
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
