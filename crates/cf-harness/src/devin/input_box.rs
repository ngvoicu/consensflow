//! Devin's input box, as its screen shows it (3000.11.92, `cargo xtask live
//! escape-unsent`): a rule across the screen that says the permission mode in it,
//! the box's lines, the first after its `❭`, and a rule again. An empty box shows a
//! hint after its `❭`, the same words always, drawn faint; it keeps words typed at
//! one Escape and at two, and clears them at Ctrl+U. Its pickers draw their filter
//! between rules with no `❭`, and the command that opened one stays above the box.

use crate::shared::input_box::{boxed, InputBox};

/// The hint an empty box shows.
const HINT: &str = "Ask Devin to build features, fix bugs, or work on your code";

/// What Devin's input box holds, where `screen` shows one: the last box drawn.
pub(crate) fn read(screen: &[String]) -> Option<InputBox> {
    let (typed, more) = boxed(screen, '❭')?;
    Some(if !more && (typed.is_empty() || typed == HINT) {
        InputBox::Empty
    } else {
        InputBox::Holds
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &str) -> Vec<String> {
        text.lines().map(str::to_owned).collect()
    }

    const RULE: &str = "────────────────────────────────────────";

    fn boxed(inside: &str) -> Vec<String> {
        lines(&format!(
            " Pro · 100% remaining\n\n{RULE} (bypass permissions on) ─\n{inside}\n{RULE}\nSWE-2 Medium      See usage and cost: /session-stats"
        ))
    }

    #[test]
    fn a_box_that_shows_its_hint_or_nothing_is_empty() {
        assert_eq!(read(&boxed(&format!("❭ {HINT}"))), Some(InputBox::Empty));
        assert_eq!(read(&boxed("❭ ")), Some(InputBox::Empty));
    }

    #[test]
    fn words_in_the_box_are_held_the_hint_among_them_or_not() {
        assert_eq!(read(&boxed("❭ probe words")), Some(InputBox::Holds));
        assert_eq!(
            read(&boxed(&format!("❭ {HINT} please"))),
            Some(InputBox::Holds)
        );
        assert_eq!(read(&boxed("❭ first\n  second")), Some(InputBox::Holds));
    }

    #[test]
    fn the_box_under_a_command_sent_is_read_and_a_picker_is_not() {
        // The picker `/model` opens, its filter typed, and the box after Escape.
        let picker = lines(&format!(
            "❭ /model\n\n{RULE}\n/ zqv\n{RULE}\nNo matches\n↑↓ select · ↵ confirm · esc cancel"
        ));
        assert_eq!(read(&picker), None);
        let after = lines(&format!(
            "❭ /model\n\n{RULE} (bypass permissions on) ─\n❭ {HINT}\n{RULE}\nSWE-2 Medium"
        ));
        assert_eq!(read(&after), Some(InputBox::Empty));
    }
}
