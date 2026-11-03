//! A dialog of Claude Code's that waits for a key mid-session (T-1183): its status says one is
//! open, and why, and the window reads which off its screen. Its question tool's dialog is the
//! window's own question, and its offers to install a plugin close by themselves after 30 s.

use cf_harness::contract::{Decline, DialogSign, Observed, Over, Waiting};

use super::{prepare, Home, Request};

/// A look whose harness says the window waits for `reason`, or does not wait.
fn waiting(reason: Option<&str>) -> Observed {
    Observed {
        reading: None,
        settled: false,
        over: Over::No,
        waiting: reason.map(|reason| Waiting {
            reason: Some(reason.to_owned()),
        }),
        failed: false,
        quota: None,
        switched: None,
        unnamed: false,
        took_back: false,
        at_prompt: false,
        shared: false,
    }
}

fn lines(text: &str) -> Vec<String> {
    text.lines().map(str::to_owned).collect()
}

/// Claude Code 2.1.317's offer to install an LSP plugin, as a live member's window drew it.
const LSP: &str = " LSP plugin recommendation\n   Plugin: clangd-lsp\n   Triggered by: .c files\n\n   Would you like to install this LSP plugin?\n\n   \u{276f} 1. Yes, install\n     2. No, not now\n     3. Never for this plugin\n     4. Disable all LSP recommendations\n\n\u{273d} Honking\u{2026} (2s \u{b7} \u{2193} 21 tokens)";

#[test]
fn a_dialog_its_status_says_is_open_is_one_of_claude_codes_and_its_question_tools_is_the_windows_own(
) {
    let home = Home::new();
    let plan = prepare(&home.adapter(), &Request::default()).unwrap();
    let window = &plan.window;
    for reason in [
        "dialog open",
        "permission prompt",
        "sandbox request",
        "goal proposal",
    ] {
        assert_eq!(
            window.dialog_sign(&waiting(Some(reason))),
            DialogSign::Open,
            "{reason}"
        );
    }
    assert_eq!(
        window.dialog_sign(&waiting(Some("input needed"))),
        DialogSign::Shut
    );
    assert_eq!(window.dialog_sign(&waiting(None)), DialogSign::Screen);
}

#[test]
fn its_offer_to_install_an_lsp_plugin_is_read_with_its_choices_and_closes_by_itself() {
    let home = Home::new();
    let plan = prepare(&home.adapter(), &Request::default()).unwrap();
    let dialog = plan.window.dialog(&lines(LSP)).expect("the offer");
    assert_eq!(
        dialog.question,
        "Would you like to install this LSP plugin?"
    );
    assert_eq!(dialog.options.len(), 4);
    assert!(dialog.asks && !dialog.prompt_below);
    assert_eq!(dialog.closes_after_s, Some(30));
    // A command's hint to install a plugin closes so too.
    let hint = LSP.replace(
        "Would you like to install this LSP plugin?",
        "Would you like to install it?",
    );
    assert_eq!(
        plan.window.dialog(&lines(&hint)).unwrap().closes_after_s,
        Some(30)
    );
    // Any other dialog of its is not known to close.
    let other = LSP.replace(
        "Would you like to install this LSP plugin?",
        "Do you want to proceed?",
    );
    assert_eq!(
        plan.window.dialog(&lines(&other)).unwrap().closes_after_s,
        None
    );
}

#[test]
fn choices_listed_above_its_prompt_are_read_with_the_prompt_below() {
    let home = Home::new();
    let plan = prepare(&home.adapter(), &Request::default()).unwrap();
    let screen = lines(
        " ⏺ Which way do you want it?\n ❯ The parser first\n   The tests first\n\n────────────────\n ❯ \n────────────────\n   ⏵⏵ bypass permissions on (shift+tab to cycle)",
    );
    let dialog = plan.window.dialog(&screen).expect("the choices' shape");
    assert!(dialog.prompt_below);
}

#[test]
fn its_list_of_safe_declines_holds_the_lsp_offer_alone_declined_by_the_digit_of_no_not_now() {
    let home = Home::new();
    let plan = prepare(&home.adapter(), &Request::default()).unwrap();
    let offer = plan.window.dialog(&lines(LSP)).expect("the offer");
    assert_eq!(
        plan.window.safe_decline(&offer),
        Some(Decline {
            choice: "No, not now".to_owned(),
            key: b"2".to_vec(),
        })
    );
    // Its hint to install a plugin a command named: every answer but a cancel writes the
    // plugin's name in the owner's `~/.claude.json`, so it is not on the list.
    let hint = LSP
        .replace(
            "Would you like to install this LSP plugin?",
            "Would you like to install it?",
        )
        .replace("2. No, not now", "2. No");
    let hint = plan.window.dialog(&lines(&hint)).expect("the hint");
    assert_eq!(plan.window.safe_decline(&hint), None);
    // A permission prompt is no offer at all.
    let prompt = plan
        .window
        .dialog(&lines(
            " Bash command\n   rm -rf \"$DIR/build\"\n\n Do you want to proceed?\n \u{276f} 1. Yes\n   2. No",
        ))
        .expect("the prompt");
    assert_eq!(plan.window.safe_decline(&prompt), None);
    // Another question that offers "No, not now" is not on the list: only the LSP offer's
    // answer is known to write nothing.
    let other = LSP.replace(
        "Would you like to install this LSP plugin?",
        "Would you like to enable this?",
    );
    let other = plan.window.dialog(&lines(&other)).expect("the other");
    assert_eq!(plan.window.safe_decline(&other), None);
    // The offer drawn without its "No, not now" is not declined by a key that would be another's.
    let without = LSP.replace("2. No, not now", "2. Maybe later");
    let without = plan.window.dialog(&lines(&without)).expect("the offer");
    assert_eq!(plan.window.safe_decline(&without), None);
}
