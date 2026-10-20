//! The two patterns of Node's Claude record reader, and the two of a command
//! typed at Claude's prompt, each compiled once, the first time it is used, as
//! JavaScript reads it (`shared::pattern`).

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

/// What a command typed at Claude's prompt leaves as the text of the user's
/// turn: its name first (`<command-name>/compact</command-name>`), then its
/// message and its arguments.
pub(super) static COMMAND: LazyLock<Regex> =
    LazyLock::new(|| compile(r"^<command-name>/[^<]+</command-name>"));

/// What Claude Code writes, as a user's record, of what a command typed at its
/// prompt printed: `<local-command-stdout>`, or `<local-command-stderr>`.
pub(super) static OUTPUT: LazyLock<Regex> =
    LazyLock::new(|| compile(r"^<local-command-(?:stdout|stderr)>"));

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
        for command in [
            clear.to_owned(),
            clear.replace("/clear", "/compact"),
            "<command-name>/model</command-name><command-args>haiku</command-args>".to_owned(),
        ] {
            assert!(COMMAND.is_match(&command), "{command:?}");
        }
        for not in [
            "/compact",
            "<command-name>/</command-name>",
            "<command-name>compact</command-name>",
            " <command-name>/compact</command-name>",
            "<command-message>review</command-message><command-name>/review</command-name>",
        ] {
            assert!(!COMMAND.is_match(not), "{not:?}");
        }
        for output in [
            "<local-command-stdout></local-command-stdout>",
            "<local-command-stdout>Set model to Haiku</local-command-stdout>",
            "<local-command-stderr>Error</local-command-stderr>",
        ] {
            assert!(OUTPUT.is_match(output), "{output:?}");
        }
        for not in [
            "Compacted",
            " <local-command-stdout>x</local-command-stdout>",
            "<local-command-caveat>Caveat</local-command-caveat>",
            "<local-command-stdin>x</local-command-stdin>",
        ] {
            assert!(!OUTPUT.is_match(not), "{not:?}");
        }
    }
}
