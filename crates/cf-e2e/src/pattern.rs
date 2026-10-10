//! Regular expressions the support writes in its own source: a pattern is
//! built once, on its first use, and a pattern that is not well formed is a
//! mistake in the source and nothing a case could meet or mend.

use std::sync::OnceLock;

use regex::Regex;

/// The expression `pattern`, built the first time `cell` is asked for it. The
/// pattern is this crate's own, held well formed by the tests that use it.
#[allow(clippy::expect_used)]
pub fn once(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("a pattern written in this crate's own source"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pattern_is_built_once_and_the_same_one_is_given_again() {
        static CELL: OnceLock<Regex> = OnceLock::new();
        let first = once(&CELL, r"^a+$");
        assert!(first.is_match("aaa"));
        // The second pattern is never built: the cell holds the first.
        let second = once(&CELL, r"(unclosed");
        assert!(std::ptr::eq(first, second));
    }
}
