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
        begun: Vec::new(),
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
            listed: true,
            members_only: false,
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

/// Claude Code 2.1.319's permission prompt of a dangerous `rm`, as the pane host's tail showed it
/// on the owner's board on 2026-11-06 (T-1268), the paths and the clock placeholders: the line
/// under the command counts down from `left`.
fn rm_prompt(command: &str, left: &str) -> Vec<String> {
    vec![
        " Bash command".to_owned(),
        format!("   {command}"),
        "   Dangerous rm operation on possibly-empty variable path: \"$S/$l\" in `rm \"$S/$l\"`".to_owned(),
        format!(" \u{26a0} Claude Code will automatically block this request in {left}, to avoid holding up an unattended session"),
        " Do you want to proceed?".to_owned(),
        " \u{276f} 1. Yes".to_owned(),
        "   2. No".to_owned(),
    ]
}

/// What a Claude Code window says tells its dialogs apart (T-1277): the time in its own warning
/// line, the offer from its title down, and no time of a request.
#[test]
fn a_claude_window_blanks_the_time_of_its_own_warning_and_no_time_of_a_request() {
    let home = Home::new();
    let plan = prepare(&home.adapter(), &Request::default()).unwrap();
    let window = &plan.window;
    let read = |screen: &[String]| window.dialog(screen).expect("a dialog");
    let command = "$T/tree/shots/make.mjs $T/scratch/";
    // The same request at another clock, in either of the forms Claude Code draws it.
    let first = window.dialog_identity(&read(&rm_prompt(command, "1:57")));
    for left in ["0:03", "9:59", "about 2 minutes", "about 3 seconds"] {
        assert_eq!(
            window.dialog_identity(&read(&rm_prompt(command, left))),
            first,
            "{left}"
        );
    }
    // A time in the command is the request's own: two requests.
    let five = window.dialog_identity(&read(&rm_prompt("printf 'retry in 5s'", "1:57")));
    let ten = window.dialog_identity(&read(&rm_prompt("printf 'retry in 10s'", "1:57")));
    assert_ne!(five, ten);
    let clock = |at: &str| {
        window.dialog_identity(&read(&rm_prompt(
            &format!("printf 'retry in {at}'"),
            "1:57",
        )))
    };
    assert_ne!(clock("1:30"), clock("2:30"));
    // The warning wrapped before its time: the row under it is the same line.
    let wrapped = |left: &str| {
        let mut screen = rm_prompt(command, "-");
        screen[3] = " \u{26a0} Claude Code will automatically block this request in".to_owned();
        screen.insert(
            4,
            format!(" {left}, to avoid holding up an unattended session"),
        );
        window.dialog_identity(&read(&screen))
    };
    assert_eq!(wrapped("1:57"), wrapped("0:03"));
}

/// T-1273's finding 7: a row of a hook's own reason that begins with a time and goes on ", to avoid
/// …" is the hook's words, with no warning of Claude Code's above it.
#[test]
fn a_claude_windows_hook_reasons_that_say_two_and_three_minutes_are_two_requests() {
    let home = Home::new();
    let plan = prepare(&home.adapter(), &Request::default()).unwrap();
    let window = &plan.window;
    let hook = |minutes: u32| {
        let screen: Vec<String> = [
            "\u{2502} sleep 30".to_owned(),
            "\u{2502} Hook tool.check requires confirmation for this command:".to_owned(),
            format!("\u{2502} about {minutes} minutes, to avoid interrupting the deployment"),
            "\u{2502} [plugin:review-deployment]".to_owned(),
            "Do you want to proceed?".to_owned(),
            "\u{276f} 1. Yes".to_owned(),
            "  2. No".to_owned(),
        ]
        .to_vec();
        let dialog = window.dialog(&screen).expect("the hook's ask");
        // It is on the list of safe declines, as the real hook's ask is.
        let decline = window.safe_decline(&dialog).expect("a hook's ask");
        assert_eq!(decline.key, b"2");
        window.dialog_identity(&dialog)
    };
    assert_ne!(hook(2), hook(3));
}

