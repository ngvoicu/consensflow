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

/// The same offer as Claude Code 2.1.317 drew it in a member's window mid-turn, numbered and
/// unframed, over its spinner, as the pane host's snapshot had it (`cargo xtask live
/// harness-dialogs`, 2026-11-03).
const CLAUDE_LSP_LIVE: &str = " LSP plugin recommendation\n\n   LSP provides code intelligence like go-to-definition and error checking\n\n   Plugin: clangd-lsp\n   Marketplace: claude-plugins-official\n   Triggered by: .c files\n   Description: C/C++ language server (clangd) for code intelligence\n\n   Would you like to install this LSP plugin?\n\n   \u{276f} 1. Yes, install\n     2. No, not now\n     3. Never for this plugin\n     4. Disable all LSP recommendations\n\n\u{273d} Honking\u{2026} (2s \u{b7} \u{2193} 55 tokens)";

#[test]
fn claude_codes_lsp_offer_as_a_live_window_drew_it_is_read_numbers_and_spinner_and_all() {
    let shown = read(&lines(CLAUDE_LSP_LIVE)).expect("the offer");
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
    assert_eq!(shown.below.len(), 2);
    // What it says above its question is its own paragraph, the plugin's: a conversation's lines
    // above it, which change as the turn works, are none of it.
    assert_eq!(
        shown.dialog.detail,
        [
            "Plugin: clangd-lsp",
            "Marketplace: claude-plugins-official",
            "Triggered by: .c files",
            "Description: C/C++ language server (clangd) for code intelligence"
        ]
    );
    let below_work = format!("⏺ Writing probe_dialog.c (12s)\n{CLAUDE_LSP_LIVE}");
    assert_eq!(
        read(&lines(&below_work)).unwrap().dialog.detail,
        shown.dialog.detail
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

/// Devin 3000.11.92's dialog of a tool call in its normal permission mode, as `cargo xtask live
/// devin-resume` recorded its rows (2026-10-15; `live_devin_resume/scene.rs`, and the stand-in
/// Devin that draws it, `fake_devin/permission.rs`): its `❭` on "1 Yes", the list numbered
/// without a dot and drawn left of the chosen choice's words, the call above it and the hint of
/// its keys below. No line above it asks.
const DEVIN_CALL: &str = "└ $ echo ran > shell.txt\n❭ 1 Yes  (Approve once)\n2 Yes, switch to accept edits mode\n3 Yes, switch to bypass mode\n4 Edit command\n5 Describe change to command\n6 No\n↑↓ select · ↵ confirm · esc cancel";

#[test]
fn devins_dialog_of_a_call_is_read_by_its_cursor_its_numbers_and_the_hint_of_its_keys() {
    let shown = read(&lines(DEVIN_CALL)).expect("the dialog");
    assert_eq!(
        shown.dialog.options,
        [
            "Yes  (Approve once)",
            "Yes, switch to accept edits mode",
            "Yes, switch to bypass mode",
            "Edit command",
            "Describe change to command",
            "No"
        ]
    );
    assert_eq!(shown.dialog.question, "$ echo ran > shell.txt");
    assert!(
        shown.dialog.asks,
        "the hint of its keys says it waits for one"
    );
    // Without the hint, nothing says it waits for an answer.
    let unhinted = DEVIN_CALL.replace("\n↑↓ select · ↵ confirm · esc cancel", "");
    assert!(!read(&lines(&unhinted)).expect("the list").dialog.asks);
}

#[test]
fn devins_input_box_under_its_rule_is_no_dialog() {
    let boxed = lines(
        " ⏺ Done.\n──────── (bypass permissions on) ─\n❭ Ask Devin to build features, fix bugs, or work on your code\n────────────────────────────────\nSWE-2 Medium",
    );
    assert_eq!(read(&boxed), None);
    // Words typed into it over two lines are no choices either.
    let typed = lines(
        "──────── (bypass permissions on) ─\n❭ 1 thing first\n2 then the other\n────────────────────────────────",
    );
    assert_eq!(read(&typed), None);
}

#[test]
fn a_numbered_list_left_of_a_cursor_is_held_to_its_count() {
    // Numbers that do not follow the chosen one's are no choices of its list.
    let stray = lines(" Which?\n❭ 1 Yes\n3 No");
    assert_eq!(read(&stray), None);
}

/// Claude Code 2.1.317's ask of a plugin's PreToolUse hook on a Bash call, in a worker's window
/// on the owner's kluris board (2026-11-03, T-1198, from the screenshot's text): the call's last
/// lines, a rule, the hook's words in their frame, where its hooks are kept, the question, two
/// choices and the hint of its keys. Its status said nothing of it; its window waited for good.
pub(crate) const CLAUDE_HOOK_ASK: [&str; 16] = [
    "│ \"`KLURIS_VERSION=3.2.2`\"); date(so)",
    "│ PY",
    "│ git status -s",
    "╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌",
    "│ Hook tool.check requires confirmation for this",
    "│ command:",
    "│ This call's input holds your account email",
    "│ address, which may leave this machine. If nobody",
    "│ asked to send it, go on without it.",
    "│ [plugin:cc-plugin-privacy]",
    "plugin hooks.json to update hooks",
    "",
    "Do you want to proceed?",
    "\u{276f} 1. Yes",
    "  2. No",
    "",
];

#[test]
fn a_hooks_ask_is_read_with_its_question_its_two_choices_and_the_hooks_own_words() {
    let mut screen: Vec<String> = CLAUDE_HOOK_ASK
        .iter()
        .map(|line| (*line).to_owned())
        .collect();
    screen.push("Esc to cancel · Tab to amend".to_owned());
    let shown = read(&screen).expect("the ask");
    assert_eq!(shown.dialog.question, "Do you want to proceed?");
    assert!(shown.dialog.asks);
    assert_eq!(shown.dialog.options, ["Yes", "No"]);
    assert_eq!(
        shown.dialog.detail,
        [
            "Hook tool.check requires confirmation for this",
            "command:",
            "This call's input holds your account email",
            "address, which may leave this machine. If nobody",
            "asked to send it, go on without it.",
            "[plugin:cc-plugin-privacy]",
            "plugin hooks.json to update hooks",
        ],
        "the hook's words, back to the rule under the call"
    );
}

#[test]
fn a_dialogs_detail_stops_at_its_rule_and_at_ten_lines() {
    let long: Vec<String> = (1..=14)
        .map(|n| format!(" line {n}"))
        .chain([
            "Proceed?".to_owned(),
            "❯ 1. Yes".to_owned(),
            "  2. No".to_owned(),
        ])
        .collect();
    let shown = read(&long).expect("the ask");
    assert_eq!(shown.dialog.detail.len(), 10);
    assert_eq!(shown.dialog.detail[0], "line 5");
    assert_eq!(
        read(&lines(" ⏺ Done.\n────────\n Proceed?\n❯ 1. Yes\n  2. No"))
            .unwrap()
            .dialog
            .detail,
        Vec::<String>::new()
    );
}

// ─── A harness's own prompt is no list of choices (T-1240) ─────────────────────────────────────
//
// The engine reads the pane host's tail: no empty line and no rule that says no word. The screens
// are `crate::testing::screens`: each harness's last lines at rest after a worker's question,
// drawn as its recorded screens draw them, and the dialogs the inventory lists.

use crate::testing::screens::{
    lines as screen, DEVIN_BOX_HOLDING_LINES, DEVIN_IDLE_AFTER_QUESTION, DEVIN_ON_THE_QUESTION,
    MUSE_BOX_HOLDING_LINES, MUSE_IDLE_AFTER_QUESTION, PI_IDLE_AFTER_QUESTION,
};

#[test]
fn the_cursor_line_under_a_rule_that_says_something_is_an_input_boxs_and_no_choice() {
    // Muse's rule names the voice key, Devin's the mode and the conversation: the tail keeps
    // them, a column in from the box's words, among the lines the choices took in.
    for (what, drawn) in [
        ("Muse, its tip", MUSE_IDLE_AFTER_QUESTION),
        ("Muse, two lines typed", MUSE_BOX_HOLDING_LINES),
        ("Devin, two lines typed", DEVIN_BOX_HOLDING_LINES),
        ("Devin, over its box", DEVIN_ON_THE_QUESTION),
        ("Devin, at rest", DEVIN_IDLE_AFTER_QUESTION),
    ] {
        assert_eq!(read(&screen(drawn)), None, "{what}");
    }
    // Without the rule the same lines are a list, as any conversation's is.
    let ruled = lines(
        "◆ Which way do you want it?\n❯ The parser first\n  The tests first\n── Voice input (⌥ + v to start) ──────────────",
    );
    assert!(read(&ruled).is_some());
    let boxed = lines(
        "◆ Which way do you want it?\n── Voice input (⌥ + v to start) ──────────────\n❯ The parser first\n  The tests first",
    );
    assert_eq!(read(&boxed), None);
}

/// What T-1240's first try did and T-1243 took out: the reader ends a list at a line that is not
/// numbered, or at a hint of keys, and so reads, as a dialog, what the conversation of a harness
/// draws (Pi 1.5.0's own rendering of a fenced example, with its footer under it, was one: `5b7acff7f`
/// read none). The reader reads such a list as it always did, and a window that knows its dialogs'
/// shape reads them (Codex's: `CodexWindow::dialog`).
#[test]
fn a_numbered_list_with_words_in_its_column_under_it_is_no_dialog_whatever_the_words() {
    // Pi 1.5.0's rendering of a question and a fenced example, its footer under it.
    let pi = screen(&[
        " Which approach should we take?",
        "   > 1. Use the table",
        "     2. Test it first",
        "     This is sample quoted text",
        "/work/app",
        "↑5.5k ↓555 R55k CH55.5% $5.555 5.5%/5.5M (auto)                       meta/muse-spark-1.3 • low",
    ]);
    assert_eq!(read(&pi), None);
    // A user's numbered message in Muse, the status of its box in the column of the words.
    let muse = screen(&[
        "❯ 1. Use the table",
        "  2. Test it first",
        "  please decide?",
        "── Voice input (⌥ + v to start) ────────────────────────────────────────────────",
        "  muse-spark-1.3 · minimal · /p/w/app · YOLO",
    ]);
    assert_eq!(read(&muse), None);
    // A hint of keys in the column of numbered choices is one of them, as it was.
    let quoted = screen(&[
        "Which one do you want?",
        "› 1. Use the table",
        "  2. Test it first",
        "  Press enter to confirm or esc to go back",
    ]);
    assert_eq!(read(&quoted), None);
}

#[test]
fn a_screen_with_no_cursor_on_words_shows_none_whatever_it_asks() {
    // Pi draws no cursor on its box, and the line of its stats is no choice.
    assert_eq!(read(&screen(PI_IDLE_AFTER_QUESTION)), None);
}
