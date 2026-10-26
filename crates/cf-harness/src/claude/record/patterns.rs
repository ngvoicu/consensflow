//! The pattern of Node's Claude record reader that is kept (its `/clear`'s is
//! one of the quiet commands' now), and those of a command typed at Claude's
//! prompt, each compiled once, the first time it is used, as JavaScript reads it
//! (`shared::pattern`).

use std::sync::LazyLock;

use regex::Regex;

use crate::shared::pattern::compile;

/// `/^<([A-Za-z][\w-]*)(?:\s[^>]*)?>/`: the opening tag of the envelope Claude
/// Code wraps a cross-session message in. Group 1 is its name.
pub(super) static ENVELOPE: LazyLock<Regex> =
    LazyLock::new(|| compile(r"^<([A-Za-z][A-Za-z0-9_-]*)(?:\s[^>]*)?>"));

/// What a command typed at Claude's prompt that runs no model leaves as the
/// text of the user's turn: its name first (`<command-name>/compact</command-name>`),
/// then its message and its arguments. A closed list, of the commands Claude
/// Code's own records show it does by itself, with no turn after what they
/// printed: `/compact`, `/model`, `/effort` (2.1.222 to 2.1.304), `/clear` and
/// `/reload-skills` (2.1.219). Another command may go on to ask the model what
/// it printed (2.1.303's `/plan <description>`, a skill, a command of the user's
/// own), and its turn ends as a prompt's does.
pub(super) static QUIET_COMMAND: LazyLock<Regex> = LazyLock::new(|| {
    compile(r"^<command-name>/(?:clear|compact|effort|model|reload-skills)</command-name>")
});

/// What Claude Code writes, as a user's record or as the system's
/// (`local_command`), of what a command typed at its prompt printed:
/// `<local-command-stdout>`, or `<local-command-stderr>`.
pub(super) static OUTPUT: LazyLock<Regex> =
    LazyLock::new(|| compile(r"^<local-command-(?:stdout|stderr)>"));

/// What Claude Code writes as the user's turn the moment `/compact` is typed,
/// before it compacts (2.1.303 to 2.1.310): the command as typed, with its
/// instructions where it was given some.
pub(super) static BARE_COMPACT: LazyLock<Regex> =
    LazyLock::new(|| compile(r"^/compact(?:\s[\s\S]*)?$"));

/// What Claude Code writes, as a meta record of the user's, before a command
/// typed at its prompt, and alone where a compaction was cancelled with Escape.
pub(super) static CAVEAT: LazyLock<Regex> = LazyLock::new(|| compile(r"^<local-command-caveat>"));

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
        for command in [
            clear.to_owned(),
            clear.replace("clear", "compact"),
            "<command-name>/model</command-name><command-args>haiku</command-args>".to_owned(),
            "<command-name>/effort</command-name>".to_owned(),
            "<command-name>/reload-skills</command-name>".to_owned(),
        ] {
            assert!(QUIET_COMMAND.is_match(&command), "{command:?}");
        }
        for not in [
            "/compact",
            "<command-name>/</command-name>",
            "<command-name>compact</command-name>",
            " <command-name>/compact</command-name>",
            "<command-name>/compacted</command-name>",
            "<command-name>/plan</command-name><command-args>a plan</command-args>",
            "<command-name>/review</command-name>",
            "<command-name>/my-skill</command-name>",
            "<command-message>review</command-message><command-name>/review</command-name>",
        ] {
            assert!(!QUIET_COMMAND.is_match(not), "{not:?}");
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
        for typed in ["/compact", "/compact keep the plan", "/compact\nand this"] {
            assert!(BARE_COMPACT.is_match(typed), "{typed:?}");
        }
        for not in [
            "/compacted",
            " /compact",
            "/compactness",
            "Please /compact",
            "<command-name>/compact</command-name>",
        ] {
            assert!(!BARE_COMPACT.is_match(not), "{not:?}");
        }
        assert!(CAVEAT.is_match("<local-command-caveat>Run by the user.</local-command-caveat>"));
        for not in [" <local-command-caveat>", "<local-command-stdout>x"] {
            assert!(!CAVEAT.is_match(not), "{not:?}");
        }
    }
}