/// The offer as the pane host's tail showed it in the kit's live runs (T-1261), the turn at work
/// and once it ended: one dialog, and the decline of the one is the decline of the other. A tail
/// that cut the title off is another read, and the window still declines it.
#[test]
fn a_claude_windows_offer_is_one_dialog_from_its_title_down_and_a_cut_one_is_still_declined() {
    use cf_harness::testing::screens::{CLAUDE_LSP_AT_WORK, CLAUDE_LSP_TURN_ENDED};
    let home = Home::new();
    let plan = prepare(&home.adapter(), &Request::default()).unwrap();
    let window = &plan.window;
    let read = |screen: &[&str]| {
        window
            .dialog(
                &screen
                    .iter()
                    .map(|line| (*line).to_owned())
                    .collect::<Vec<_>>(),
            )
            .expect("the offer")
    };
    let at_work = read(CLAUDE_LSP_AT_WORK);
    let ended = read(CLAUDE_LSP_TURN_ENDED);
    assert_ne!(
        at_work.detail, ended.detail,
        "the reader sees the line above"
    );
    assert_eq!(
        window.dialog_identity(&at_work),
        window.dialog_identity(&ended)
    );
    assert_eq!(
        window
            .dialog_identity(&ended)
            .detail
            .first()
            .map(String::as_str),
        Some("LSP plugin recommendation")
    );
    // The same decline in either.
    assert_eq!(window.safe_decline(&at_work), window.safe_decline(&ended));
    assert_eq!(window.safe_decline(&ended).expect("a decline").key, b"2");
    // A tail that cut the title (one line more under the offer): another read, still declined.
    let mut cut: Vec<&str> = CLAUDE_LSP_AT_WORK[1..].to_vec();
    cut.push(" (a second status line)");
    let cut = read(&cut);
    assert_ne!(
        window.dialog_identity(&cut),
        window.dialog_identity(&at_work)
    );
    assert!(window.safe_decline(&cut).is_some());
}

/// T-1282's blocker through the real window: a command that prints Claude Code's warning (a
/// here-document), asked about by a hook, above the hook's header and reason. The quoted time is
/// the command's own: two such commands are two requests, though the window's list of safe
/// declines answers both the same way after ten minutes.
#[test]
fn a_claude_windows_command_that_prints_the_warning_is_not_blanked_whatever_its_time() {
    let home = Home::new();
    let plan = prepare(&home.adapter(), &Request::default()).unwrap();
    let window = &plan.window;
    let screen = |clock: &str, wrapped: bool| -> Vec<String> {
        let mut rows = vec!["\u{2502} cat <<'NOTICE'".to_owned()];
        if wrapped {
            rows.push(
                "\u{2502} \u{26a0} Claude Code will automatically block this request in".to_owned(),
            );
            rows.push(format!(
                "\u{2502} {clock}, to avoid holding up an unattended session"
            ));
        } else {
            rows.push(format!("\u{2502} \u{26a0} Claude Code will automatically block this request in {clock}, to avoid holding up an unattended session"));
        }
        rows.extend(
            [
                "\u{2502} NOTICE",
                "\u{2502} Hook tool.check requires confirmation for this command:",
                "\u{2502} Review the generated warning text.",
                "\u{2502} [plugin:review-warning]",
                "Do you want to proceed?",
                "\u{276f} 1. Yes",
                "  2. No",
            ]
            .map(str::to_owned),
        );
        rows
    };
    for wrapped in [false, true] {
        let first = window
            .dialog(&screen("1:57", wrapped))
            .expect("the hook's ask");
        let second = window
            .dialog(&screen("0:03", wrapped))
            .expect("the hook's ask");
        assert_ne!(first, second, "two reads of two commands");
        assert_ne!(
            window.dialog_identity(&first),
            window.dialog_identity(&second),
            "wrapped: {wrapped}"
        );
        // Both are the list's hook ask, answered "No" (2) after ten minutes: held to the identity,
        // the first one's decline is never pressed into the second.
        let decline = window.safe_decline(&first).expect("a hook's ask");
        assert_eq!(decline.key, b"2");
        assert_eq!(decline.after_ms, Some(600_000));
        assert_eq!(window.safe_decline(&second).as_ref(), Some(&decline));
    }
}

