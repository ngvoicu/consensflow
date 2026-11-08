//! The reader of a dialog off a window's screen, **as it was at `5b7acff7f`** (3.0.7-alpha.2's
//! release head), frozen: a copy, never edited, and never used by the product (this module is
//! `testing`, which no build ships).
//!
//! It is here for one reason: the release bar of T-1243. A fix of the engine's reading of a harness's
//! own prompt as a dialog (T-1240) must not make any screen read as a dialog that the release head
//! read as none, on any harness; [`super::corpus`] runs a corpus of screens through this reader and
//! through each harness's window as it is now, and a screen the two read differently is a case the
//! corpus names, or a failure. The release head's `Window::dialog` is this reader and the window's
//! `shows_prompt` over what is below the choices ([`base_window_dialog`]).
//!
//! The only change from `crates/cf-harness/src/shared/dialog.rs` at that commit: `is_rule` is the
//! copy of `shared/input_box.rs`'s that is below, the items are `pub`, and the module's own tests
//! are not here.

use crate::contract::Dialog;

/// `shared/input_box.rs`'s `is_rule` at `5b7acff7f`: a rule drawn across the screen.
fn is_rule(line: &str) -> bool {
    let line = line.trim();
    line.starts_with("────") && line.ends_with('─')
}

/// The glyphs a harness marks the choice its Enter would take with: Devin's is `❭`, the mark
/// of its input box too (T-1192).
const CURSORS: [char; 7] = ['❯', '›', '❭', '▶', '►', '→', '>'];

/// The characters of the frame a harness draws round a dialog, taken off both ends of a line.
const FRAME: [char; 14] = [
    '│', '┃', '║', '╭', '╮', '╰', '╯', '─', '━', '═', '┌', '┐', '└', '┘',
];

/// How many lines above its choices a dialog's words are looked for.
const WORDS_ABOVE: usize = 8;

/// How many lines of what a dialog says above its words are kept ([`Dialog::detail`]).
const DETAIL_LINES: usize = 10;

/// What a screen shows of a dialog: the dialog, and what the reader's caller judges it by.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shown {
    pub dialog: Dialog,
    /// The lines below the last choice: where the harness's prompt may be drawn.
    pub below: Vec<String>,
}

/// A line without its frame and the spaces inside it, as wide as it was drawn: the frame's
/// characters at its start become spaces, so a choice keeps the column it was drawn at.
fn unframed(line: &str) -> String {
    let start = line
        .char_indices()
        .find(|(_, at)| !at.is_whitespace() && !FRAME.contains(at))
        .map_or(line.len(), |(at, _)| at);
    let (frame, rest) = line.split_at(start);
    let rest = rest.trim_end_matches(|at: char| at.is_whitespace() || FRAME.contains(&at));
    " ".repeat(frame.chars().count()) + rest
}

/// The column of a line's first character that is not a space, in characters.
fn indent(line: &str) -> Option<usize> {
    line.chars().position(|at| !at.is_whitespace())
}

/// A choice's words without the number a harness draws before it (`1.`, `2)`, or Devin's `1`
/// and a space): the number, where there is one.
fn numbered(words: &str) -> (Option<u32>, &str) {
    let digits = words.chars().take_while(char::is_ascii_digit).count();
    if (1..=2).contains(&digits) {
        let rest = &words[digits..];
        let after = rest
            .strip_prefix(". ")
            .or_else(|| rest.strip_prefix(") "))
            .or_else(|| {
                rest.strip_prefix(' ')
                    .filter(|after| after.starts_with(char::is_alphanumeric))
            });
        if let Some(after) = after {
            return (words[..digits].parse().ok(), after.trim());
        }
    }
    (None, words)
}

/// Whether `line`, as drawn, is a rule across the screen: a run of four or more of a rule's
/// characters (`─`, Claude Code's dashed `╌` between a call and a hook's words) and no words.
fn rule_across(line: &str) -> bool {
    const RULES: [char; 7] = ['─', '━', '═', '╌', '┄', '┈', '-'];
    !line.chars().any(char::is_alphanumeric)
        && line.chars().filter(|at| RULES.contains(at)).count() >= 4
}

