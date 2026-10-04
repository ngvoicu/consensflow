//! The two patterns of `hosts/lib/completion/claude-code.js`, each compiled
//! once, the first time it is used, as JavaScript reads it
//! (`shared::pattern`).

use std::sync::LazyLock;

use regex::Regex;

use crate::shared::pattern::compile;

/// `/^<([A-Za-z][\w-]*)(?:\s[^>]*)?>/`: the opening tag of the envelope Claude
/// Code wraps a cross-session message in. Group 1 is its name.
pub(super) static ENVELOPE: LazyLock<Regex> =
    LazyLock::new(|| compile(r"^<([A-Za-z][A-Za-z0-9_-]*)(?:\s[^>]*)?>"));

/// `/^<command-name>\/clear<\/command-name>\s*<command-message>clear<\/command-message>\s*<command-args><\/command-args>$/`:
/// what a `/clear` leaves as the text of the user's turn.
pub(super) static CLEAR: LazyLock<Regex> = LazyLock::new(|| {
    compile(
        r"^<command-name>/clear</command-name>\s*<command-message>clear</command-message>\s*<command-args></command-args>$",
    )
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_pattern_builds_and_tells_a_match_from_what_is_not_one() {
        let name = |text: &str| ENVELOPE.captures(text).map(|found| found[1].to_owned());
        assert_eq!(name("<tag>x").as_deref(), Some("tag"));
        assert_eq!(name("<a-b_1 k=\"v\">x").as_deref(), Some("a-b_1"));
        assert_eq!(name("<tag\n k=\"v\n\">x").as_deref(), Some("tag"));
        for not in ["<1tag>", "<tag/>", " <tag>", "<tag k=\"v\"", "tag>"] {
            assert!(ENVELOPE.captures(not).is_none(), "{not:?}");
        }
        let clear = "<command-name>/clear</command-name>\n            <command-message>clear</command-message>\n            <command-args></command-args>";
        assert!(CLEAR.is_match(clear));
        assert!(CLEAR.is_match(&clear.replace('\n', "\u{FEFF}").replace(' ', "")));
        for not in [
            format!("{clear}\n"),
            format!("x{clear}"),
            clear.replace("/clear", "/compact"),
        ] {
            assert!(!CLEAR.is_match(&not), "{not:?}");
        }
    }
}