// ----- Its Computer Use permission prompt (T-1300) -----

/// The prompt as T-1291's window drew it, the pane host's tail (see the constant).
fn computer_use_prompt() -> Vec<String> {
    cf_harness::testing::screens::CLAUDE_COMPUTER_USE_PROMPT
        .iter()
        .map(|line| (*line).to_owned())
        .collect()
}

#[test]
fn its_computer_use_permission_prompt_is_read_as_a_dialog_that_waits_whatever_its_footer_says() {
    let home = Home::new();
    let plan = prepare(&home.adapter(), &Request::default()).unwrap();
    let dialog = plan
        .window
        .dialog(&computer_use_prompt())
        .expect("the prompt");
    // The wrapped last row of its sentence, as the field's notice quoted it.
    assert_eq!(dialog.question, "Recording.");
    assert_eq!(
        dialog.options,
        [
            "Open System Settings \u{2192} Screen Recording",
            "Try again"
        ]
    );
    assert!(dialog.asks && !dialog.prompt_below);
    // Its status says nothing while it is open (2.1.319's table of dialogs has no `waitingFor`
    // for it), so the screen is the only sign: it waits whatever the footer under it says.
    let mut bare = computer_use_prompt();
    bare.pop();
    let bare = plan
        .window
        .dialog(&bare)
        .expect("the prompt without a footer");
    assert!(bare.asks, "{bare:?}");
    // Any other list of choices that asks nothing is not read as one that asks.
    let mut other = computer_use_prompt();
    other[3] = "  Something else".to_owned();
    other.pop();
    let other = plan.window.dialog(&other).expect("a list");
    assert!(!other.asks, "{other:?}");
}

#[test]
fn its_computer_use_permission_prompt_is_declined_by_escape_alone_in_a_members_window() {
    let home = Home::new();
    let plan = prepare(&home.adapter(), &Request::default()).unwrap();
    let dialog = plan
        .window
        .dialog(&computer_use_prompt())
        .expect("the prompt");
    assert_eq!(
        plan.window.safe_decline(&dialog),
        Some(Decline {
            choice: "Esc".to_owned(),
            key: vec![27],
            after_ms: None,
            then_tell: Some("Computer use isn't available in ConsensFlow's windows until ConsensFlow has the Screen Recording permission and has been restarted. Do the task without it.".to_owned()),
            what: "Computer Use permission prompt".to_owned(),
            listed: false,
            members_only: true,
        })
    );
    // Accessibility missing too: its choice leads the list, and the member is told of both.
    let mut both = computer_use_prompt();
    both[4] = "  Accessibility: \u{2718} not granted".to_owned();
    both.insert(
        8,
        "  \u{276f} Open System Settings \u{2192} Accessibility".to_owned(),
    );
    both[9] = "    Open System Settings \u{2192} Screen Recording".to_owned();
    both.insert(10, "    Try again".to_owned());
    both.remove(11);
    let both = plan.window.dialog(&both).expect("the prompt");
    assert_eq!(both.options.len(), 3, "{both:?}");
    let decline = plan.window.safe_decline(&both).expect("on the list");
    assert_eq!(decline.key, vec![27]);
    assert!(decline
        .then_tell
        .as_deref()
        .is_some_and(|words| words.contains("the Accessibility and Screen Recording permissions")));
}