/// Whether `line` is a dialog's hint of how it is answered, drawn under its choices: Enter, or
/// `↵`, to confirm ("Enter to confirm · Esc to cancel", Devin's "↑↓ select · ↵ confirm · esc
/// cancel", Codex's "Press enter to confirm or esc to go back"). A dialog that draws one waits
/// for an answer, whatever its words above ask.
fn hints_an_answer(line: &str) -> bool {
    let line = line.to_lowercase();
    line.contains("confirm")
        && (line.contains("enter") || line.contains('↵') || line.contains("esc"))
}

/// The line `line` as the harness's cursor on a choice: the column its words start at, and
/// the words. None where it is no such line: a cursor with nothing after it is an empty box.
fn cursor_on(line: &str) -> Option<(usize, String)> {
    let at = indent(line)?;
    let rest: String = line.chars().skip(at).collect();
    let first = rest.chars().next()?;
    if !CURSORS.contains(&first) {
        return None;
    }
    let after = &rest[first.len_utf8()..];
    let words = after.trim_start();
    // A cursor glued to its words is no cursor (`>=`, `->`), and a lone one is a box.
    if words.is_empty() || after.len() == words.len() {
        return None;
    }
    let column = at + 1 + (after.chars().count() - words.chars().count());
    words
        .chars()
        .next()
        .is_some_and(char::is_alphanumeric)
        .then(|| (column, words.trim_end().to_owned()))
}

/// The dialog the bottom of `screen` shows, where it shows one: its lowest line of choices
/// with the harness's cursor on one, and the words above it.
pub fn read(screen: &[String]) -> Option<Shown> {
    let lines: Vec<String> = screen.iter().map(|line| unframed(line)).collect();
    let (at, (column, chosen)) = lines
        .iter()
        .enumerate()
        .rev()
        .find_map(|(at, line)| cursor_on(line).map(|cursor| (at, cursor)))?;
    // The choices aligned with the chosen one, above it and below, up to a line that is
    // neither a choice nor one of a choice's own (a description drawn further in). Where the
    // chosen one is numbered, a numbered choice drawn left of it is one too, as Devin draws its
    // list under its `❭`; the count in order below holds them to the list.
    let numbers_its_choices = numbered(&chosen).0.is_some();
    let choice = |line: &str| -> Option<Option<String>> {
        let start = indent(line)?;
        let words: String = line.chars().skip(start).collect();
        match start.cmp(&column) {
            // A line drawn where the choices are that says no words (a footer's `⏵⏵`, a
            // frame) ends them.
            std::cmp::Ordering::Equal if words.starts_with(char::is_alphanumeric) => {
                Some(Some(words.trim_end().to_owned()))
            }
            std::cmp::Ordering::Greater => Some(None),
            std::cmp::Ordering::Less if numbers_its_choices && numbered(&words).0.is_some() => {
                Some(Some(words.trim_end().to_owned()))
            }
            _ => None,
        }
    };
    let mut first = at;
    let mut above = Vec::new();
    while first > 0 {
        match choice(&lines[first - 1]) {
            Some(words) => {
                above.extend(words);
                first -= 1;
            }
            None => break,
        }
    }
    above.reverse();
    // An input box is drawn under a rule across the screen, its mark on its first line (Claude
    // Code's `❯`, Devin's `❭`): what it holds, a line or more, is no choice (`input_box`).
    if first == at && first > 0 && is_rule(&screen[first - 1]) {
        return None;
    }
    let mut last = at;
    let mut below = Vec::new();
    while last + 1 < lines.len() {
        match choice(&lines[last + 1]) {
            Some(words) => {
                below.extend(words);
                last += 1;
            }
            None => break,
        }
    }
    let mut options: Vec<String> = above;
    options.push(chosen);
    options.extend(below);
    // Choices that a harness numbers are numbered from one, in their order.
    let numbers: Vec<Option<u32>> = options.iter().map(|words| numbered(words).0).collect();
    if numbers.iter().any(Option::is_some) {
        let counted = numbers
            .iter()
            .enumerate()
            .all(|(at, number)| *number == u32::try_from(at + 1).ok());
        if !counted {
            return None;
        }
    }
    let options: Vec<String> = options
        .iter()
        .map(|words| numbered(words).1.to_owned())
        .collect();
    if options.len() < 2 {
        return None;
    }
    // The words it opens with: the nearest line above the choices that asks, or else the
    // nearest that says anything.
    let near: Vec<usize> = (first.saturating_sub(WORDS_ABOVE)..first)
        .rev()
        .filter(|at| !lines[*at].trim().is_empty())
        .collect();
    let asked = near.iter().copied().find(|at| asks(&lines[*at]));
    let hinted = lines[last + 1..]
        .iter()
        .filter(|line| !line.trim().is_empty())
        .take(2)
        .any(|line| hints_an_answer(line));
    let (said_at, asks) = match asked {
        Some(at) => (at, true),
        None => (*near.first()?, hinted),
    };
    let question = lines[said_at].trim().to_owned();
    // What it says right above its words, the lines of one paragraph, back to a blank line, a
    // rule across it or [`DETAIL_LINES`] lines: a hook's own words of why it asks (T-1198), the
    // plugin an offer is of. A conversation's lines above a dialog change as it works, and
    // never count.
    let mut detail: Vec<String> = Vec::new();
    for at in (0..said_at).rev() {
        if rule_across(&screen[at]) || detail.len() == DETAIL_LINES {
            break;
        }
        let words = lines[at].trim();
        if words.is_empty() {
            // The blank lines between it and its words are passed over; one above them ends
            // its paragraph, and what the conversation drew above is none of it.
            if detail.is_empty() {
                continue;
            }
            break;
        }
        if words.chars().any(char::is_alphanumeric) {
            detail.push(words.to_owned());
        }
    }
    detail.reverse();
    Some(Shown {
        dialog: Dialog {
            question,
            options,
            asks,
            detail,
            prompt_below: false,
            closes_after_s: None,
        },
        below: screen[last + 1..].to_vec(),
    })
}

