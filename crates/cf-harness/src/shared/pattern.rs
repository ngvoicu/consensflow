//! JavaScript's regular expressions, as JavaScript reads one without the `u`
//! flag, in the `regex` crate, which reads Unicode by default. A pattern of
//! the Node code is written as it was, but for four things:
//! - a digit is `[0-9]`, where `\d` is any Unicode digit;
//! - a word character is `[A-Za-z0-9_]` and a letter `[a-zA-Z]`; a word
//!   matched in any case is `(?i-u:…)`, since `/i` folds ASCII alone, where
//!   Unicode's folding adds `ſ` and the Kelvin sign;
//! - a word boundary is `(?-u:\b)`, between ASCII word characters alone;
//! - `\s` is JavaScript's own set, which [`compile`] writes in: it holds
//!   U+FEFF, and not U+0085.

use regex::Regex;

/// What JavaScript's `\s` is, as the items of a class: the white space and
/// the line terminators of ECMAScript.
const SPACE: &str = r"\t\n\x0B\x0C\r \x{A0}\x{1680}\x{2000}-\x{200A}\x{2028}\x{2029}\x{202F}\x{205F}\x{3000}\x{FEFF}";

/// A pattern of the Node code, each `\s` of it (in a class too) the class
/// JavaScript's is. No pattern holds an escaped backslash before an `s`.
// The patterns are constants of the code that calls this, and a test there
// builds each: a mistake in one fails that test, and no input can reach it.
#[allow(clippy::expect_used)]
pub(crate) fn compile(pattern: &str) -> Regex {
    Regex::new(&pattern.replace(r"\s", &format!("[{SPACE}]"))).expect("a pattern of the Node code")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn white_space_is_javascripts_in_a_class_and_out_of_one() {
        let space = compile(r"^\s$");
        for character in ['\t', '\u{A0}', '\u{2028}', '\u{FEFF}', '\u{3000}'] {
            assert!(space.is_match(&character.to_string()), "{character:?}");
        }
        // Unicode's white space, which JavaScript's `\s` is not.
        assert!(!space.is_match("\u{85}"));
        let not_space = compile(r"^[^)\s]+$");
        assert!(not_space.is_match("a\u{85}b"));
        assert!(!not_space.is_match("a\u{FEFF}b") && !not_space.is_match("a)b"));
    }
}