#[test]
fn its_computer_use_permission_prompt_is_not_declined_where_only_its_words_are_quoted() {
    let home = Home::new();
    let plan = prepare(&home.adapter(), &Request::default()).unwrap();
    // The harness's own prompt drawn under the choices: they are a conversation's words.
    let mut shown = computer_use_prompt();
    shown.extend([
        "\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}".to_owned(),
        "\u{276f} ".to_owned(),
        "\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}".to_owned(),
        "  \u{23f5}\u{23f5} bypass permissions on (shift+tab to cycle)".to_owned(),
    ]);
    match plan.window.dialog(&shown) {
        None => {}
        Some(dialog) => assert_eq!(plan.window.safe_decline(&dialog), None, "{dialog:?}"),
    }
    // No cursor on the choices: no dialog at all.
    let mut plain = computer_use_prompt();
    plain[8] = "  Open System Settings \u{2192} Screen Recording".to_owned();
    assert_eq!(plan.window.dialog(&plain), None);
    // The title in a longer row of a command, a title that is not its own: another dialog.
    let mut quoted = computer_use_prompt();
    quoted[3] = "  $ echo \"Computer Use needs macOS permissions\"".to_owned();
    let quoted = plan.window.dialog(&quoted).expect("a list of choices");
    assert_eq!(plan.window.safe_decline(&quoted), None);
}

#[test]
fn its_computer_use_permission_prompt_is_one_dialog_whatever_slides_above_its_title() {
    let home = Home::new();
    let plan = prepare(&home.adapter(), &Request::default()).unwrap();
    let read = |first: &str, second: &str| {
        let mut screen = computer_use_prompt();
        screen[0] = first.to_owned();
        screen[1] = second.to_owned();
        plan.window.dialog(&screen).expect("the prompt")
    };
    let before = read("\u{23fa} Reading the log", "  3 lines so far");
    let after = read(
        "\u{23fa} Reading the log",
        "  40 lines so far, and a long word more",
    );
    assert_ne!(before.detail, after.detail, "the tail slid");
    assert_eq!(
        plan.window.dialog_identity(&before),
        plan.window.dialog_identity(&after)
    );
    // Its own words are still told apart: the same prompt with Accessibility missing is another.
    let mut other = computer_use_prompt();
    other[4] = "  Accessibility: \u{2718} not granted".to_owned();
    let other = plan.window.dialog(&other).expect("the prompt");
    assert_ne!(
        plan.window.dialog_identity(&before),
        plan.window.dialog_identity(&other)
    );
    // A dialog that has other choices keeps all its words: nothing above its title is cut.
    let mut deny = computer_use_prompt();
    deny[8] = "  \u{276f} Deny, and tell Claude what to do differently (esc)".to_owned();
    deny[9] = "    Allow for this session (1 app)".to_owned();
    let first = plan.window.dialog(&deny).expect("a list");
    deny[0] = "\u{23fa} Another row".to_owned();
    let second = plan.window.dialog(&deny).expect("a list");
    assert_ne!(
        plan.window.dialog_identity(&first),
        plan.window.dialog_identity(&second)
    );
}

/// The prompt as 2.1.319 draws it on the terminal: its dialog's rule across the top, a blank row
/// between its title and its body and between the body's blocks (`gap:1`), the choices without
/// their numbers, the footer under them.
fn computer_use_prompt_drawn() -> Vec<String> {
    [
        "\u{23fa} I'll type that URL into the chief's input, Cmd+click it, and then check the log, with plain and",
        "  Ctrl-click as negative tests.",
        "",
        "\u{23fa} Computer Use[request_access]",
        "",
        "\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}",
        " Computer Use needs macOS permissions",
        "",
        "  Accessibility: \u{2714} granted",
        "  Screen Recording: \u{2718} not granted",
        "",
        "  Grant the missing permissions in System Settings, then select \"Try again\". macOS may require you to restart Claude Code after granting Screen",
        "  Recording.",
        "",
        "  \u{276f} Open System Settings \u{2192} Screen Recording",
        "    Try again",
        "",
        " Enter to confirm \u{b7} Esc to cancel",
    ]
    .map(str::to_owned)
    .to_vec()
}