/// Whether `line` asks: it ends in a question mark, or in one and a word in brackets
/// ("How is Claude doing this session? (optional)").
fn asks(line: &str) -> bool {
    let line = line.trim_end();
    line.ends_with('?')
        || line
            .rfind("? (")
            .is_some_and(|at| line.ends_with(')') && line.len() - at < 24)
}

// ─── The release head's windows ────────────────────────────────────────────────────────────────

/// The offers of Claude Code's that close by themselves, and after how long: Claude's window at
/// `5b7acff7f` (`claude/adapter.rs`, `OFFERS_CLOSING` and `OFFER_CLOSES_AFTER_S`) put the second in
/// `Dialog::closes_after_s` of a dialog whose question is one of the first.
const OFFERS_CLOSING: [&str; 2] = [
    "Would you like to install this LSP plugin?",
    "Would you like to install it?",
];
const OFFER_CLOSES_AFTER_S: u32 = 30;

/// What the window of `harness` read of `screen` at `5b7acff7f`: `Window::dialog` as it was then, whole.
/// The release head's reader above, the prompt below its choices as the window's `shows_prompt` says it,
/// and for Claude Code's window the seconds its offers close after (the default `Window::dialog` is the
/// first two; Claude's override adds the last).
///
/// **What is borrowed from the code as it is now**: the window's `shows_prompt`, which is
/// `claude::trust::shows_prompt` for Claude Code, `muse::screen::shows_prompt` for Muse Code and the
/// default (false) for Codex, Devin and Pi. **Why that is safe**: this branch changes none of the files that
/// hold them, and a test below pins that: the files are byte for byte what `5b7acff7f` has (a hash of each),
/// and the adapters of Codex, Devin and Pi define no `shows_prompt`. A change to one of those files fails
/// the test, which says to freeze a copy of the old code here before the new code is judged by it.
pub fn base_window_dialog(
    harness: cf_proto::agents::Harness,
    window: &dyn crate::contract::Window,
    screen: &[String],
) -> Option<Dialog> {
    read(screen).map(|shown| {
        let closes_after_s = (harness == cf_proto::agents::Harness::Claude
            && OFFERS_CLOSING.contains(&shown.dialog.question.as_str()))
        .then_some(OFFER_CLOSES_AFTER_S);
        Dialog {
            prompt_below: window.shows_prompt(&shown.below),
            closes_after_s,
            ..shown.dialog
        }
    })
}

