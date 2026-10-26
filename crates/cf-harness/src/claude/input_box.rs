//! Claude Code's input box, as its screen shows it (2.1.309, `cargo xtask live
//! escape-unsent`): a rule across the screen, the box's lines, the first after its
//! `❯`, and a rule again, with the line that says the permission mode under it. Its
//! model picker draws its `❯` on a choice, between no rules, and takes no filter;
//! left with Escape, it leaves the box bare (`❯`). One Escape keeps words typed in
//! the box, two clear them. An empty box in a conversation with nothing said yet
//! shows a suggestion after its `❯` (`Try "…"`), drawn faint, which the screen's
//! text cannot tell from words typed: that box is not read, and the count holds.

use crate::shared::input_box::{boxed, Cursor, InputBox};

/// The suggestion an empty box shows in a new conversation.
const SUGGESTS: &str = "Try \"";

/// What Claude's input box holds, where `screen` shows one: the last box drawn,
/// the cursor where `cursor` says. A box that shows a suggestion is empty where
/// the cursor says so ([`Boxed::only_a_hint`](crate::shared::input_box::Boxed));
/// else it is not read.
pub(crate) fn read(screen: &[String], cursor: Option<Cursor>) -> Option<InputBox> {
    let boxed = boxed(screen, '❯')?;
    let (typed, more) = (boxed.typed, boxed.more);
    if (!more && typed.is_empty()) || boxed.only_a_hint(cursor) {
        return Some(InputBox::Empty);
    }
    (more || !typed.starts_with(SUGGESTS)).then_some(InputBox::Holds)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What the reader makes of `screen`, with no word of the cursor.
    fn read(screen: &[String]) -> Option<InputBox> {
        super::read(screen, None)
    }

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

    /// The cursor at the box's line (the fourth of `boxed`), `col` characters in,
    /// `plain` characters drawn plain from it on.
    fn at(col: usize, plain: usize) -> Option<Cursor> {
        Some(Cursor {
            line: 3,
            col,
            shown: true,
            plain_after: plain,
        })
    }

    #[test]
    fn a_hint_with_the_cursor_at_the_boxs_start_and_nothing_plain_after_it_is_empty() {
        // Whatever its words: the pane host says they are drawn styled, not typed.
        assert_eq!(
            super::read(&boxed("❯ Try \"how do I log an error?\""), at(2, 0)),
            Some(InputBox::Empty)
        );
        assert_eq!(
            super::read(&boxed("  ❯ Try \"how do I log an error?\""), at(4, 0)),
            Some(InputBox::Empty)
        );
        // Words typed and the cursor moved back to their start (Home) are plain.
        assert_eq!(
            super::read(&boxed("❯ probe words"), at(2, 10)),
            Some(InputBox::Holds)
        );
        // The cursor elsewhere, on another line, or a second line in the box.
        assert_eq!(
            super::read(&boxed("❯ probe words"), at(13, 0)),
            Some(InputBox::Holds)
        );
        let elsewhere = Some(Cursor {
            line: 5,
            col: 2,
            shown: true,
            plain_after: 0,
        });
        assert_eq!(
            super::read(&boxed("❯ probe words"), elsewhere),
            Some(InputBox::Holds)
        );
        assert_eq!(
            super::read(&boxed("❯ Try \"how do I log an error?\"\n  more"), at(2, 0)),
            Some(InputBox::Holds)
        );
    }
}
