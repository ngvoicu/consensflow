//! Claude Code's question about a folder it was never told to trust, as its window
//! shows it before anything else (2.1.305, `cargo xtask live folder-trust`): the
//! folder, a few lines that ask whether it is a project the person trusts, and two
//! choices, "No, exit" and "Yes, I trust this folder", the `❯` on the first. Enter
//! on "No, exit" ends the window; Down moves to the yes, which Enter confirms, and
//! Claude Code writes the trust down for the root of the folder's git repository
//! and goes on to its prompt, and to the first message it was opened with. Before
//! 2.1.287 the yes came first, numbered: the choice is read by its words.

use crate::shared::trust::{chosen, shows, DOWN, ENTER};

/// The choice that trusts the folder, and the one that ends the window.
const YES: &str = "Yes, I trust this folder";
const NO: &str = "No, exit";

/// The key toward a yes where `screen` shows the question: Enter once the `❯` is
/// on the yes, Down until then.
pub(crate) fn toward_trust(screen: &[String]) -> Option<&'static [u8]> {
    if !(shows(screen, YES) && shows(screen, NO)) {
        return None;
    }
    Some(
        if chosen(screen, '❯').is_some_and(|choice| choice.ends_with(YES)) {
            ENTER
        } else {
            DOWN
        },
    )
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
    fn a_prompt_or_a_question_with_one_choice_alone_is_no_question_about_the_folder() {
        assert_eq!(
            toward_trust(&lines(" ❯ Try \"fix lint errors\"\n bypass permissions on")),
            None
        );
        assert_eq!(toward_trust(&lines("❯ Yes, I trust this folder")), None);
        assert_eq!(toward_trust(&[]), None);
    }
}
