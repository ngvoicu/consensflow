//! Claude Code's question about a folder it was never told to trust, as its window
//! shows it before anything else (2.1.305, `cargo xtask live folder-trust`): the
//! folder, a few lines that ask whether it is a project the person trusts, and two
//! choices, "No, exit" and "Yes, I trust this folder", the `❯` on the first. Enter
//! on "No, exit" ends the window; Down moves to the yes, which Enter confirms, and
//! Claude Code writes the trust down for the root of the folder's git repository
//! and goes on to its prompt, and to the first message it was opened with. Before
//! 2.1.287 the yes came first, numbered: the choice is read by its words. Its prompt,
//! once it is past the question, is told by the line under its input box.

use crate::shared::trust::{chosen, shows, DOWN, ENTER};

/// What Claude's prompt shows as ConsensFlow opens it, in full-permission mode.
const PROMPT: &str = "bypass permissions on";

/// The choice that trusts the folder, and the one that ends the window.
const YES: &str = "Yes, I trust this folder";
const NO: &str = "No, exit";

/// The key toward a yes where `screen` shows the question: Enter once the `❯` is
/// on the yes, Down while it is on the no. The question is its two choices with the
/// `❯` on one of them: a screen that only names them (a message that quotes the
/// question) asks nothing, and one drawn halfway is read again at the next look.
pub(crate) fn toward_trust(screen: &[String]) -> Option<&'static [u8]> {
    if !(shows(screen, YES) && shows(screen, NO)) {
        return None;
    }
    match chosen(screen, '❯') {
        Some(choice) if choice.ends_with(YES) => Some(ENTER),
        Some(choice) if choice.ends_with(NO) => Some(DOWN),
        _ => None,
    }
}

/// Whether `screen` is Claude's prompt, as ConsensFlow opens its windows: the
/// line under its input box that says it is in full-permission mode ("⏵⏵ bypass
/// permissions on", 2.1.305), and no question.
pub(crate) fn shows_prompt(screen: &[String]) -> bool {
    shows(screen, PROMPT) && toward_trust(screen).is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &str) -> Vec<String> {
        text.lines().map(str::to_owned).collect()
    }

    #[test]
    fn the_question_opens_on_no_and_is_answered_with_down_then_enter() {
        // As Claude Code 2.1.305 draws it, its folder named above.
        let asked = lines(
            " Accessing workspace:\n /work/project\n Quick safety check: Is this a project you created or one you trust?\n Security guide\n ❯ No, exit\n   Yes, I trust this folder\n Enter to confirm · Esc to cancel",
        );
        assert_eq!(toward_trust(&asked), Some(DOWN));
        let on_yes = lines(
            " Quick safety check: Is this a project you created or one you trust?\n   No, exit\n ❯ Yes, I trust this folder\n Enter to confirm · Esc to cancel",
        );
        assert_eq!(toward_trust(&on_yes), Some(ENTER));
        // Before 2.1.287 the yes came first, numbered.
        let numbered = lines("❯ 1. Yes, I trust this folder\n  2. No, exit");
        assert_eq!(toward_trust(&numbered), Some(ENTER));
    }

    #[test]
    fn its_prompt_is_the_line_under_its_input_box_in_full_permission_mode() {
        // As Claude Code 2.1.305 draws it.
        let prompt = lines(" ❯ Try \"how do I log an error?\"\n   fresh Sonnet   0 tokens\n   ⏵⏵ bypass permissions on (shift+tab to cycle) · ← for agents");
        assert!(shows_prompt(&prompt));
        let asked = lines(" Quick safety check\n ❯ No, exit\n   Yes, I trust this folder");
        assert!(!shows_prompt(&asked));
        assert!(!shows_prompt(&lines(" Accessing workspace:\n /work")));
    }

    #[test]
    fn a_message_that_quotes_the_question_asks_nothing() {
        // A conversation about this very dialog, as a resumed window shows it.
        let quoted = lines(" ⏺ Claude asks whether you trust the folder: it offers \"No, exit\" and\n   \"Yes, I trust this folder\", the cursor on the first.\n ❯ \n   ⏵⏵ bypass permissions on (shift+tab to cycle)");
        assert_eq!(toward_trust(&quoted), None);
        assert!(shows_prompt(&quoted));
        // The cursor on neither choice: drawn halfway, read again at the next look.
        let halfway = lines(" Quick safety check\n   No, exit\n   Yes, I trust this folder");
        assert_eq!(toward_trust(&halfway), None);
    }

    #[test]
    fn a_prompt_or_a_question_with_one_choice_alone_is_no_question_about_the_folder() {
        assert_eq!(
            toward_trust(&lines(" ❯ Try \"fix lint errors\"\n bypass permissions on")),
            None
        );
        assert_eq!(toward_trust(&lines("❯ Yes, I trust this folder")), None);
        assert_eq!(toward_trust(&[]), None);
    }
}