#[cfg(test)]
mod tests {
    /// FNV-1a, 64 bits, of `text` with its carriage returns taken out.
    fn fnv(text: &str) -> u64 {
        text.bytes()
            .filter(|byte| *byte != b'\r')
            .fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
                (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
            })
    }

    /// What T-1300 added to `claude/adapter.rs`: the Computer Use permission prompt counts as a
    /// dialog that asks, its decline is on the list of safe declines, and the two declines the
    /// release head had carry the two flags `Decline` gained. None of it is read by the code the
    /// corpus borrows; each is taken out again, where it was added, before the file is hashed.
    const T1300_ASKS: &str =
        "            asks: shown.dialog.asks || super::computer_use_prompt::is_prompt(&shown.dialog),\n";
    const T1300_DECLINE: &str = "        if let Some(decline) = super::computer_use_prompt::decline(dialog) {\n            return Some(decline);\n        }\n";
    const T1300_FLAGS_HOOK: &str = "        listed: true,\n        members_only: false,\n";
    const T1300_FLAGS_OFFER: &str = "            listed: true,\n            members_only: false,\n";

    /// What T-1277 added to `claude/adapter.rs`, and no code the corpus borrows reads: Claude
    /// Code's `dialog_identity` (a `Window` method of its own, which the release head's window did
    /// not have) and the `pub(super)` of `LSP_OFFER` that its module reads. They are taken out
    /// again before the file is hashed, so the file is the release head's, byte for byte,
    /// otherwise: any other change to it fails the test, `shows_prompt` among them. The method is
    /// taken out only where it was added, right above the doc comment of `safe_decline`: a method
    /// that changed, or moved elsewhere in the file, is left in and fails the hash (T-1282).
    fn without_what_t1277_added(file: &str, text: &str) -> String {
        const IDENTITY: &str = "    /// What tells one of Claude Code's dialogs from another read of its screen: the time in its\n    /// own warning line blanked, and its offer to install an LSP plugin from its own title down\n    /// (`dialog_identity`, T-1277). Never a time in a command, a hook's reason or a question.\n    fn dialog_identity(&self, dialog: &Dialog) -> Dialog {\n        super::dialog_identity::identity(dialog)\n    }\n\n";
        const NEXT: &str = "    /// Claude Code's list of safe declines: its offer to install an LSP plugin, answered \"No, not\n";
        if file == "claude/adapter.rs" {
            text.replace(&format!("{IDENTITY}{NEXT}"), NEXT)
                .replace("pub(super) const LSP_OFFER", "const LSP_OFFER")
                .replace(T1300_ASKS, "")
                .replace(T1300_DECLINE, "")
                .replace(T1300_FLAGS_HOOK, "")
                .replace(T1300_FLAGS_OFFER, "")
        } else {
            text.to_owned()
        }
    }

    /// The code [`super::base_window_dialog`] borrows is the release head's.
    #[test]
    fn the_prompt_readers_the_release_heads_windows_are_borrowed_for_are_the_release_heads() {
        let pinned: [(&str, &str, u64); 5] = [
            (
                "claude/adapter.rs",
                include_str!("../claude/adapter.rs"),
                0x60cfc152347ba0cb,
            ),
            (
                "claude/trust.rs",
                include_str!("../claude/trust.rs"),
                0xffd85053667df919,
            ),
            (
                "shared/trust.rs",
                include_str!("../shared/trust.rs"),
                0xfdb8b9364f366d29,
            ),
            (
                "muse/window.rs",
                include_str!("../muse/window.rs"),
                0x7b1ce00732909c9b,
            ),
            (
                "muse/screen.rs",
                include_str!("../muse/screen.rs"),
                0xf5eb2cab0d90713b,
            ),
        ];
        for (file, text, hash) in pinned {
            assert_eq!(
                fnv(&without_what_t1277_added(file, text)),
                hash,
                "{file} is not what 5b7acff7f has: the corpus borrows its prompt reader as the release head's; \
                 freeze a copy of the old code in testing/base_dialog.rs before judging the new code by it"
            );
        }
        for (file, text) in [
            ("codex/adapter.rs", include_str!("../codex/adapter.rs")),
            ("devin/adapter.rs", include_str!("../devin/adapter.rs")),
            ("pi/adapter.rs", include_str!("../pi/adapter.rs")),
        ] {
            assert!(
                !text.contains("fn shows_prompt"),
                "{file} defines a shows_prompt: the release head's window had the default (false)"
            );
        }
        assert!(
            include_str!("../contract.rs").contains(
                "fn shows_prompt(&self, _screen: &[String]) -> bool {\n        false\n    }"
            ),
            "the default shows_prompt is not false"
        );
    }
}