#[test]
fn its_computer_use_permission_prompt_is_declined_when_the_pane_host_sends_its_tail_of_the_screen_as_drawn(
) {
    let home = Home::new();
    let plan = prepare(&home.adapter(), &Request::default()).unwrap();
    // What the engine reads is the pane host's tail: the last twelve rows with a letter or a
    // digit, no blank row and no rule.
    let tail = cf_proto::panes::word_tail(computer_use_prompt_drawn(), 12);
    assert_eq!(tail.len(), 11, "{tail:?}");
    assert!(tail.iter().all(|row| !row.trim().is_empty()));
    assert!(tail
        .iter()
        .any(|row| row.trim() == "Computer Use needs macOS permissions"));
    let dialog = plan.window.dialog(&tail).expect("the prompt");
    assert_eq!(dialog.options.len(), 2, "{dialog:?}");
    assert!(dialog.asks && !dialog.prompt_below, "{dialog:?}");
    let decline = plan.window.safe_decline(&dialog).expect("on the list");
    assert_eq!(decline.key, vec![27]);
    // A narrow window wraps its sentence in more rows, and the tail still holds the title.
    let mut narrow = computer_use_prompt_drawn();
    narrow.splice(
        11..13,
        [
            "  Grant the missing permissions in System".to_owned(),
            "  Settings, then select \"Try again\". macOS".to_owned(),
            "  may require you to restart Claude Code".to_owned(),
            "  after granting Screen Recording.".to_owned(),
        ],
    );
    let tail = cf_proto::panes::word_tail(narrow, 12);
    let dialog = plan.window.dialog(&tail).expect("the prompt");
    assert!(plan.window.safe_decline(&dialog).is_some(), "{dialog:?}");
}

// ----- No row of a request is cut on words alone (T-1300, after T-1316's review) -----

fn rows(lines: &[&str]) -> Vec<String> {
    lines.iter().map(|line| (*line).to_owned()).collect()
}

/// A hook's ask of a command, framed as the field's were: `command` rows, the hook's header, its
/// `reason` rows, its source, the configuration hint, the question, its choices.
fn hook_ask_of(command: &[&str], reason: &[&str]) -> Vec<String> {
    let mut screen: Vec<String> = command
        .iter()
        .map(|row| format!("\u{2502} {row}"))
        .collect();
    screen.push("\u{2502} Hook tool.check requires confirmation for this command:".to_owned());
    screen.extend(reason.iter().map(|row| format!("\u{2502} {row}")));
    screen.extend(rows(&[
        "\u{2502} [plugin:review-shell]",
        "plugin hooks.json to update hooks",
        "Do you want to proceed?",
        "\u{276f} 1. Yes",
        "  2. No",
        "Esc to cancel \u{b7} Tab to amend",
    ]));
    screen
}

