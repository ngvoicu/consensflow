//! The dialog reader on the screens the harnesses draw, and on the screens that only look like
//! them: a conversation's list, a box, a user's turn.

use super::*;

fn lines(text: &str) -> Vec<String> {
    text.lines().map(str::to_owned).collect()
}

/// Claude Code 2.1.317's LSP offer as its dialog draws it: a frame with its title, what the
/// plugin is, the question and four choices, the cursor on the first.
pub(crate) const CLAUDE_LSP: &str = "\
╭───────────────────────────────────────────────────────────────╮
│ LSP plugin recommendation                                     │
│                                                               │
│ LSP provides code intelligence like go-to-definition and      │
│ error checking                                                │
│                                                               │
│ Plugin:       rust-analyzer-lsp                               │
│ Marketplace:  claude-plugins-official                         │
│ Triggered by: .rs files                                       │
│                                                               │
│ Would you like to install this LSP plugin?                    │
│                                                               │
│ ❯ Yes, install                                                │
│   No, not now                                                 │
│   Never for this plugin                                       │
│   Disable all LSP recommendations                             │
╰───────────────────────────────────────────────────────────────╯";

#[test]
fn claude_codes_lsp_offer_is_a_dialog_with_its_question_and_four_choices() {
    let shown = read(&lines(CLAUDE_LSP)).expect("the offer");
    assert_eq!(
        shown.dialog.question,
        "Would you like to install this LSP plugin?"
    );
    assert_eq!(
        shown.dialog.options,
        [
            "Yes, install",
            "No, not now",
            "Never for this plugin",
            "Disable all LSP recommendations"
        ]
    );
    assert!(shown.dialog.asks);
    assert_eq!(
        shown.below,
        ["╰───────────────────────────────────────────────────────────────╯"]
    );
}

#[test]
fn the_cursor_on_a_later_choice_reads_the_same_choices() {
    let moved = CLAUDE_LSP
        .replace("│ ❯ Yes, install    ", "│   Yes, install    ")
        .replace("│   No, not now    ", "│ ❯ No, not now    ");
    let shown = read(&lines(&moved)).expect("the offer");
    assert_eq!(shown.dialog.options.len(), 4);
    assert_eq!(shown.dialog.options[1], "No, not now");
}

#[test]
fn numbered_choices_drop_their_numbers_and_codexs_cursor_is_read() {
    let codex = lines(
        "  Approaching rate limits\n  Switch to gpt-5.6-mini for lower credit usage?\n\n› 1. Switch to gpt-5.6-mini\n  2. Keep current model\n  3. Keep current model (never show again)\n\n  Press enter to confirm or esc to go back",
    );
    let shown = read(&codex).expect("the nudge");
    assert_eq!(
        shown.dialog.question,
        "Switch to gpt-5.6-mini for lower credit usage?"
    );
    assert_eq!(
        shown.dialog.options,
        [
            "Switch to gpt-5.6-mini",
            "Keep current model",
            "Keep current model (never show again)"
        ]
    );
}

#[test]
fn a_choices_description_drawn_further_in_is_no_choice() {
    let remote = lines(
        " Enable Remote Control\n Take this session with you and pick up right where you left off on any device.\n\n ❯ Enable Remote Control\n     Opens a secure connection to claude.ai.\n   Never mind\n     You can always enable it later with /remote-control.",
    );
    let shown = read(&remote).expect("the callout");
    assert_eq!(
        shown.dialog.options,
        ["Enable Remote Control", "Never mind"]
    );
    // Its words ask nothing, and the reader says so.
    assert!(!shown.dialog.asks);
    assert_eq!(
        shown.dialog.question,
        "Take this session with you and pick up right where you left off on any device."
    );
}

#[test]
fn a_survey_that_asks_with_a_word_in_brackets_asks() {
    let survey = lines(
        " How is Claude doing this session? (optional)\n ❯ 1. Bad\n   2. Fine\n   3. Good\n   0. Dismiss",
    );
    // Numbered from one, then a nought: not counted in order, so no dialog by its shape.
    assert_eq!(read(&survey), None);
    let counted =
        lines(" How is Claude doing this session? (optional)\n ❯ 1. Bad\n   2. Fine\n   3. Good");
    let shown = read(&counted).expect("the survey");
    assert!(shown.dialog.asks);
    assert_eq!(shown.dialog.options, ["Bad", "Fine", "Good"]);
}

#[test]
fn an_empty_input_box_and_a_box_that_holds_words_are_no_dialog() {
    let empty = lines(" ⏺ Done.\n\n ❯ \n   ⏵⏵ bypass permissions on (shift+tab to cycle)");
    assert_eq!(read(&empty), None);
    let typed = lines(
        " ⏺ Shall I go on?\n\n ❯ yes, and then\n   ⏵⏵ bypass permissions on (shift+tab to cycle)",
    );
    // The footer under the box, drawn where a choice would be, says no words: one choice.
    assert_eq!(read(&typed), None);
    // A box that holds lines, under its rule: what it holds is no choice.
    let held = lines(
        " ⏺ Which way do you want it?\n────────────────────────────────\n ❯ the parser first\n   then the tests\n────────────────────────────────\n   ⏵⏵ bypass permissions on",
    );
    assert_eq!(read(&held), None);
}

#[test]
fn a_conversations_list_without_a_cursor_is_no_dialog() {
    let listed = lines(
        " ⏺ Which way do you want it?\n   1. The parser first\n   2. The tests first\n\n ❯ \n   ⏵⏵ bypass permissions on",
    );
    assert_eq!(read(&listed), None);
}

#[test]
fn choices_numbered_out_of_their_order_are_no_dialog() {
    let quoted = lines(" Which?\n › 2. The second\n   1. The first");
    assert_eq!(read(&quoted), None);
}

#[test]
fn a_cursor_glued_to_its_words_is_no_cursor() {
    let arrows = lines(" What maps where?\n ->a\n ->b");
    assert_eq!(read(&arrows), None);
}

#[test]
fn the_lowest_cursor_is_the_dialogs() {
    // A dialog drawn below a conversation that quoted an older one's choices.
    let both = format!(" ⏺ It asked:\n Old question?\n ❯ Old yes\n   Old no\n\n{CLAUDE_LSP}");
    let shown = read(&lines(&both)).expect("the offer");
    assert_eq!(shown.dialog.options[0], "Yes, install");
}

#[test]
fn a_screen_without_words_above_its_choices_shows_none() {
    assert_eq!(read(&lines(" ❯ Yes\n   No")), None);
}
