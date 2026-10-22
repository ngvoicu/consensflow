//! Claude Code's input box, as its screen shows it (2.1.309, `cargo xtask live
//! escape-unsent`): a rule across the screen, the box's lines, the first after its
//! `❯`, and a rule again, with the line that says the permission mode under it. Its
//! model picker draws its `❯` on a choice, between no rules, and takes no filter;
//! left with Escape, it leaves the box bare (`❯`). One Escape keeps words typed in
//! the box, two clear them. An empty box in a conversation with nothing said yet
//! shows a suggestion after its `❯` (`Try "…"`), drawn faint, which the screen's
//! text cannot tell from words typed: that box is not read, and the count holds.

use crate::shared::input_box::{boxed, InputBox};

/// The suggestion an empty box shows in a new conversation.
const SUGGESTS: &str = "Try \"";

/// What Claude's input box holds, where `screen` shows one: the last box drawn.
pub(crate) fn read(screen: &[String]) -> Option<InputBox> {
    let (typed, more) = boxed(screen, '❯')?;
    if more || !typed.is_empty() {
        return (more || !typed.starts_with(SUGGESTS)).then_some(InputBox::Holds);
    }
    Some(InputBox::Empty)
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
            "⏺ 4\n\n{RULE}\n{inside}\n{RULE}\n  ⏵⏵ bypass permissions on (shift+tab to cycle)"
        ))
    }

    #[test]
    fn a_box_with_nothing_after_its_prompt_is_empty() {
        assert_eq!(read(&boxed("❯ ")), Some(InputBox::Empty));
        assert_eq!(read(&boxed("❯\u{a0}")), Some(InputBox::Empty));
    }

    #[test]
    fn words_in_the_box_on_its_first_line_or_after_it_are_held() {
        assert_eq!(read(&boxed("❯ half a thought")), Some(InputBox::Holds));
        assert_eq!(read(&boxed("❯ \n  a second line")), Some(InputBox::Holds));
        assert_eq!(
            read(&boxed("❯ [Pasted text #1 +20 lines]")),
            Some(InputBox::Holds)
        );
    }

    #[test]
    fn a_suggestion_in_an_empty_box_is_not_read() {
        assert_eq!(read(&boxed("❯ Try \"how do I log an error?\"")), None);
    }

    #[test]
    fn a_picker_or_a_screen_with_no_box_is_not_read() {
        let picker = lines(&format!(
            "{RULE}\n Select model\n ❯ 1. Default (recommended)\n   2. Sonnet\n Enter to confirm · Esc to exit"
        ));
        assert_eq!(read(&picker), None);
        assert_eq!(
            read(&lines("fake agent started on s-1 · bypass permissions on")),
            None
        );
        assert_eq!(read(&[]), None);
        // A box drawn halfway, its closing rule not yet there.
        assert_eq!(read(&lines(&format!("{RULE}\n❯ "))), None);
    }
}