#[test]
fn its_permission_prompt_of_the_field_is_two_identities_at_1_57_and_at_0_05() {
    // T-1306's window, 2026-11-07: the prompt of a dangerous `rm` of a mounted drive's path, told
    // twice (m-7776, m-7780) because the second read began at the request, two rows lower. Nothing
    // the engine reads says those rows were the conversation's, so it stays two (the chief,
    // 2026-11-08: an extra notice is safe, a merged identity is not).
    let home = Home::new();
    let plan = prepare(&home.adapter(), &Request::default()).unwrap();
    let screen = |clock: &str, slid: usize| -> Vec<String> {
        [
            "  \u{23bf} \u{a0}Waiting\u{2026}".to_owned(),
            " Bash command".to_owned(),
            " Delete the sparse bundle by its literal path".to_owned(),
            " rm -rf /Volumes/Data/consensflow-builds.sparsebundle; echo \"rm finished rc=$?\"".to_owned(),
            " \u{2502} Dangerous rm operation on mounted drive path: /Volumes/Data/consensflow-builds.sparsebundle".to_owned(),
            format!(" \u{26a0} Claude Code will automatically block this request in {clock}, to avoid holding up an unattended session"),
            " Do you want to proceed?".to_owned(),
            " \u{276f} 1. Yes".to_owned(),
            "   2. No".to_owned(),
            " Esc to cancel \u{b7} Tab to amend".to_owned(),
        ]
        .into_iter()
        .skip(slid)
        .collect()
    };
    let first = plan.window.dialog(&screen("1:57", 0)).expect("the prompt");
    let ticked = plan.window.dialog(&screen("0:05", 0)).expect("the prompt");
    let slid = plan.window.dialog(&screen("0:05", 2)).expect("the prompt");
    assert_eq!(
        plan.window.dialog_identity(&first),
        plan.window.dialog_identity(&ticked),
        "the countdown alone is blanked"
    );
    assert_ne!(
        plan.window.dialog_identity(&first),
        plan.window.dialog_identity(&slid)
    );
}

#[test]
fn its_permission_dialog_keeps_every_row_of_a_request_that_prints_a_title() {
    // T-1316, adapter-probes.rs:8, :18 and :26, through the real window: a command that prints
    // "Bash command", a hook's reason that is that title, the command row slid out of the tail.
    let home = Home::new();
    let plan = prepare(&home.adapter(), &Request::default()).unwrap();
    let window = &plan.window;
    let identity = |lines: &[String]| {
        let dialog = window.dialog(lines).expect("the dialog");
        let made = window.dialog_identity(&dialog);
        assert_eq!(made.detail, dialog.detail, "no row of the request is cut");
        made
    };
    // A hook's reason that is a title.
    let reason = |path: &str| hook_ask_of(&[&format!("rm -rf -- {path}")], &["Bash command"]);
    assert_ne!(
        identity(&reason("$T/review-one")),
        identity(&reason("$T/review-two"))
    );
    // The command row slid out, the title left in the reason.
    let whole = reason("$T/review-one");
    assert_ne!(identity(&whole), identity(&whole[1..]));
    // A multi-line command that prints the title, in a twelve-row tail that begins under the dialog's
    // own title (what the tail keeps of the field's layout).
    let command = |path: &str| {
        let mut screen = rows(&[
            "  \u{23bf} Waiting\u{2026}",
            " Bash command",
            " Clean up and print a report",
        ]);
        screen.extend(hook_ask_of(
            &[
                &format!("rm -rf -- {path}"),
                "printf '%s\\n' '",
                "Bash command",
                "cleanup finished'",
            ],
            &["Review this shell command."],
        ));
        screen[screen.len() - 12..].to_vec()
    };
    let (one, two) = (command("$T/review-one"), command("$T/review-two"));
    assert!(window.dialog(&one).unwrap().detail[0].contains("rm -rf"));
    assert_ne!(identity(&one), identity(&two));
    // Every title Claude Code draws, as a row of the command or of the reason.
    for title in [
        "Bash command",
        "Run command",
        "PowerShell command",
        "Edit file",
        "Create file",
        "Overwrite file",
        "Read file",
        "Fetch",
        "Use this skill?",
        "Tool use",
        "Bash command (unsandboxed)",
        "Computer Use needs macOS permissions",
    ] {
        let as_command =
            |path: &str| hook_ask_of(&[&format!("rm -rf -- {path}"), title], &["Why."]);
        let as_reason = |path: &str| hook_ask_of(&[&format!("rm -rf -- {path}")], &[title]);
        assert_ne!(
            identity(&as_command("$T/review-one")),
            identity(&as_command("$T/review-two")),
            "{title}"
        );
        assert_ne!(
            identity(&as_reason("$T/review-one")),
            identity(&as_reason("$T/review-two")),
            "{title}"
        );
    }
}
