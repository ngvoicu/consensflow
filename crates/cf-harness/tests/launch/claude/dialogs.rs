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
const LSP: &str = " LSP plugin recommendation\n   Plugin: clangd-lsp\n   Triggered by: .c files\n\n   Would you like to install this LSP plugin?\n\n   \u{276f} 1. Yes, install\n     2. No, not now\n     3. Never for this plugin\n     4. Disable all LSP recommendations\n\n\u{273d} Honking\u{2026} (2s \u{b7} \u{2193} 55 tokens)";

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
            after_ms: None,
            then_tell: None,
            what: "offer".to_owned(),
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

/// Claude Code 2.1.317's ask of a plugin's PreToolUse hook on a Bash call, as a worker's window
/// on the owner's kluris board showed it (2026-11-03, T-1198).
const HOOK_ASK: &str = "\u{2502} git status -s\n\u{254c}\u{254c}\u{254c}\u{254c}\u{254c}\u{254c}\u{254c}\u{254c}\n\u{2502} Hook tool.check requires confirmation for this\n\u{2502} command:\n\u{2502} This call's input holds your account email\n\u{2502} address, which may leave this machine. If nobody\n\u{2502} asked to send it, go on without it.\n\u{2502} [plugin:cc-plugin-privacy]\nplugin hooks.json to update hooks\n\nDo you want to proceed?\n\u{276f} 1. Yes\n  2. No\n\nEsc to cancel \u{b7} Tab to amend";

#[test]
fn a_hooks_ask_is_answered_no_after_ten_minutes_and_the_window_told_to_go_on_with_the_hooks_reason()
{
    let home = Home::new();
    let plan = prepare(&home.adapter(), &Request::default()).unwrap();
    let ask = plan.window.dialog(&lines(HOOK_ASK)).expect("the ask");
    let decline = plan.window.safe_decline(&ask).expect("on the list");
    assert_eq!(decline.choice, "No");
    assert_eq!(decline.key, b"2");
    assert_eq!(decline.after_ms, Some(600_000));
    assert_eq!(decline.what, "hook prompt on its call");
    assert_eq!(
        decline.then_tell.as_deref(),
        Some("A hook asked for confirmation of your last call, and nobody answered it within 10 minutes, so ConsensFlow answered \u{201c}No\u{201d} for you: that call did not run. The hook said: \u{201c}This call's input holds your account email address, which may leave this machine. If nobody asked to send it, go on without it.\u{201d} Go on without that call, or say in your result that you could not do the task without it.")
    );
    // Never "Yes", whatever the order of the choices.
    let swapped = HOOK_ASK.replace("\u{276f} 1. Yes\n  2. No", "\u{276f} 1. No\n  2. Yes");
    let swapped = plan.window.dialog(&lines(&swapped)).expect("the ask");
    assert_eq!(
        plan.window.safe_decline(&swapped).expect("on the list").key,
        b"1"
    );
    // A permission rule's ask, or a classifier's, is no hook's: not on the list.
    let rule = HOOK_ASK.replace(
        "Hook tool.check requires",
        "Permission rule Bash(git:*) requires",
    );
    let rule = plan.window.dialog(&lines(&rule)).expect("the ask");
    assert_eq!(plan.window.safe_decline(&rule), None);
    // An ask that offers no "No" is never pressed.
    let no_no = HOOK_ASK.replace("2. No", "2. Later");
    let no_no = plan.window.dialog(&lines(&no_no)).expect("the ask");
    assert_eq!(plan.window.safe_decline(&no_no), None);
}

/// A Claude Code chief's last lines at rest after a worker's question (T-1240), as the engine
/// reads them (the pane host's tail: no empty line, and no rule that says no word): the line
/// under its box that says it is in full-permission mode is the prompt it draws below any
/// conversation's list, and its dialogs never draw it.
#[test]
fn a_claude_chief_at_rest_after_a_workers_question_shows_no_dialog() {
    use cf_harness::testing::screens::{
        reads_no_dialog, CLAUDE_BOX_HOLDING_LINES, CLAUDE_IDLE_AFTER_QUESTION,
    };
    let home = Home::new();
    let plan = prepare(&home.adapter(), &Request::default()).unwrap();
    reads_no_dialog(&*plan.window, "at rest", CLAUDE_IDLE_AFTER_QUESTION);
    reads_no_dialog(
        &*plan.window,
        "two lines typed under a question",
        CLAUDE_BOX_HOLDING_LINES,
    );
}

#[test]
fn its_offer_to_install_a_plugin_and_its_hooks_ask_are_dialogs_still() {
    use cf_harness::testing::screens::{reads_a_dialog, CLAUDE_HOOK_ASK, CLAUDE_LSP};
    let home = Home::new();
    let plan = prepare(&home.adapter(), &Request::default()).unwrap();
    let offer = reads_a_dialog(&*plan.window, "the LSP offer", CLAUDE_LSP);
    assert_eq!(offer.options.len(), 4);
    let ask = reads_a_dialog(&*plan.window, "the hook's ask", CLAUDE_HOOK_ASK);
    assert_eq!(ask.question, "Do you want to proceed?");
    assert_eq!(ask.options, ["Yes", "No"]);
}

/// T-1243's release bar, over the whole corpus (`cf_harness::testing::corpus`): this window reads no
/// screen as a dialog that `5b7acff7f`'s window read as none, reads every screen labelled a dialog as
/// that window did, lets only idle prompts and conversations read less,
/// and reads none of the idle screens of its own harness as a dialog the engine counts.
#[test]
fn the_claude_window_reads_no_screen_as_a_dialog_that_the_release_head_read_as_none() {
    use cf_harness::testing::corpus::{assert_no_regression, say};
    use cf_proto::agents::Harness;
    let home = Home::new();
    let plan = prepare(&home.adapter(), &Request::default()).unwrap();
    let window = &*plan.window;
    let audit = assert_no_regression(window, Harness::Claude, false);
    for (name, how) in &audit.changed {
        println!("claude window, {how}: {name}");
    }
    println!("{}", say(&audit, Harness::Claude));
}
