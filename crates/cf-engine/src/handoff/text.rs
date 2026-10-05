//! The small readings of text the handoff and the history pages share.

use std::fmt;

use cf_base::js;
use cf_base::text::{utf16_len, utf16_prefix};

/// A harness as the pages name it (`HARNESS_NAMES`): its own word when it has
/// no name of its own.
pub(super) fn name_of(harness: &str) -> &str {
    match harness {
        "claude-code" => "Claude Code",
        "codex" => "Codex",
        "opencode" => "OpenCode",
        "pi" => "Pi",
        "devin" => "Devin",
        other => other,
    }
}

/// No text on a page may read as a delivery's header.
pub(super) fn defuse(text: &str) -> String {
    text.replace("[ConsensFlow m-", "[earlier m-")
}

/// The first line of `text` that says something, cut to `max` UTF-16 code
/// units with an ellipsis when it is longer.
///
/// A cut inside a surrogate pair is written U+FFFD, where Node left half of
/// the pair: the ledger stores such a half as U+FFFD, and so does the JSON a
/// reader gets it in, so every reader of Node's text saw U+FFFD.
pub(super) fn first_line(text: &str, max: usize) -> String {
    let line = text
        .split('\n')
        .find(|part| !js::trim(part).is_empty())
        .unwrap_or("");
    if utf16_len(line) > max {
        format!("{}…", utf16_prefix(line, max - 1))
    } else {
        line.to_owned()
    }
}

/// Where a delivery's header starts in a window's record, and its id's digits
/// (`/\[ConsensFlow m-(\d+) /`, with `\d` the ASCII digits).
pub(super) struct Header<'a> {
    pub at: usize,
    pub id: &'a str,
}

/// The first delivery header in `text`.
pub(super) fn delivered(text: &str) -> Option<Header<'_>> {
    const START: &str = "[ConsensFlow m-";
    text.match_indices(START).find_map(|(at, _)| {
        let after = &text[at + START.len()..];
        let digits = after.bytes().take_while(u8::is_ascii_digit).count();
        (digits > 0 && after[digits..].starts_with(' ')).then(|| Header {
            at,
            id: &after[..digits],
        })
    })
}

/// A value as a template literal writes it: `null` for none.
pub(super) struct Shown<'a, T>(pub &'a Option<T>);

impl<T: fmt::Display> fmt::Display for Shown<'_, T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some(value) => fmt::Display::fmt(value, formatter),
            None => formatter.write_str("null"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_harness_is_named_or_is_its_own_word() {
        assert_eq!(name_of("claude-code"), "Claude Code");
        assert_eq!(name_of("opencode"), "OpenCode");
        assert_eq!(name_of("kimi"), "kimi");
    }

    #[test]
    fn the_first_line_is_the_first_that_says_something_and_is_cut_in_units() {
        assert_eq!(first_line("\n \t\nreal\nmore", 160), "real");
        assert_eq!(first_line("\u{FEFF}\u{A0}\nreal", 160), "real");
        // U+0085 is no white space to JavaScript's trim: it says something.
        assert_eq!(first_line("\u{85}\nreal", 160), "\u{85}");
        assert_eq!(first_line("", 160), "");
        assert_eq!(first_line("x".repeat(160).as_str(), 160), "x".repeat(160));
        assert_eq!(
            first_line("x".repeat(161).as_str(), 160),
            format!("{}…", "x".repeat(159))
        );
        // Two units each: the cut after 159 falls inside the 80th emoji.
        assert_eq!(
            first_line("🙂".repeat(81).as_str(), 160),
            format!("{}\u{FFFD}…", "🙂".repeat(79))
        );
    }

    #[test]
    fn a_header_is_the_marker_digits_and_a_space_the_first_one_counting() {
        let found = |text| delivered(text).map(|header| (header.at, header.id));
        assert_eq!(found("[ConsensFlow m-12 · note]"), Some((0, "12")));
        assert_eq!(found("typed[ConsensFlow m-7 x"), Some((5, "7")));
        assert_eq!(found("[ConsensFlow m-x [ConsensFlow m-3 "), Some((17, "3")));
        assert_eq!(found("[ConsensFlow m-12"), None);
        assert_eq!(found("[ConsensFlow m- 12 "), None);
        assert_eq!(found("[ConsensFlow m-12· note"), None);
        // An Arabic-Indic digit is no `\d` of JavaScript's.
        assert_eq!(found("[ConsensFlow m-\u{663} "), None);
        assert_eq!(found("[ConsensFlow m-007 "), Some((0, "007")));
    }

    #[test]
    fn a_missing_value_is_written_null() {
        assert_eq!(Shown(&Some(3)).to_string(), "3");
        assert_eq!(Shown(&Some("zeus")).to_string(), "zeus");
        assert_eq!(Shown::<i64>(&None).to_string(), "null");
    }
}
