//! Claude Code's question about a folder it was never told to trust, as its window
//! shows it before anything else (2.1.305, `cargo xtask live folder-trust`): the
//! folder, a few lines that ask whether it is a project the person trusts, and two
//! choices, "No, exit" and "Yes, I trust this folder", the `❯` on the first. Enter
//! on "No, exit" ends the window; Down moves to the yes, which Enter confirms, and
//! Claude Code writes the trust down for the root of the folder's git repository
//! and goes on to its prompt, and to the first message it was opened with. Before
//! 2.1.287 the yes came first, numbered: the choice is read by its words. Its prompt,
//! once it is past the question, is told by the line under its input box.

use crate::contract::Question;
use crate::shared::trust::{chosen, names, offers, shows, DOWN, ENTER};

/// What Claude's prompt shows as ConsensFlow opens it, in full-permission mode.
const PROMPT: &str = "bypass permissions on";

/// The choice that trusts the folder, and the one that ends the window.
const YES: &str = "Yes, I trust this folder";
const NO: &str = "No, exit";

/// The question, where `screen` shows it, and the key toward a yes: Enter once the
/// `❯` is on the yes, Down while it is on the no. The question is its two choices,
/// each a line of its own: a screen that only names them (a message that quotes the
/// question) asks nothing. A `❯` on a line that is neither is Claude's input box
/// (empty, or holding words that end with a choice), below a conversation that quotes
/// them, each on a line of its own (a list in a message): no question. Where there is
/// no `❯` at all (a cursor drawn otherwise, or a redraw caught halfway), the question
/// is there and its key unknown.
pub(crate) fn toward_trust(screen: &[String]) -> Option<Question> {
    if !(offers(screen, YES) && offers(screen, NO)) {
        return None;
    }
    Some(match chosen(screen, '❯') {
        Some(choice) if names(choice, YES) => Question::Toward(ENTER),
        Some(choice) if names(choice, NO) => Question::Toward(DOWN),
        Some(_) => return None,
        None => Question::Unplaced,
    })
}

/// Whether `screen` is Claude's prompt, as ConsensFlow opens its windows: the
/// line under its input box that says it is in full-permission mode ("⏵⏵ bypass
/// permissions on", 2.1.305), and no question whose choice is placed: the question
/// draws no such line, so one read with its choice unplaced beside it is quoted.
pub(crate) fn shows_prompt(screen: &[String]) -> bool {
    shows(screen, PROMPT) && !matches!(toward_trust(screen), Some(Question::Toward(_)))
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
        assert_eq!(toward_trust(&asked), Some(Question::Toward(DOWN)));
        let on_yes = lines(
            " Quick safety check: Is this a project you created or one you trust?\n   No, exit\n ❯ Yes, I trust this folder\n Enter to confirm · Esc to cancel",
        );
        assert_eq!(toward_trust(&on_yes), Some(Question::Toward(ENTER)));
        // Before 2.1.287 the yes came first, numbered.
        let numbered = lines("❯ 1. Yes, I trust this folder\n  2. No, exit");
        assert_eq!(toward_trust(&numbered), Some(Question::Toward(ENTER)));
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
        // The same before its input box is drawn: no `❯` at all, and still no question.
        assert_eq!(toward_trust(&quoted[..2]), None);
    }

    #[test]
    fn a_question_whose_cursor_is_on_neither_choice_is_asked_with_no_key_known() {
        // Its cursor drawn otherwise than Claude Code 2.1.305 draws it.
        let otherwise = lines(" Quick safety check: Is this a project you created or one you trust?\n > No, exit\n   Yes, I trust this folder\n Enter to confirm · Esc to cancel");
        assert_eq!(toward_trust(&otherwise), Some(Question::Unplaced));
        assert!(!shows_prompt(&otherwise));
        // Drawn halfway, its cursor not yet on a choice.
        let halfway = lines(" Quick safety check\n   No, exit\n   Yes, I trust this folder");
        assert_eq!(toward_trust(&halfway), Some(Question::Unplaced));
        // Numbered, as before 2.1.287.
        assert_eq!(
            toward_trust(&lines("  1. Yes, I trust this folder\n  2. No, exit")),
            Some(Question::Unplaced)
        );
    }

    #[test]
    fn a_conversation_that_lists_the_choices_above_the_input_box_asks_nothing() {
        // Calliope's check of round 4 (T-231): a resumed chief at rest whose last lines
        // quote the dialog, each choice a line of its own.
        let listed = lines(" ⏺ The dialog offers:\n   - No, exit\n   - Yes, I trust this folder\n ❯ \n   ⏵⏵ bypass permissions on (shift+tab to cycle)");
        assert_eq!(toward_trust(&listed), None);
        assert!(shows_prompt(&listed));
        // Its input box holding words that end with a choice is no cursor on it.
        let typed = lines(" ⏺ The dialog offers:\n   - No, exit\n   - Yes, I trust this folder\n ❯ tell me more about No, exit\n   ⏵⏵ bypass permissions on (shift+tab to cycle)");
        assert_eq!(toward_trust(&typed), None);
        assert!(shows_prompt(&typed));
        // The same with no input box drawn yet: unplaced, but the prompt's line wins.
        let tail = lines(" ⏺ The dialog offers:\n   - No, exit\n   - Yes, I trust this folder\n   ⏵⏵ bypass permissions on");
        assert_eq!(toward_trust(&tail), Some(Question::Unplaced));
        assert!(shows_prompt(&tail));
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
