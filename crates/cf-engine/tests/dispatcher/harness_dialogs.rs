//! A dialog of a window's harness that waits for a key mid-session and is not its prompt
//! (T-1183). Claude Code 3.0.6 offered a worker's window to install an LSP plugin, and the
//! chief never heard of it (field, 3.0.6, 2026-10-26). A dialog its harness says is open, or one
//! its screen shows by its shape where its harness says nothing, is told once to whoever gave
//! the window's task and to the human, with its question and its choices; one only the screen
//! shows holds the window as one its harness says it waits on. The window's own question tool
//! is no such dialog, and nor is a conversation that lists choices above the prompt. A dialog on
//! its harness's list of safe declines is declined once the note of it has gone, by its one
//! key, at a look that still shows it (T-1192); no other is ever pressed.

use cf_engine::testing::{
    Context, DECLINED_OFFER, DECLINE_CHOICE, DIALOG_OPEN, HOOK_ASK_MS, HOOK_ASK_QUESTION,
    HOOK_ASK_THEN, SELF_ENDING_DIALOG, TRUST_PROMPT,
};
use cf_engine::ActivityState;
use cf_harness::contract::Waiting;
use cf_harness::records::Role;
use cf_harness::testing::screens::{
    CLAUDE_COMPUTER_USE_PROMPT, CLAUDE_LSP_AT_WORK as LSP_AT_WORK,
    CLAUDE_LSP_TURN_ENDED as LSP_TURN_ENDED,
};
use serde_json::json;

use crate::chiefs::to_human;
use crate::fixtures::Tiers;

/// Claude Code 2.1.317's offer to install an LSP plugin, as a live member's window drew it
/// mid-turn, over its spinner (`cargo xtask live harness-dialogs`, 2026-11-03).
const LSP: [&str; 12] = [
    " LSP plugin recommendation",
    "   Plugin: clangd-lsp",
    "   Marketplace: claude-plugins-official",
    "   Triggered by: .c files",
    "",
    "   Would you like to install this LSP plugin?",
    "",
    "   \u{276f} 1. Yes, install",
    "     2. No, not now",
    "     3. Never for this plugin",
    "     4. Disable all LSP recommendations",
    "\u{273d} Honking\u{2026} (2s \u{b7} \u{2193} 55 tokens)",
];

const QUESTION: &str = "Would you like to install this LSP plugin?";

/// A permission prompt of Claude Code's that full permission does not cover, as its window's
/// last lines show it: no dialog of the test window's list.
const PROMPT: [&str; 7] = [
    " Bash command",
    "   rm -rf \"$DIR/build\"",
    "",
    " Do you want to proceed?",
    " \u{276f} 1. Yes",
    "   2. No",
    "",
];

const PROMPT_QUESTION: &str = "Do you want to proceed?";

/// The keys pressed in zeus's window, by its pane: the chief's window shows the same screen
/// (the host's one) and declines its own.
fn keys_to(context: &Context, tiers: &Tiers<'_>) -> Vec<serde_json::Value> {
    let pane = context
        .dispatcher
        .pane(tiers.id(&tiers.assignee(1)))
        .expect("its pane");
    context
        .host
        .inputs()
        .into_iter()
        .filter(|input| input["id"] == json!(pane.id))
        .map(|input| input["bytes"].clone())
        .collect()
}

/// T-1 given to zeus and at work in its window, its screen showing `screen`.
fn at_work<'a, S: AsRef<str>>(context: &'a Context, screen: &[S]) -> Tiers<'a> {
    let tiers = Tiers::new(context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");
    let lines: Vec<&str> = screen.iter().map(AsRef::as_ref).collect();
    context.host.set_snapshot(json!({ "tail": lines }));
    tiers
}

/// `count` passes, `ms` apart.
fn passes(context: &Context, count: usize, ms: i64) {
    for _ in 0..count {
        context.advance(ms);
        context.pass().unwrap();
        context.settle();
    }
}

/// The notes among `bodies` that tell of a dialog in a window of zeus's. The chief's window
/// shows the same screen (the host's one), and its own dialog is told to the human alone.
fn of_dialogs(bodies: &[String]) -> Vec<String> {
    bodies
        .iter()
        .filter(|body| {
            body.starts_with("@zeus") && body.contains("'s window shows a dialog of its harness")
        })
        .cloned()
        .collect()
}

/// The chief's inbox keeps both queued notes and the notes shown rather than typed.
fn chief_dialog_inbox(context: &Context, tiers: &Tiers<'_>) -> Vec<cf_ledger::MessageView> {
    context
        .ledger
        .borrow()
        .inbox(tiers.id("chief"), 100)
        .unwrap()
        .into_iter()
        .filter(|message| {
            message.body.starts_with("@zeus") && message.body.contains("'s window shows a dialog")
        })
        .collect()
}

fn clear_screen_and_pass(context: &Context) {
    context.host.set_snapshot(json!({ "tail": [TRUST_PROMPT] }));
    passes(context, 12, 1_000);
}

#[test]
fn cln75_a_self_ending_dialog_is_shown_in_the_chiefs_inbox_not_typed() {
    let context = Context::new();
    let screen = [SELF_ENDING_DIALOG, " ❯ 1. Yes", "   2. No"];
    let tiers = at_work(&context, &screen);
    let before = crate::fixtures::given(&context, "chief").len();
    passes(&context, 12, 1_000);
    let notes = chief_dialog_inbox(&context, &tiers);
    assert_eq!(
        notes.len(),
        1,
        "{notes:?}; all chief notes: {:?}",
        tiers.notes("chief")
    );
    assert!(
        notes[0]
            .receipt
            .get("shown")
            .and_then(|v| v.as_str())
            .is_some(),
        "{:?}",
        notes[0]
    );
    assert_eq!(notes[0].state, "read");
    assert!(
        notes[0]
            .body
            .contains("Its harness closes it by itself within 17 s."),
        "{}",
        notes[0].body
    );
    assert_eq!(
        of_dialogs(&to_human(&context, tiers.project.id)),
        [notes[0].body.clone()]
    );
    clear_screen_and_pass(&context);
    assert!(crate::fixtures::given(&context, "chief")[before..]
        .iter()
        .all(|text| !text.contains(SELF_ENDING_DIALOG)));
}

#[test]
fn cln76_review_a_closed_dialog_no_longer_hides_the_background_hold() {
    let context = Context::new();
    let tiers = at_work(&context, &[TRUST_PROMPT]);
    context.adapter.answer_leaving_a_job("zeus", "Watching");
    context.pass().unwrap();
    assert_eq!(
        tiers.task(1).task.aside.as_deref(),
        Some("in the background: 1 command")
    );

    context.adapter.wake("zeus", "<task-notification>\n<task-id>b1</task-id>\n<status>completed</status>\n</task-notification>");
    context.pass().unwrap();
    context
        .host
        .set_snapshot(json!({ "tail": [SELF_ENDING_DIALOG, " ❯ 1. Yes", "   2. No"] }));
    passes(&context, 12, 1_000);
    let notes = chief_dialog_inbox(&context, &tiers);
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert_eq!(
        tiers.task(1).task.aside.as_deref(),
        Some("dialog reported in @zeus-amber-pine's window")
    );

    clear_screen_and_pass(&context);
    context
        .adapter
        .answer_leaving_a_job("zeus", "Still watching");
    context.pass().unwrap();
    let dialog = context.message(notes[0].id);
    assert!(
        dialog.receipt.get("over").is_some(),
        "the closed dialog's shown note is over: {dialog:?}"
    );
    assert_eq!(
        tiers.task(1).task.aside.as_deref(),
        Some("in the background: 1 command")
    );
}

#[test]
fn cln76_review_a_dialog_is_over_when_restart_ends_its_window() {
    let context = Context::new();
    let tiers = at_work(&context, &[TRUST_PROMPT]);
    context.adapter.answer_leaving_a_job("zeus", "Watching");
    context.pass().unwrap();
    context.adapter.wake("zeus", "<task-notification>\n<task-id>b1</task-id>\n<status>completed</status>\n</task-notification>");
    context.pass().unwrap();
    context
        .host
        .set_snapshot(json!({ "tail": [SELF_ENDING_DIALOG, " ❯ 1. Yes", "   2. No"] }));
    passes(&context, 12, 1_000);
    let notes = chief_dialog_inbox(&context, &tiers);
    assert_eq!(notes.len(), 1, "{notes:?}");

    // A daemon restart ends every pane; the project and its task stay suspended as they were.
    context.ledger.borrow_mut().suspend_for_restart().unwrap();
    let dialog = context.message(notes[0].id);
    assert!(
        dialog.receipt.get("over").is_some(),
        "the dialog's window died at restart: {dialog:?}"
    );
    assert_eq!(
        tiers.task(1).task.aside.as_deref(),
        Some("in the background: 1 command")
    );
}

#[test]
fn cln76_review_restart_ends_a_dialog_in_an_already_suspended_project() {
    let context = Context::new();
    let tiers = at_work(&context, &[SELF_ENDING_DIALOG, " ❯ 1. Yes", "   2. No"]);
    passes(&context, 12, 1_000);
    let notes = chief_dialog_inbox(&context, &tiers);
    assert_eq!(notes.len(), 1, "{notes:?}");
    // A Close can leave a window alive when it refuses to close.
    context
        .ledger
        .borrow_mut()
        .set_project_state(tiers.project.id, "suspended")
        .unwrap();
    assert!(context.message(notes[0].id).receipt.get("over").is_none());

    context.ledger.borrow_mut().suspend_for_restart().unwrap();
    let dialog = context.message(notes[0].id);
    assert!(
        dialog.receipt.get("over").is_some(),
        "the daemon restart ended this window too: {dialog:?}"
    );
}

#[test]
fn cln76_review_an_unreadable_screen_does_not_end_a_shown_dialog() {
    let context = Context::new();
    let screen = [SELF_ENDING_DIALOG, " ❯ 1. Yes", "   2. No"];
    let tiers = at_work(&context, &screen);
    passes(&context, 12, 1_000);
    let notes = chief_dialog_inbox(&context, &tiers);
    assert_eq!(notes.len(), 1);
    context.host.set_snapshot(json!({}));
    passes(&context, 12, 1_000);
    assert!(
        context.message(notes[0].id).receipt.get("over").is_none(),
        "an unreadable screen does not say the dialog is gone"
    );
    context.host.set_snapshot(json!({ "tail": screen }));
    passes(&context, 12, 1_000);
    assert_eq!(
        chief_dialog_inbox(&context, &tiers).len(),
        1,
        "the same dialog is not shown twice"
    );
    clear_screen_and_pass(&context);
    assert!(context.message(notes[0].id).receipt.get("over").is_some());
}

#[test]
fn cln76_review_an_open_harness_dialog_is_told_when_its_screen_is_unreadable() {
    let context = Context::new();
    let tiers = at_work(&context, &[TRUST_PROMPT]);
    context.host.set_snapshot(json!({}));
    context.adapter.with("zeus", |agent| {
        agent.waiting = Some(Waiting {
            reason: Some(DIALOG_OPEN.to_owned()),
        });
    });
    passes(&context, 6, 1_000);
    let notes = chief_dialog_inbox(&context, &tiers);
    assert_eq!(
        notes.len(),
        1,
        "the harness says a dialog is open: {notes:?}"
    );
    assert!(notes[0]
        .body
        .contains("its screen shows no words ConsensFlow can read"));
    assert_eq!(
        of_dialogs(&to_human(&context, tiers.project.id)),
        [notes[0].body.clone()]
    );
}

#[test]
fn cln75_a_dialog_consensflow_declines_is_shown_and_the_human_is_told_the_same_words() {
    for screen in [&LSP[..], CU_PROMPT] {
        let context = Context::new();
        let tiers = at_work(&context, screen);
        let before = crate::fixtures::given(&context, "chief").len();
        passes(&context, 12, 1_000);
        let notes = chief_dialog_inbox(&context, &tiers);
        assert_eq!(notes.len(), 1, "{notes:?}");
        assert!(notes[0].receipt.get("shown").is_some(), "{:?}", notes[0]);
        assert_eq!(notes[0].state, "read");
        assert!(
            notes[0].body.contains("ConsensFlow declines it"),
            "{}",
            notes[0].body
        );
        assert_eq!(
            of_dialogs(&to_human(&context, tiers.project.id)),
            [notes[0].body.clone()]
        );
        clear_screen_and_pass(&context);
        assert!(crate::fixtures::given(&context, "chief")[before..]
            .iter()
            .all(|text| !text.contains(&notes[0].body)));
    }
}

#[test]
fn cln75_a_delayed_decline_is_shown_too() {
    let context = Context::new();
    let screen = [
        " Hook test requires confirmation for this command:",
        " Do you want to proceed?",
        " ❯ 1. Yes",
        "   2. No",
    ];
    let tiers = at_work(&context, &screen);
    passes(&context, 12, 1_000);
    let notes = chief_dialog_inbox(&context, &tiers);
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(
        notes[0].body.contains("if nobody does within 10 minutes"),
        "{}",
        notes[0].body
    );
    assert!(notes[0].receipt.get("shown").is_some(), "{:?}", notes[0]);
    assert_eq!(
        of_dialogs(&to_human(&context, tiers.project.id)),
        [notes[0].body.clone()]
    );
    assert_eq!(
        tiers.task(1).task.aside.as_deref(),
        Some("dialog reported in @zeus-amber-pine's window")
    );
    // The pre-existing note that the decline actually happened asks no decision either.
    context.advance(HOOK_ASK_MS);
    context.pass().unwrap();
    let inbox = context
        .ledger
        .borrow()
        .inbox(tiers.id("chief"), 100)
        .unwrap();
    let completion: Vec<_> = inbox
        .iter()
        .filter(|note| note.body.starts_with("Nobody answered @zeus"))
        .collect();
    assert_eq!(completion.len(), 1, "{inbox:?}");
    assert!(
        completion[0].receipt.get("shown").is_some(),
        "{:?}",
        completion[0]
    );
    assert!(to_human(&context, tiers.project.id).contains(&completion[0].body));
}

#[test]
fn cln75_a_dialog_nothing_ends_is_still_typed_and_the_human_told() {
    let context = Context::new();
    let tiers = at_work(&context, &PROMPT);
    let before = crate::fixtures::given(&context, "chief").len();
    passes(&context, 12, 1_000);
    let notes = chief_dialog_inbox(&context, &tiers);
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(notes[0].receipt.get("shown").is_none(), "{:?}", notes[0]);
    assert_eq!(
        of_dialogs(&to_human(&context, tiers.project.id)),
        [notes[0].body.clone()]
    );
    clear_screen_and_pass(&context);
    assert!(crate::fixtures::given(&context, "chief")[before..]
        .iter()
        .any(|text| text.contains(&notes[0].body)));
}

#[test]
fn cln75_a_decline_the_host_refuses_is_not_retried_or_escalated() {
    let context = Context::new();
    let tiers = at_work(&context, CU_PROMPT);
    context.host.refuse_keys.set(true);
    passes(&context, 50, 1_000);
    let inbox = context
        .ledger
        .borrow()
        .inbox(tiers.id("chief"), 100)
        .unwrap();
    let dialog_notes: Vec<_> = inbox
        .iter()
        .filter(|note| {
            note.body.starts_with("@zeus") && note.body.contains("'s window shows a dialog")
                || note.body.starts_with("ConsensFlow declined @zeus")
        })
        .collect();
    assert_eq!(
        dialog_notes.len(),
        2,
        "the original and the existing decline notice only: {inbox:?}"
    );
    assert!(
        dialog_notes
            .iter()
            .all(|note| note.receipt.get("shown").is_some()),
        "{dialog_notes:?}"
    );
    assert_eq!(
        keys_to(&context, &tiers),
        [escape_key()],
        "one refused attempt"
    );
    assert_eq!(
        context
            .dispatcher
            .activity(tiers.id(&tiers.assignee(1)))
            .state,
        ActivityState::Waiting
    );
    assert!(crate::fixtures::given(&context, "chief")
        .iter()
        .all(|text| !text.contains("Computer Use permission prompt")));
}

#[test]
fn a_dialog_its_harness_says_is_open_is_told_once_to_the_tasks_giver_and_the_human() {
    let context = Context::new();
    let tiers = at_work(&context, &PROMPT);
    context.adapter.with("zeus", |agent| {
        agent.waiting = Some(Waiting {
            reason: Some(DIALOG_OPEN.to_owned()),
        });
    });
    // Open a moment only: a dialog answered at once is told of to nobody.
    passes(&context, 1, 1_000);
    assert_eq!(of_dialogs(&tiers.notes("chief")), Vec::<String>::new());
    passes(&context, 4, 1_000);
    let told = of_dialogs(&tiers.notes("chief"));
    assert_eq!(told.len(), 1, "{told:?}");
    let note = &told[0];
    assert!(
        note.contains(
            "'s window shows a dialog of its harness that waits for a key, not its prompt"
        ),
        "{note}"
    );
    assert!(
        note.contains(&format!("\u{201c}{PROMPT_QUESTION}\u{201d}")),
        "{note}"
    );
    for choice in ["Yes", "No"] {
        assert!(
            note.contains(&format!("\u{201c}{choice}\u{201d}")),
            "{note}"
        );
    }
    assert!(note.contains("The window waits on it"), "{note}");
    assert!(note.contains("ConsensFlow presses nothing in it"), "{note}");
    assert_eq!(of_dialogs(&to_human(&context, tiers.project.id)), told);
    // The window's lane says what it waits on, in the dialog's words.
    let activity = context.dispatcher.activity(tiers.id(&tiers.assignee(1)));
    assert_eq!(activity.state, ActivityState::Waiting);
    assert!(
        activity
            .reason
            .as_deref()
            .is_some_and(|reason| reason.contains(PROMPT_QUESTION)),
        "{activity:?}"
    );
    // A dialog not on the list is never pressed, and it is not told of again while it stays.
    passes(&context, 20, 1_000);
    assert_eq!(of_dialogs(&tiers.notes("chief")).len(), 1);
    assert_eq!(keys_to(&context, &tiers), Vec::<serde_json::Value>::new());
}

#[test]
fn a_dialog_only_its_screen_shows_holds_the_window_and_is_told_once() {
    let context = Context::new();
    let tiers = at_work(&context, &LSP);
    // One look is no dialog yet: a redraw caught halfway shows for one.
    passes(&context, 5, 1_000);
    assert_eq!(of_dialogs(&tiers.notes("chief")), Vec::<String>::new());
    passes(&context, 6, 1_000);
    let told = of_dialogs(&tiers.notes("chief"));
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(told[0].contains(QUESTION), "{}", told[0]);
    assert_eq!(of_dialogs(&to_human(&context, tiers.project.id)), told);
    let activity = context.dispatcher.activity(tiers.id(&tiers.assignee(1)));
    assert_eq!(activity.state, ActivityState::Waiting, "{activity:?}");
    // The chief's window shows it too (the host's one screen), at rest: zeus's result, on its
    // way to the chief, waits, as a paste's Enter would answer the dialog.
    let chief_before = crate::fixtures::given(&context, "chief").len();
    context.adapter.answer("zeus", "Parser done");
    passes(&context, 12, 1_000);
    assert_eq!(tiers.task(1).task.state, "done");
    assert_eq!(
        crate::fixtures::given(&context, "chief").len(),
        chief_before
    );
    // Gone, the window goes on, and what waited goes in.
    context.host.set_snapshot(json!({ "tail": [TRUST_PROMPT] }));
    passes(&context, 12, 1_000);
    assert!(crate::fixtures::given(&context, "chief").len() > chief_before);
}

#[test]
fn the_windows_own_question_is_no_dialog_of_its_harness() {
    let context = Context::new();
    let tiers = at_work(&context, &LSP);
    context.adapter.with("zeus", |agent| {
        agent.waiting = Some(Waiting {
            reason: Some("its own question dialog is open".to_owned()),
        });
    });
    let zeus = context
        .dispatcher
        .pane(tiers.id(&tiers.assignee(1)))
        .expect("its pane");
    let looked = |context: &Context| {
        context
            .host
            .screen_looks()
            .iter()
            .filter(|pane| **pane == zeus)
            .count()
    };
    let before = looked(&context);
    passes(&context, 12, 1_000);
    assert_eq!(of_dialogs(&tiers.notes("chief")), Vec::<String>::new());
    assert_eq!(
        of_dialogs(&to_human(&context, tiers.project.id)),
        Vec::<String>::new()
    );
    // Its screen is not even looked at for one while it waits on its own question.
    assert_eq!(looked(&context), before);
}

#[test]
fn choices_a_conversation_lists_above_the_prompt_are_no_dialog() {
    let context = Context::new();
    let screen = [
        " Which way do you want it?",
        " ❯ The parser first",
        "   The tests first",
        TRUST_PROMPT,
    ];
    let tiers = at_work(&context, &screen);
    passes(&context, 12, 1_000);
    assert_eq!(of_dialogs(&tiers.notes("chief")), Vec::<String>::new());
    assert!(!context.host.screen_looks().is_empty());
}

/// A window at rest whose prompt is drawn the way a harness draws its own, with a worker's
/// question on its screen above it (T-1240): the box's first line carries the harness's cursor
/// (`❯ Ask to monitor a running build…`, one of Muse's tips) and the status under it is in the
/// column of its words, and the engine reads the pane host's tail, which has no empty line. By
/// their shape alone they were two choices, and the question above them the dialog's words: a
/// chief at rest was held as waiting for a dialog nobody had opened, and its workers' results
/// waited for it (the live matrix's `question-ask-answer` with a Codex chief, 2026-11-05).
#[test]
fn a_prompt_box_at_rest_under_a_question_is_no_dialog_and_holds_nothing() {
    use cf_harness::testing::screens::{
        DEVIN_BOX_HOLDING_LINES, MUSE_BOX_HOLDING_LINES, MUSE_IDLE_AFTER_QUESTION,
    };
    for (what, screen) in [
        ("a Muse box showing a tip", MUSE_IDLE_AFTER_QUESTION),
        ("a Muse box holding two lines", MUSE_BOX_HOLDING_LINES),
        ("a Devin box holding two lines", DEVIN_BOX_HOLDING_LINES),
    ] {
        let context = Context::new();
        let tiers = at_work(&context, screen);
        passes(&context, 20, 1_000);
        assert_eq!(
            of_dialogs(&tiers.notes("chief")),
            Vec::<String>::new(),
            "{what}"
        );
        assert_eq!(
            of_dialogs(&to_human(&context, tiers.project.id)),
            Vec::<String>::new(),
            "{what}"
        );
        let activity = context.dispatcher.activity(tiers.id(&tiers.assignee(1)));
        assert_ne!(
            activity.state,
            ActivityState::Waiting,
            "{what}: {activity:?}"
        );
        assert!(
            !context.host.screen_looks().is_empty(),
            "{what}: never looked"
        );
        // Nothing is held: zeus's result goes to the chief.
        let chief_before = crate::fixtures::given(&context, "chief").len();
        context.adapter.answer("zeus", "Parser done");
        passes(&context, 12, 1_000);
        assert_eq!(tiers.task(1).task.state, "done", "{what}");
        assert!(
            crate::fixtures::given(&context, "chief").len() > chief_before,
            "{what}: the result waited"
        );
    }
}

/// T-1243 (the review of `9a7fca6cb`): numbered conversation text over an idle prompt, in Pi's and in
/// Muse's windows (the reader every window but Codex's has), is no dialog: `5b7acff7f` read none, a rule
/// that ended a numbered list at the first line that is not numbered read one, and the window was held
/// and the chief and the human were told of a dialog nobody had opened.
#[test]
fn numbered_conversation_text_over_an_idle_prompt_is_no_dialog_and_holds_nothing() {
    use cf_harness::testing::screens::{MUSE_NUMBERED_ECHO_AT_REST, PI_QUOTED_EXAMPLE_AT_REST};
    for (what, screen) in [
        ("Pi's numbered quoted example", PI_QUOTED_EXAMPLE_AT_REST),
        ("Muse's numbered echo", MUSE_NUMBERED_ECHO_AT_REST),
    ] {
        let context = Context::new();
        let tiers = at_work(&context, screen);
        passes(&context, 20, 1_000);
        assert_eq!(
            of_dialogs(&tiers.notes("chief")),
            Vec::<String>::new(),
            "{what}"
        );
        assert_eq!(
            of_dialogs(&to_human(&context, tiers.project.id)),
            Vec::<String>::new(),
            "{what}"
        );
        let activity = context.dispatcher.activity(tiers.id(&tiers.assignee(1)));
        assert_ne!(
            activity.state,
            ActivityState::Waiting,
            "{what}: {activity:?}"
        );
        assert!(
            !context.host.screen_looks().is_empty(),
            "{what}: never looked"
        );
        // Nothing is held: zeus's result goes to the chief.
        let chief_before = crate::fixtures::given(&context, "chief").len();
        context.adapter.answer("zeus", "Parser done");
        passes(&context, 12, 1_000);
        assert_eq!(tiers.task(1).task.state, "done", "{what}");
        assert!(
            crate::fixtures::given(&context, "chief").len() > chief_before,
            "{what}: the result waited"
        );
    }
}

#[test]
fn a_dialog_that_goes_and_comes_again_is_told_again() {
    let context = Context::new();
    let tiers = at_work(&context, &LSP);
    passes(&context, 12, 1_000);
    assert_eq!(of_dialogs(&tiers.notes("chief")).len(), 1);
    context.host.set_snapshot(json!({ "tail": [TRUST_PROMPT] }));
    passes(&context, 12, 1_000);
    assert_eq!(of_dialogs(&tiers.notes("chief")).len(), 1);
    context.host.set_snapshot(json!({ "tail": LSP }));
    passes(&context, 12, 1_000);
    assert_eq!(of_dialogs(&tiers.notes("chief")).len(), 2);
}

#[test]
fn an_offer_on_the_list_is_told_then_declined_once_by_its_one_key() {
    let context = Context::new();
    let tiers = at_work(&context, &LSP);
    // Told first, as any dialog, and the note says it is declined: nothing pressed yet.
    let mut passed = 0;
    while of_dialogs(&tiers.notes("chief")).is_empty() && passed < 20 {
        passes(&context, 1, 1_000);
        passed += 1;
    }
    let told = of_dialogs(&tiers.notes("chief"));
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(
        told[0].contains(&format!(
            "ConsensFlow declines it for the window now, choosing \u{201c}{DECLINE_CHOICE}\u{201d}, which writes nothing"
        )),
        "{}",
        told[0]
    );
    assert_eq!(of_dialogs(&to_human(&context, tiers.project.id)), told);
    assert_eq!(keys_to(&context, &tiers), Vec::<serde_json::Value>::new());
    // The next look that still shows it presses its one key: the digit of "No, not now".
    passes(&context, 6, 1_000);
    assert_eq!(keys_to(&context, &tiers), [json!(b"2")]);
    // Never again while it stays, whatever the screen (a press the window did not take).
    passes(&context, 20, 1_000);
    assert_eq!(keys_to(&context, &tiers), [json!(b"2")]);
    assert_eq!(of_dialogs(&tiers.notes("chief")).len(), 1);
    let _ = DECLINED_OFFER;
}

#[test]
fn a_decline_whose_look_finds_the_dialog_gone_presses_nothing() {
    let context = Context::new();
    let tiers = at_work(&context, &LSP);
    let mut passed = 0;
    while of_dialogs(&tiers.notes("chief")).is_empty() && passed < 20 {
        passes(&context, 1, 1_000);
        passed += 1;
    }
    assert_eq!(of_dialogs(&tiers.notes("chief")).len(), 1);
    // It closed by itself before the next look.
    context.host.set_snapshot(json!({ "tail": [TRUST_PROMPT] }));
    passes(&context, 12, 1_000);
    assert_eq!(keys_to(&context, &tiers), Vec::<serde_json::Value>::new());
}

#[test]
fn a_decline_whose_look_finds_another_dialog_where_its_harness_still_says_one_is_open_presses_nothing(
) {
    let context = Context::new();
    let tiers = at_work(&context, &LSP);
    context.adapter.with("zeus", |agent| {
        agent.waiting = Some(Waiting {
            reason: Some(DIALOG_OPEN.to_owned()),
        });
    });
    passes(&context, 4, 1_000);
    let told = of_dialogs(&tiers.notes("chief"));
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(told[0].contains("ConsensFlow declines it"), "{}", told[0]);
    // Before the next look the offer went and a permission prompt came, its harness saying a
    // dialog is open all along: the key of the offer's decline would answer the prompt.
    context.host.set_snapshot(json!({ "tail": PROMPT }));
    passes(&context, 12, 1_000);
    assert_eq!(keys_to(&context, &tiers), Vec::<serde_json::Value>::new());
    // And where its screen shows nothing it can read, nothing either.
    context.host.set_snapshot(json!({ "tail": [" (drawing)"] }));
    passes(&context, 12, 1_000);
    assert_eq!(keys_to(&context, &tiers), Vec::<serde_json::Value>::new());
}

/// A hook's ask on a call, as Claude Code 2.1.317 drew it in a worker's window on the owner's
/// kluris board (2026-11-03, T-1198): the hook's words, the question, "Yes" and "No".
const HOOK_ASK: [&str; 9] = [
    " Hook tool.check requires confirmation for this command:",
    " This call's input holds your account email address. If nobody asked to send it, go on without it.",
    " [plugin:cc-plugin-privacy]",
    "",
    "Do you want to proceed?",
    "\u{276f} 1. Yes",
    "  2. No",
    "",
    "Esc to cancel \u{b7} Tab to amend",
];

/// The messages handed to zeus's window, oldest first.
fn given_zeus(context: &Context) -> Vec<String> {
    crate::fixtures::given(context, "zeus")
}

/// Passes until zeus's hook ask is told, at most 20 s.
fn until_told(context: &Context, tiers: &Tiers<'_>) {
    let mut passed = 0;
    while of_dialogs(&tiers.notes("chief")).is_empty() && passed < 20 {
        passes(context, 1, 1_000);
        passed += 1;
    }
}

/// The notes to the chief that say ConsensFlow answered a hook's ask of zeus's.
fn answered(tiers: &Tiers<'_>) -> Vec<String> {
    tiers
        .notes("chief")
        .into_iter()
        .filter(|body| body.starts_with("Nobody answered @zeus"))
        .collect()
}

#[test]
fn a_hooks_ask_nobody_answers_is_answered_no_after_ten_minutes_and_its_window_sent_on_to_its_result(
) {
    let context = Context::new();
    let tiers = at_work(&context, &HOOK_ASK);
    until_told(&context, &tiers);
    let told = of_dialogs(&tiers.notes("chief"));
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(
        told[0].contains(
            "Above it, it says: \u{201c}Hook tool.check requires confirmation for this command:"
        ),
        "{}",
        told[0]
    );
    assert!(
        told[0].contains("It is yours to answer in that window; if nobody does within 10 minutes, ConsensFlow answers \u{201c}No\u{201d} for it, which writes nothing, and tells it to go on without that call. It never answers yes."),
        "{}",
        told[0]
    );
    assert_eq!(of_dialogs(&to_human(&context, tiers.project.id)), told);
    // Nothing is pressed before its ten minutes.
    passes(&context, 9, 60_000);
    assert_eq!(keys_to(&context, &tiers), Vec::<serde_json::Value>::new());
    assert_eq!(answered(&tiers), Vec::<String>::new());
    passes(&context, 2, 60_000);
    assert_eq!(keys_to(&context, &tiers), [json!(b"2")], "\"No\", once");
    let session = tiers.assignee(1);
    assert_eq!(
        answered(&tiers),
        [format!("Nobody answered @{session}'s hook prompt on its call in 10 minutes, so ConsensFlow answered \u{201c}No\u{201d} and told it to go on without that call.")]
    );
    assert!(to_human(&context, tiers.project.id)
        .iter()
        .any(|body| body.starts_with("Nobody answered @zeus")));
    // Claude Code's "No" ends the turn: its record says the user interrupted it, and its screen
    // goes back to its prompt.
    let marker = context
        .adapter
        .item(Role::User, "[Request interrupted by user for tool use]");
    context.adapter.with("zeus", |agent| {
        agent.items.push(marker.clone());
        agent.settled = true;
    });
    context.host.set_snapshot(json!({ "tail": [TRUST_PROMPT] }));
    let before = given_zeus(&context).len();
    passes(&context, 6, 5_000);
    // Only the words that send it on go in: no note that its turn was cut, no stop owed.
    let given = given_zeus(&context);
    assert_eq!(given.len(), before + 1, "{given:?}");
    assert!(
        given
            .last()
            .is_some_and(|words| words.contains(HOOK_ASK_THEN)),
        "{given:?}"
    );
    assert_eq!(
        keys_to(&context, &tiers),
        [json!(b"2")],
        "never pressed again"
    );
    assert_eq!(tiers.task(1).task.state, "working");
    // Its next turn ends with its answer: the task's result.
    context.adapter.answer("zeus", "Done, without the email");
    passes(&context, 6, 5_000);
    let task = tiers.task(1);
    assert_eq!(task.task.state, "done", "{task:?}");
    assert!(
        task.messages
            .iter()
            .any(|message| message.kind == "result"
                && message.body.contains("Done, without the email")),
        "{task:?}"
    );
    let _ = (HOOK_ASK_MS, HOOK_ASK_QUESTION);
}

#[test]
fn a_hooks_ask_a_human_answers_before_its_ten_minutes_is_never_pressed() {
    let context = Context::new();
    let tiers = at_work(&context, &HOOK_ASK);
    until_told(&context, &tiers);
    assert_eq!(of_dialogs(&tiers.notes("chief")).len(), 1);
    passes(&context, 5, 60_000);
    // The human answers it in the window: the ask is gone, and the turn goes on.
    context.host.set_snapshot(json!({ "tail": [TRUST_PROMPT] }));
    let before = given_zeus(&context).len();
    passes(&context, 12, 60_000);
    assert_eq!(keys_to(&context, &tiers), Vec::<serde_json::Value>::new());
    assert_eq!(answered(&tiers), Vec::<String>::new());
    assert_eq!(
        given_zeus(&context).len(),
        before,
        "no words of ConsensFlow's go in"
    );
}

#[test]
fn a_hooks_ask_whose_window_shows_another_dialog_at_its_ten_minutes_is_never_pressed() {
    let context = Context::new();
    let tiers = at_work(&context, &HOOK_ASK);
    context.adapter.with("zeus", |agent| {
        agent.waiting = Some(Waiting {
            reason: Some(DIALOG_OPEN.to_owned()),
        });
    });
    until_told(&context, &tiers);
    assert_eq!(of_dialogs(&tiers.notes("chief")).len(), 1);
    // The human answered it, and the window's next call asks a permission rule's question.
    passes(&context, 5, 60_000);
    context.host.set_snapshot(json!({ "tail": PROMPT }));
    passes(&context, 8, 60_000);
    assert_eq!(keys_to(&context, &tiers), Vec::<serde_json::Value>::new());
    assert_eq!(answered(&tiers), Vec::<String>::new());
}

// ----- Claude Code's countdown and its LSP offer are told once (T-1277) -----
//
// A dialog is told of once, and a decline is pressed only into the dialog that was told of. The two
// are held to what the dialog's own window says tells it from another read of its screen
// (`Window::dialog_identity`): Claude Code's window blanks the time in its own warning line, and
// begins its LSP offer at the offer's own title; every other window's identity is the words as
// they were read. Telling a dialog twice is a nuisance; telling two dialogs as one is a failure
// (the second request is never told, and a decline meant for the first is pressed on it). The
// tests that say "two notices" are those of T-1273's reviews of T-1261: they hold on `967dbd48b`
// and must go on holding.

/// zeus's turn ends under the dialog on its screen: the tail slides, and the harness says a
/// dialog of its own is open.
fn turn_ends_under(context: &Context, screen: &[&str]) {
    context.host.set_snapshot(json!({ "tail": screen }));
    context.adapter.with("zeus", |agent| {
        agent.waiting = Some(Waiting {
            reason: Some(DIALOG_OPEN.to_owned()),
        });
    });
}

/// The notes to the chief and to the human that tell of a dialog in zeus's window.
fn told_counts(context: &Context, tiers: &Tiers<'_>) -> (usize, usize) {
    (
        of_dialogs(&tiers.notes("chief")).len(),
        of_dialogs(&to_human(context, tiers.project.id)).len(),
    )
}

#[test]
fn an_offer_whose_turn_ends_under_it_is_told_once_and_declined_by_its_key_once() {
    // The kit's runs of 2026-11-06 (T-1178, T-1261): told at its second look with the turn at
    // work; the turn ended before the look the decline waited for, and the line that closes a
    // turn slid into the words above the offer. Nothing was pressed, and the offer closed by
    // itself after its 30 s, counting against the owner's own offers.
    let context = Context::new();
    let tiers = at_work(&context, LSP_AT_WORK);
    until_told(&context, &tiers);
    assert_eq!(told_counts(&context, &tiers), (1, 1));
    turn_ends_under(&context, LSP_TURN_ENDED);
    // At the next look, as the offer's 30 s leave no room to wait for a second one.
    passes(&context, 6, 1_000);
    assert_eq!(keys_to(&context, &tiers), [json!(b"2")], "\"No, not now\"");
    passes(&context, 12, 1_000);
    assert_eq!(keys_to(&context, &tiers), [json!(b"2")], "once");
    assert_eq!(told_counts(&context, &tiers), (1, 1));
}

#[test]
fn an_offer_closing_by_itself_as_its_status_flips_back_is_not_told_again() {
    // The failed run's second pair of notes: the offer had closed, the status went from
    // `waiting` back to `busy` in a step that was no look, and the stale read of the slid tail was
    // counted as a dialog told of the first time. Every step of a five-second span is tried.
    for offset in 0..5 {
        let context = Context::new();
        let tiers = at_work(&context, LSP_AT_WORK);
        until_told(&context, &tiers);
        turn_ends_under(&context, LSP_TURN_ENDED);
        passes(&context, 12 + offset, 1_000);
        context.adapter.with("zeus", |agent| agent.waiting = None);
        context.host.set_snapshot(json!({ "tail": [TRUST_PROMPT] }));
        passes(&context, 12, 1_000);
        assert_eq!(told_counts(&context, &tiers), (1, 1), "offset {offset}");
    }
}

#[test]
fn an_offer_whose_status_says_nothing_is_told_once_though_its_tail_slides() {
    // Where its harness says nothing of it, the screen alone: the slid tail is the same offer.
    let context = Context::new();
    let tiers = at_work(&context, LSP_AT_WORK);
    until_told(&context, &tiers);
    context.host.set_snapshot(json!({ "tail": LSP_TURN_ENDED }));
    passes(&context, 30, 1_000);
    assert_eq!(told_counts(&context, &tiers), (1, 1));
    assert_eq!(keys_to(&context, &tiers), [json!(b"2")], "declined once");
}

/// Claude Code 2.1.319's permission prompt of a dangerous `rm`, as the pane host's tail showed it
/// in a worker's window on the owner's board on 2026-11-06 (T-1268: alpha.2 told it twice, two
/// minutes apart), `left` the time the line under its command says is left before Claude Code
/// blocks the request by itself. The paths and the clock are placeholders. The warning is the
/// line nearest the question (its margin is a blank line, which the tail does not keep).
fn rm_prompt(command: &str, left: &str) -> Vec<String> {
    vec![
        " Bash command".to_owned(),
        format!("   {command}"),
        "   Dangerous rm operation on possibly-empty variable path: \"$S/$l\" in `rm \"$S/$l\"` (rewrite it as \"${S:?}/${l:?}\" or use a literal path)".to_owned(),
        format!(" \u{26a0} Claude Code will automatically block this request in {left}, to avoid holding up an unattended session"),
        " Do you want to proceed?".to_owned(),
        " \u{276f} 1. Yes".to_owned(),
        "   2. No".to_owned(),
    ]
}

const RM_COMMAND: &str =
    "$T/tree/app/tests/shots/make.mjs $T/tree/app/tests/shots/windows.cjs $T/scratch/ && ls $T/scratch";

/// The clock Claude Code draws for `seconds` left: `m:ss`.
fn clock(seconds: u32) -> String {
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

/// zeus's window shows `screen(left)` for each of the seconds from `from` down to `to`, a pass of
/// a second each, as a countdown ticks; `each` is called before the pass.
fn counting_down(
    context: &Context,
    from: u32,
    to: u32,
    screen: impl Fn(u32) -> Vec<String>,
    mut each: impl FnMut(u32),
) {
    for left in (to..=from).rev() {
        context.host.set_snapshot(json!({ "tail": screen(left) }));
        each(left);
        passes(context, 1, 1_000);
    }
}

#[test]
fn a_permission_prompt_whose_countdown_ticks_is_told_once_though_its_status_goes_from_open_to_nothing(
) {
    // The field's shape (the owner's board, 2026-11-06): the status says a dialog is open all
    // along, and for the last seconds it says nothing, as the prompt is answered by Claude Code's
    // own block; the read then differs from the one told of in the clock alone.
    let context = Context::new();
    let tiers = at_work(&context, &rm_prompt(RM_COMMAND, "1:57"));
    context.adapter.with("zeus", |agent| {
        agent.waiting = Some(Waiting {
            reason: Some(DIALOG_OPEN.to_owned()),
        });
    });
    until_told(&context, &tiers);
    assert_eq!(told_counts(&context, &tiers), (1, 1));
    counting_down(
        &context,
        117,
        3,
        |left| rm_prompt(RM_COMMAND, &clock(left)),
        |left| {
            if left == 8 {
                context.adapter.with("zeus", |agent| agent.waiting = None);
            }
        },
    );
    assert_eq!(told_counts(&context, &tiers), (1, 1));
    assert_eq!(keys_to(&context, &tiers), Vec::<serde_json::Value>::new());
    // The note still quotes the words as they were read, the clock of that look in them.
    let told = of_dialogs(&tiers.notes("chief"));
    assert!(
        told[0].contains("block this request in 1:57,"),
        "{}",
        told[0]
    );
}

#[test]
fn a_dialog_only_its_screen_shows_whose_countdown_ticks_is_told_once() {
    // Where its harness says nothing of it, every look reads another clock.
    let context = Context::new();
    let tiers = at_work(&context, &rm_prompt(RM_COMMAND, "1:57"));
    counting_down(
        &context,
        117,
        3,
        |left| rm_prompt(RM_COMMAND, &clock(left)),
        |_| {},
    );
    assert_eq!(told_counts(&context, &tiers), (1, 1));
}

#[test]
fn a_countdown_the_screen_wraps_before_its_time_is_told_once() {
    // A narrow screen breaks the warning after "in": its second row begins with the time.
    let wrapped = |left: u32| {
        let mut lines = rm_prompt(RM_COMMAND, "-");
        lines[3] = " \u{26a0} Claude Code will automatically block this request in".to_owned();
        lines.insert(
            4,
            format!(
                " {}, to avoid holding up an unattended session",
                clock(left)
            ),
        );
        lines
    };
    let context = Context::new();
    let tiers = at_work(&context, &wrapped(117));
    counting_down(&context, 117, 3, wrapped, |_| {});
    assert_eq!(told_counts(&context, &tiers), (1, 1));
}

#[test]
fn a_dialog_that_follows_another_with_other_words_is_told_though_both_count_down() {
    // The clock is no part of what makes a dialog this one: its command is. Two prompts that
    // differ in a figure that is no clock, one after the other, are two dialogs.
    let first = "$T/tree/scripts/shots.sh $T/scratch/x && rm -rf \"${S:?}/build1\"";
    let second = "$T/tree/scripts/shots.sh $T/scratch/x && rm -rf \"${S:?}/build2\"";
    let context = Context::new();
    let tiers = at_work(&context, &rm_prompt(first, "1:57"));
    counting_down(
        &context,
        117,
        88,
        |left| rm_prompt(first, &clock(left)),
        |_| {},
    );
    assert_eq!(told_counts(&context, &tiers), (1, 1));
    counting_down(
        &context,
        117,
        88,
        |left| rm_prompt(second, &clock(left)),
        |_| {},
    );
    let told = of_dialogs(&tiers.notes("chief"));
    assert_eq!(told.len(), 2, "{told:?}");
    assert!(
        told[0].contains("build1") && !told[0].contains("build2"),
        "{}",
        told[0]
    );
    assert!(told[1].contains("build2"), "{}", told[1]);
}

// T-1273's cases: two different requests are two dialogs, whatever words they share, and the
// first one's pending decline is never pressed into the second. These held on `967dbd48b`
// (the exact words) and are the ones a broad rule about times broke.

/// A hook's ask whose command is `command`, as a worker's window shows it.
fn hook_command(command: &str) -> Vec<String> {
    vec![
        " Hook tool.check requires confirmation for this command:".into(),
        format!(" {command}"),
        " [plugin:review-example]".into(),
        " Do you want to proceed?".into(),
        " \u{276f} 1. Yes".into(),
        "   2. No".into(),
    ]
}

#[test]
fn a_command_that_says_a_time_is_another_request_when_the_time_differs() {
    // `retry in 5s`, `retry in 10s`: neither is a countdown, both are the request.
    let context = Context::new();
    let tiers = at_work(&context, &rm_prompt("printf 'retry in 5s'", "1:57"));
    until_told(&context, &tiers);
    context
        .host
        .set_snapshot(json!({ "tail": rm_prompt("printf 'retry in 10s'", "1:57") }));
    passes(&context, 12, 1_000);
    assert_eq!(told_counts(&context, &tiers), (2, 2));
    assert_eq!(keys_to(&context, &tiers), Vec::<serde_json::Value>::new());
}

#[test]
fn two_plain_commands_are_two_requests() {
    let context = Context::new();
    let tiers = at_work(&context, &rm_prompt("sleep 30", "1:57"));
    until_told(&context, &tiers);
    context
        .host
        .set_snapshot(json!({ "tail": rm_prompt("sleep 45", "1:57") }));
    passes(&context, 12, 1_000);
    assert_eq!(told_counts(&context, &tiers), (2, 2));
}

#[test]
fn a_question_about_a_duration_is_not_a_countdown() {
    let context = Context::new();
    let tiers = at_work(&context, &[" Retry in 5s?", " \u{276f} 1. Yes", "   2. No"]);
    until_told(&context, &tiers);
    context
        .host
        .set_snapshot(json!({ "tail": [" Retry in 10s?", " \u{276f} 1. Yes", "   2. No"] }));
    passes(&context, 12, 1_000);
    assert_eq!(told_counts(&context, &tiers), (2, 2));
}

#[test]
fn a_hooks_ask_for_another_command_never_has_the_first_ones_decline_pressed_into_it() {
    let context = Context::new();
    let tiers = at_work(&context, &hook_command("printf 'retry in 5s'"));
    until_told(&context, &tiers);
    passes(&context, 1, 595_000);
    assert_eq!(keys_to(&context, &tiers), Vec::<serde_json::Value>::new());
    context
        .host
        .set_snapshot(json!({ "tail": hook_command("printf 'retry in 10s'") }));
    passes(&context, 1, 5_000);
    assert_eq!(keys_to(&context, &tiers), Vec::<serde_json::Value>::new());
    assert_eq!(told_counts(&context, &tiers), (2, 2));
    // It has its own ten minutes, from its own note.
    passes(&context, 9, 60_000);
    assert_eq!(keys_to(&context, &tiers), Vec::<serde_json::Value>::new());
    passes(&context, 2, 60_000);
    assert_eq!(keys_to(&context, &tiers), [json!(b"2")]);
}

#[test]
fn a_hooks_ask_whose_command_row_is_cut_from_one_read_is_not_the_one_that_follows() {
    // The command row (the one line that tells `sleep 30` from `sleep 45`) is above the tail for
    // one look: that read is a dialog of its own (told, nothing pressed), and the next request
    // is another. Three notes, none of them lost, and no key into the wrong one.
    let a = hook_command("sleep 30");
    let b = hook_command("sleep 45");
    let mut cut = a.clone();
    cut.remove(1);
    let context = Context::new();
    let tiers = at_work(&context, &a);
    until_told(&context, &tiers);
    context.host.set_snapshot(json!({ "tail": cut }));
    passes(&context, 1, 5_000);
    context.host.set_snapshot(json!({ "tail": b }));
    passes(&context, 12, 1_000);
    let told = of_dialogs(&tiers.notes("chief"));
    assert_eq!(told.len(), 3, "{told:?}");
    assert!(told[2].contains("sleep 45"), "{}", told[2]);
    assert_eq!(keys_to(&context, &tiers), Vec::<serde_json::Value>::new());
}

#[test]
fn a_hooks_ask_for_another_command_after_a_cut_read_does_not_spend_the_first_ones_deadline() {
    let a = hook_command("sleep 30");
    let b = hook_command("sleep 45");
    let mut cut = a.clone();
    cut.remove(1);
    let context = Context::new();
    let tiers = at_work(&context, &a);
    until_told(&context, &tiers);
    passes(&context, 1, 590_000);
    context.host.set_snapshot(json!({ "tail": cut }));
    passes(&context, 1, 5_000);
    context.host.set_snapshot(json!({ "tail": b }));
    passes(&context, 1, 5_000);
    assert_eq!(keys_to(&context, &tiers), Vec::<serde_json::Value>::new());
    let told = of_dialogs(&tiers.notes("chief"));
    assert!(told.last().is_some_and(|note| note.contains("sleep 45")));
}

#[test]
fn a_hooks_ask_whose_command_sits_above_the_tail_is_told_as_another_dialog_and_nothing_is_pressed()
{
    // The next request's command is long and its rows are pushed above the twelve lines of the
    // tail by rows under the question: what is left is the first one's explanation, question and
    // choices, and not the row that told it from the first.
    let a = hook_command("sleep 30");
    let mut b = a.clone();
    b.remove(1);
    b.extend([
        " ctrl+o to expand".to_owned(),
        " 1 background task".to_owned(),
        " shift+tab to cycle permissions".to_owned(),
    ]);
    let context = Context::new();
    let tiers = at_work(&context, &a);
    until_told(&context, &tiers);
    passes(&context, 1, 590_000);
    context.host.set_snapshot(json!({ "tail": b }));
    passes(&context, 6, 5_000);
    assert_eq!(told_counts(&context, &tiers), (2, 2));
    assert_eq!(keys_to(&context, &tiers), Vec::<serde_json::Value>::new());
}

#[test]
fn devins_commands_that_differ_in_a_time_are_two_dialogs() {
    // Devin's command is the parsed question, not a line above it.
    use cf_harness::testing::screens::DEVIN_CALL;
    let make = |duration: &str| {
        DEVIN_CALL
            .iter()
            .map(|line| {
                line.replace(
                    "echo ran > shell.txt",
                    &format!("printf 'retry in {duration}'"),
                )
            })
            .collect::<Vec<_>>()
    };
    let context = Context::new();
    let tiers = at_work(&context, &make("5s"));
    until_told(&context, &tiers);
    context.host.set_snapshot(json!({ "tail": make("10s") }));
    passes(&context, 12, 1_000);
    assert_eq!(told_counts(&context, &tiers), (2, 2));
}

#[test]
fn pis_untimed_selectors_titled_with_a_time_are_two_dialogs() {
    // Pi 1.5.0's selector built without a timeout draws its title as it was given: the 30 and the
    // 45 are the request's, not seconds left. (T-1273: the pane host's rows of the real
    // component.) Pi's window takes nothing out of the words.
    let selector = |seconds: u32| {
        vec![
            format!(" Run the health check? ({seconds}s)"),
            " \u{2192} Yes".to_owned(),
            "   No".to_owned(),
            " \u{2191}\u{2193} navigate  enter select  escape/ctrl+c cancel".to_owned(),
        ]
    };
    let context = Context::new();
    let tiers = at_work(&context, &selector(30));
    until_told(&context, &tiers);
    context.host.set_snapshot(json!({ "tail": selector(45) }));
    passes(&context, 360, 5_000);
    assert_eq!(told_counts(&context, &tiers), (2, 2));
    assert_eq!(keys_to(&context, &tiers), Vec::<serde_json::Value>::new());
}

#[test]
fn hook_reasons_that_say_about_two_and_about_three_minutes_are_two_requests() {
    // T-1273's finding 7: a row of a hook's reason that begins with a time and goes on ", to
    // avoid …" is a hook's own words, and no Claude warning is above it.
    let hook = |minutes: u32| {
        vec![
            "\u{2502} sleep 30".to_owned(),
            "\u{2502} Hook tool.check requires confirmation for this command:".to_owned(),
            format!("\u{2502} about {minutes} minutes, to avoid interrupting the deployment"),
            "\u{2502} [plugin:review-deployment]".to_owned(),
            "Do you want to proceed?".to_owned(),
            "\u{276f} 1. Yes".to_owned(),
            "  2. No".to_owned(),
        ]
    };
    let context = Context::new();
    let tiers = at_work(&context, &hook(2));
    until_told(&context, &tiers);
    context.host.set_snapshot(json!({ "tail": hook(3) }));
    passes(&context, 12, 1_000);
    assert_eq!(told_counts(&context, &tiers), (2, 2));
    assert_eq!(keys_to(&context, &tiers), Vec::<serde_json::Value>::new());
}

#[test]
fn a_hooks_ask_another_request_replaces_while_its_status_says_one_is_open_is_told_and_never_pressed_by_the_firsts_deadline(
) {
    // Where its harness says a dialog is open all along (Claude Code's status), a request that takes
    // the place of the first in the same words and choices is another dialog (T-1300; before, it was
    // taken for the first and never told). The decline of the first, at its ten minutes, is pressed
    // only into a read that is the first (its command the same), never into the request that took
    // its place; the second has its ten minutes from its own note.
    let first = hook_command("sleep 30");
    let second = hook_command("sleep 45");
    let context = Context::new();
    let tiers = at_work(&context, &first);
    context.adapter.with("zeus", |agent| {
        agent.waiting = Some(Waiting {
            reason: Some(DIALOG_OPEN.to_owned()),
        });
    });
    until_told(&context, &tiers);
    passes(&context, 9, 60_000);
    context.host.set_snapshot(json!({ "tail": second }));
    passes(&context, 1, 6_000);
    assert_eq!(told_counts(&context, &tiers), (2, 2), "the second is told");
    // Past the first's ten minutes: nothing is pressed into the second.
    passes(&context, 4, 60_000);
    assert_eq!(keys_to(&context, &tiers), Vec::<serde_json::Value>::new());
    assert_eq!(answered(&tiers), Vec::<String>::new());
    // The second's own ten minutes: "No", once.
    passes(&context, 7, 60_000);
    assert_eq!(keys_to(&context, &tiers), [json!(b"2")]);
    // The first, shown again, is another dialog than the second: told again, its ten minutes its own.
    context.host.set_snapshot(json!({ "tail": first }));
    passes(&context, 1, 6_000);
    assert_eq!(told_counts(&context, &tiers), (3, 3));
    passes(&context, 9, 60_000);
    assert_eq!(keys_to(&context, &tiers), [json!(b"2")]);
    passes(&context, 3, 60_000);
    assert_eq!(keys_to(&context, &tiers), [json!(b"2"), json!(b"2")]);
}

#[test]
fn claude_codes_live_permission_prompt_counting_down_is_told_once_whichever_way_its_status_goes() {
    // The kit's live run of 2026-11-07 (T-1277): 120 looks, the clock 1:59 down to 0:00, the warning
    // wrapped after its time; its status said a permission prompt waited all along. And the field's
    // shape of 2026-11-06, where the status stopped saying so for the last seconds.
    use cf_harness::testing::screens::CLAUDE_RM_PROMPT_LIVE;
    let at = |left: u32| -> Vec<String> {
        CLAUDE_RM_PROMPT_LIVE
            .iter()
            .map(|line| line.replace("in 1:59,", &format!("in {},", clock(left))))
            .collect()
    };
    for status_leaves in [false, true] {
        let context = Context::new();
        let tiers = at_work(&context, &at(119));
        context.adapter.with("zeus", |agent| {
            agent.waiting = Some(Waiting {
                reason: Some(DIALOG_OPEN.to_owned()),
            });
        });
        until_told(&context, &tiers);
        counting_down(&context, 119, 0, at, |left| {
            if status_leaves && left == 6 {
                context.adapter.with("zeus", |agent| agent.waiting = None);
            }
        });
        assert_eq!(
            told_counts(&context, &tiers),
            (1, 1),
            "status leaves: {status_leaves}"
        );
        assert_eq!(keys_to(&context, &tiers), Vec::<serde_json::Value>::new());
    }
}

// ----- A warning quoted in a command is the command's own (T-1282) -----
//
// Claude Code draws its own warning as the last block above its question. A command that holds the
// same words (a here-document that prints them) is command data: two commands that differ only in
// the time they quote are two requests, and the decline of the first is never pressed into the
// second. The reader drops the frame (`│`) of a box's rows, so the words alone do not tell the
// block from a quoted line: its place does.

/// A hook's ask for a command that prints Claude Code's warning, as the screen shows it, `clock` the
/// time in the quoted line, which is wrapped before its time when `wrapped`. The quoted rows are
/// the command's, above the hook's header and reason.
fn warning_as_command_data(clock: &str, wrapped: bool) -> Vec<String> {
    let mut screen = vec!["\u{2502} cat <<'NOTICE'".to_owned()];
    if wrapped {
        screen.push(
            "\u{2502} \u{26a0} Claude Code will automatically block this request in".to_owned(),
        );
        screen.push(format!(
            "\u{2502} {clock}, to avoid holding up an unattended session"
        ));
    } else {
        screen.push(format!(
            "\u{2502} \u{26a0} Claude Code will automatically block this request in {clock}, to avoid holding up an unattended session"
        ));
    }
    screen.extend(
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
    screen
}

#[test]
fn two_commands_that_quote_the_warning_with_another_time_are_two_requests() {
    for wrapped in [false, true] {
        let context = Context::new();
        let tiers = at_work(&context, &warning_as_command_data("1:57", wrapped));
        until_told(&context, &tiers);
        context
            .host
            .set_snapshot(json!({ "tail": warning_as_command_data("0:03", wrapped) }));
        passes(&context, 12, 1_000);
        assert_eq!(told_counts(&context, &tiers), (2, 2), "wrapped: {wrapped}");
        assert_eq!(keys_to(&context, &tiers), Vec::<serde_json::Value>::new());
    }
}

#[test]
fn a_command_that_quotes_the_warning_never_has_the_first_commands_decline_pressed_into_it() {
    for wrapped in [false, true] {
        let context = Context::new();
        let tiers = at_work(&context, &warning_as_command_data("1:57", wrapped));
        until_told(&context, &tiers);
        passes(&context, 1, 595_000);
        assert_eq!(keys_to(&context, &tiers), Vec::<serde_json::Value>::new());
        // The second command replaces the first, five seconds before the first one's ten minutes.
        context
            .host
            .set_snapshot(json!({ "tail": warning_as_command_data("0:03", wrapped) }));
        passes(&context, 1, 5_000);
        assert_eq!(
            keys_to(&context, &tiers),
            Vec::<serde_json::Value>::new(),
            "wrapped: {wrapped}"
        );
        assert_eq!(told_counts(&context, &tiers), (2, 2), "wrapped: {wrapped}");
        // And it has the ten minutes of its own, from its own note.
        passes(&context, 9, 60_000);
        assert_eq!(keys_to(&context, &tiers), Vec::<serde_json::Value>::new());
        passes(&context, 2, 60_000);
        assert_eq!(
            keys_to(&context, &tiers),
            [json!(b"2")],
            "wrapped: {wrapped}"
        );
    }
}

#[test]
fn a_native_countdown_under_a_quoted_one_ticks_alone() {
    // The command quotes the warning with a time, and Claude Code's own warning, the last block
    // above the question, counts down under it: only the native time is a clock.
    let screen = |quoted: &str, native: &str| -> Vec<String> {
        vec![
            " Bash command".to_owned(),
            "\u{2502} cat <<'NOTICE'".to_owned(),
            format!("\u{2502} \u{26a0} Claude Code will automatically block this request in {quoted}, to avoid holding up an unattended session"),
            "\u{2502} NOTICE".to_owned(),
            "\u{2502} Dangerous rm operation on possibly-empty variable path: \"$A/$B\" in `rm \"$A/$B\"`".to_owned(),
            format!(" \u{26a0} Claude Code will automatically block this request in {native}, to avoid holding up an unattended session"),
            " Do you want to proceed?".to_owned(),
            " \u{276f} 1. Yes".to_owned(),
            "   2. No".to_owned(),
        ]
    };
    let context = Context::new();
    let tiers = at_work(&context, &screen("1:57", "1:57"));
    counting_down(
        &context,
        117,
        3,
        |left| screen("1:57", &clock(left)),
        |_| {},
    );
    assert_eq!(told_counts(&context, &tiers), (1, 1));
    // The quoted time is the command's own: another time there is another command.
    counting_down(
        &context,
        117,
        100,
        |left| screen("0:03", &clock(left)),
        |_| {},
    );
    assert_eq!(told_counts(&context, &tiers), (2, 2));
}

// ----- Claude Code's Computer Use permission prompt (T-1300) -----
//
// A member called `request_access`; ConsensFlow.app has Accessibility but not Screen Recording
// (2026-11-07, T-1291's window, Claude Code 2.1.319). Claude Code drew "Computer Use needs macOS
// permissions" with "Open System Settings → Screen Recording" and "Try again", and waited for a
// key. Nothing on the board could clear it, and the chief's tell waited `queued` behind it. A
// grant of Screen Recording takes effect only when ConsensFlow restarts, which stops every window:
// so the prompt is declined in a member's window, with Escape (the one key that neither opens
// System Settings nor asks again), and left to the human in the chief's.

/// The prompt as T-1291's window drew it (the field notice m-7690), over the conversation.
const CU_PROMPT: &[&str] = CLAUDE_COMPUTER_USE_PROMPT;

/// What the member is told once the prompt is closed (the brief of T-1300).
const CU_NOW: &str = "Computer use isn't available in ConsensFlow's windows until ConsensFlow has the Screen Recording permission and has been restarted. Do the task without it.";

/// Escape, as the pane host is given it.
fn escape_key() -> serde_json::Value {
    json!([27])
}

/// zeus's status says a dialog of its own is open.
fn dialog_is_open(context: &Context) {
    context.adapter.with("zeus", |agent| {
        agent.waiting = Some(Waiting {
            reason: Some(DIALOG_OPEN.to_owned()),
        });
    });
}

/// A member's window shows the prompt: it is told, closed by Escape once, and the chief's tell,
/// held behind it, goes in after, with the member's line. `status_open`: whether Claude Code's
/// status says a dialog is open (it said nothing of this one in 2.1.319: the screen is the
/// sign), or the window reads at work.
fn a_member_prompt_is_closed_once_and_the_held_tell_follows(status_open: bool) {
    let context = Context::new();
    let tiers = at_work(&context, CU_PROMPT);
    if status_open {
        dialog_is_open(&context);
    }
    until_told(&context, &tiers);
    let told = of_dialogs(&tiers.notes("chief"));
    assert_eq!(told.len(), 1, "{told:?}");
    for words in [
        "\u{201c}Open System Settings \u{2192} Screen Recording\u{201d}, \u{201c}Try again\u{201d}",
        "ConsensFlow declines it for the window now, by pressing \u{201c}Esc\u{201d}, which closes it and writes nothing",
    ] {
        assert!(told[0].contains(words), "{words:?} is not in {}", told[0]);
    }
    assert_eq!(of_dialogs(&to_human(&context, tiers.project.id)), told);
    // The chief's tell comes while the prompt is open (the field's m-7692): it waits behind it.
    let tell = context.tell(
        tiers.project.id,
        &tiers.assignee(1),
        1,
        "Skip the computer-use hand check",
    );
    assert_eq!(keys_to(&context, &tiers), Vec::<serde_json::Value>::new());
    passes(&context, 2, 1_000);
    assert!(
        !given_zeus(&context)
            .iter()
            .any(|words| words.contains("Skip the computer-use")),
        "{:?}",
        given_zeus(&context)
    );
    assert_eq!(context.message(tell.id).state, "queued");
    // The next look that still shows it presses Escape, once, and never "Try again" or a digit.
    passes(&context, 8, 1_000);
    assert_eq!(keys_to(&context, &tiers), [escape_key()]);
    passes(&context, 20, 1_000);
    assert_eq!(keys_to(&context, &tiers), [escape_key()], "once");
    assert_eq!(of_dialogs(&tiers.notes("chief")).len(), 1);
    // The tell paused the task, so the window has no task in hand and the giver is not told of
    // the press (it was told, with the prompt, that ConsensFlow would press it): the human is.
    let declined: Vec<String> = to_human(&context, tiers.project.id)
        .into_iter()
        .filter(|body| body.starts_with("ConsensFlow declined @zeus"))
        .collect();
    assert_eq!(declined.len(), 1, "{declined:?}");
    assert!(
        declined[0].contains("Computer Use permission prompt by pressing \u{201c}Esc\u{201d}"),
        "{}",
        declined[0]
    );
    // The prompt is closed and the window rests: the held tell goes in on its own. It was queued
    // before the member's line was written, so it goes first, and the line once the member has
    // answered it.
    context.adapter.with("zeus", |agent| {
        agent.waiting = None;
        agent.settled = true;
    });
    context.host.set_snapshot(json!({ "tail": [TRUST_PROMPT] }));
    passes(&context, 8, 5_000);
    let given = given_zeus(&context);
    assert!(
        given
            .iter()
            .any(|words| words.contains("Skip the computer-use hand check")),
        "the held tell never went in: {given:?}"
    );
    assert_eq!(context.message(tell.id).state, "delivered");
    assert!(
        !given.iter().any(|words| words.contains(CU_NOW)),
        "the line goes after the tell: {given:?}"
    );
    context.adapter.answer("zeus", "Skipping the hand check");
    passes(&context, 8, 5_000);
    // The tell paused the task, and a paused task's rows wait for its resume: the member's line
    // is kept for it, and goes in when the chief resumes it.
    assert!(
        !given_zeus(&context)
            .iter()
            .any(|words| words.contains(CU_NOW)),
        "{:?}",
        given_zeus(&context)
    );
    context.resume_task(tiers.project.id, 1, "Go on without it");
    passes(&context, 8, 5_000);
    let given = given_zeus(&context);
    let tell_at = given
        .iter()
        .position(|words| words.contains("Skip the computer-use hand check"))
        .expect("the tell");
    let line_at = given
        .iter()
        .position(|words| words.contains(CU_NOW))
        .unwrap_or_else(|| panic!("the member was not told: {given:?}"));
    assert!(tell_at < line_at, "{given:?}");
    assert_eq!(keys_to(&context, &tiers), [escape_key()], "never again");
}

#[test]
fn a_computer_use_permission_prompt_the_screen_shows_is_told_then_closed_by_escape_once_and_the_held_tell_goes_in_after_it(
) {
    a_member_prompt_is_closed_once_and_the_held_tell_follows(false);
}

#[test]
fn a_computer_use_permission_prompt_its_status_says_is_open_is_told_then_closed_by_escape_once_and_the_held_tell_goes_in_after_it(
) {
    a_member_prompt_is_closed_once_and_the_held_tell_follows(true);
}

#[test]
fn a_computer_use_permission_prompt_in_the_chiefs_window_is_told_to_the_human_and_never_closed() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    context.pass().unwrap();
    context.pass().unwrap();
    let pane = context
        .dispatcher
        .pane(tiers.id("chief"))
        .expect("the chief's pane");
    context.host.set_snapshot(json!({ "tail": CU_PROMPT }));
    context.adapter.with("chief", |agent| {
        agent.waiting = Some(Waiting {
            reason: Some(DIALOG_OPEN.to_owned()),
        });
    });
    passes(&context, 60, 1_000);
    let told: Vec<String> = to_human(&context, tiers.project.id)
        .into_iter()
        .filter(|body| body.starts_with("@chief's window shows a dialog of its harness"))
        .collect();
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(
        told[0].contains("Open System Settings \u{2192} Screen Recording"),
        "{}",
        told[0]
    );
    assert!(
        told[0]
            .ends_with("ConsensFlow presses nothing in it: answer it in that window, or leave it."),
        "{}",
        told[0]
    );
    let pressed: Vec<serde_json::Value> = context
        .host
        .inputs()
        .into_iter()
        .filter(|input| input["id"] == json!(pane.id))
        .map(|input| input["bytes"].clone())
        .collect();
    assert_eq!(pressed, Vec::<serde_json::Value>::new());
}

/// The notes to the human that ConsensFlow declined zeus's prompt.
fn declined_to_human(context: &Context, tiers: &Tiers<'_>) -> Vec<String> {
    to_human(context, tiers.project.id)
        .into_iter()
        .filter(|body| body.starts_with("ConsensFlow declined @zeus"))
        .collect()
}

/// Passes until Escape was pressed into zeus's window, at most 30 s.
fn until_escaped(context: &Context, tiers: &Tiers<'_>) {
    let mut passed = 0;
    while keys_to(context, tiers).is_empty() && passed < 30 {
        passes(context, 1, 1_000);
        passed += 1;
    }
}

#[test]
fn a_member_at_work_on_its_task_is_told_to_do_it_without_computer_use_once_the_prompt_is_closed() {
    let context = Context::new();
    let tiers = at_work(&context, CU_PROMPT);
    until_escaped(&context, &tiers);
    assert_eq!(keys_to(&context, &tiers), [escape_key()]);
    // The note of the press goes to the chief, who gave the task, and to the human.
    let declined: Vec<String> = tiers
        .notes("chief")
        .into_iter()
        .filter(|body| body.starts_with("ConsensFlow declined @zeus"))
        .collect();
    assert_eq!(declined.len(), 1, "{declined:?}");
    assert_eq!(declined_to_human(&context, &tiers), declined);
    // Not before its turn is over: the line is a message of the board's, as any.
    passes(&context, 4, 5_000);
    assert!(!given_zeus(&context)
        .iter()
        .any(|words| words.contains(CU_NOW)));
    context.adapter.with("zeus", |agent| {
        agent.settled = true;
    });
    context.host.set_snapshot(json!({ "tail": [TRUST_PROMPT] }));
    passes(&context, 8, 5_000);
    let line: Vec<String> = given_zeus(&context)
        .into_iter()
        .filter(|words| words.contains(CU_NOW))
        .collect();
    assert_eq!(line.len(), 1, "{line:?}");
    assert!(line[0].contains("T-1"), "{}", line[0]);
    assert_eq!(keys_to(&context, &tiers), [escape_key()], "never again");
}

/// The prompt over a conversation whose last row is `row`: the tail slides as the model goes on.
fn cu_prompt_over(row: &str) -> Vec<String> {
    let mut screen: Vec<String> = CU_PROMPT.iter().map(|line| (*line).to_owned()).collect();
    screen[0] = row.to_owned();
    screen
}

#[test]
fn a_prompt_whose_conversation_above_it_slides_is_told_once_and_closed_once() {
    let context = Context::new();
    let tiers = at_work(
        &context,
        &cu_prompt_over("\u{23fa} Reading the log, 0 lines so far"),
    );
    let mut lines = 0;
    for _ in 0..40 {
        lines += 7;
        context.host.set_snapshot(json!({
            "tail": cu_prompt_over(&format!("\u{23fa} Reading the log, {lines} lines so far"))
        }));
        passes(&context, 1, 1_000);
    }
    assert_eq!(of_dialogs(&tiers.notes("chief")).len(), 1);
    assert_eq!(keys_to(&context, &tiers), [escape_key()], "once");
}

#[test]
fn a_prompt_that_closed_before_the_look_is_never_pressed_escape_would_interrupt_the_turn() {
    let context = Context::new();
    let tiers = at_work(&context, CU_PROMPT);
    until_told(&context, &tiers);
    assert_eq!(of_dialogs(&tiers.notes("chief")).len(), 1);
    // The human answered it in the window before the next look.
    context.host.set_snapshot(json!({ "tail": [TRUST_PROMPT] }));
    passes(&context, 20, 1_000);
    assert_eq!(keys_to(&context, &tiers), Vec::<serde_json::Value>::new());
    assert_eq!(declined_to_human(&context, &tiers), Vec::<String>::new());
    assert!(!given_zeus(&context)
        .iter()
        .any(|words| words.contains(CU_NOW)));
}

#[test]
fn another_dialog_where_the_prompt_was_is_never_pressed_escape_would_refuse_it() {
    let context = Context::new();
    let tiers = at_work(&context, CU_PROMPT);
    dialog_is_open(&context);
    until_told(&context, &tiers);
    assert_eq!(of_dialogs(&tiers.notes("chief")).len(), 1);
    // A permission prompt took its place; the status says a dialog is open all along.
    context.host.set_snapshot(json!({ "tail": PROMPT }));
    passes(&context, 20, 1_000);
    assert_eq!(keys_to(&context, &tiers), Vec::<serde_json::Value>::new());
}

/// The words of the prompt in a command's output, a file shown or a log: the choices carry
/// no cursor of the harness.
const CU_QUOTED: [&str; 8] = [
    "\u{23fa} Bash(cat computer-use-screen.txt)",
    "  Computer Use needs macOS permissions",
    "  Accessibility: \u{2714} granted",
    "  Screen Recording: \u{2718} not granted",
    "  Grant the missing permissions in System Settings, then select \"Try again\".",
    "  Open System Settings \u{2192} Screen Recording",
    "  Try again",
    "  Enter to confirm \u{b7} Esc to cancel",
];

/// The same, as a recorded screen of the prompt keeps them: the cursor on its first choice.
const CU_QUOTED_WITH_CURSOR: [&str; 9] = [
    "\u{23fa} Bash(cat computer-use-screen.txt)",
    "  Computer Use needs macOS permissions",
    "  Accessibility: \u{2714} granted",
    "  Screen Recording: \u{2718} not granted",
    "  Grant the missing permissions in System Settings, then select \"Try again\".",
    "  \u{276f} Open System Settings \u{2192} Screen Recording",
    "    Try again",
    "  Enter to confirm \u{b7} Esc to cancel",
    "Ready for work (a test's prompt)",
];

#[test]
fn words_of_the_prompt_in_a_commands_output_are_no_dialog_and_get_no_key() {
    for status_open in [false, true] {
        let context = Context::new();
        let tiers = at_work(&context, &CU_QUOTED);
        if status_open {
            dialog_is_open(&context);
        }
        passes(&context, 40, 1_000);
        assert_eq!(
            keys_to(&context, &tiers),
            Vec::<serde_json::Value>::new(),
            "status open: {status_open}"
        );
        assert_eq!(declined_to_human(&context, &tiers), Vec::<String>::new());
        assert!(!given_zeus(&context)
            .iter()
            .any(|words| words.contains(CU_NOW)));
    }
}

#[test]
fn a_recorded_screen_of_the_prompt_shown_above_the_harnesss_own_prompt_gets_no_key() {
    // A worker that prints the field's screen (this very test's fixture) shows the cursor and
    // the choices; the harness's prompt is drawn under them, and it is that which waits.
    for status_open in [false, true] {
        let context = Context::new();
        let tiers = at_work(&context, &CU_QUOTED_WITH_CURSOR);
        if status_open {
            dialog_is_open(&context);
        }
        passes(&context, 40, 1_000);
        assert_eq!(
            keys_to(&context, &tiers),
            Vec::<serde_json::Value>::new(),
            "status open: {status_open}"
        );
        assert_eq!(declined_to_human(&context, &tiers), Vec::<String>::new());
    }
}

#[test]
fn a_dialog_with_the_prompts_choices_and_another_title_gets_no_key() {
    let mut screen: Vec<String> = CU_PROMPT.iter().map(|line| (*line).to_owned()).collect();
    screen[3] = "  Something else needs your attention".to_owned();
    let context = Context::new();
    let tiers = at_work(&context, &screen);
    passes(&context, 40, 1_000);
    assert_eq!(of_dialogs(&tiers.notes("chief")).len(), 1, "still told");
    assert_eq!(keys_to(&context, &tiers), Vec::<serde_json::Value>::new());
}

#[test]
fn a_prompt_with_a_choice_it_does_not_offer_gets_no_key() {
    let mut screen: Vec<String> = CU_PROMPT.iter().map(|line| (*line).to_owned()).collect();
    screen.insert(10, "    Never ask again".to_owned());
    let context = Context::new();
    let tiers = at_work(&context, &screen);
    passes(&context, 40, 1_000);
    assert_eq!(of_dialogs(&tiers.notes("chief")).len(), 1, "still told");
    assert_eq!(keys_to(&context, &tiers), Vec::<serde_json::Value>::new());
}

// ----- No row of a request is cut on words alone (T-1300, after T-1316's review) -----
//
// T-1306's window, 2026-11-07 about 23:4x, Claude Code 2.1.319 on 3.0.7-alpha.3: Claude Code's
// prompt of a dangerous `rm` of a mounted drive's path was told to the chief twice, m-7776 at 1:57
// and m-7780 at 0:05. The first read began at `⎿  Waiting…`, the row of the tool that waits, then
// the dialog's title and its description; the second began at the description: the tail is twelve
// rows, and the rows above the request slid out of it as rows below changed. A first attempt cut
// every row up to a title Claude Code draws, in the first four rows; T-1316 showed that a command
// that prints "Bash command", or a hook's reason that is "Bash command", is a title as far as the
// words go, and that the cut then erased the command above it: two commands became one identity,
// the second was never told, and the first's pending "No" was pressed into it. The words of the
// two notices below are the field's; the rows between them, the dialog's own layout, are those of
// the live recorded prompt of a dangerous `rm` (`CLAUDE_RM_PROMPT_LIVE`).

/// The prompt as the first notice read it, over the tool that waits (its row is drawn with a
/// no-break space after the glyph), its title and its request, `clock` left to block it.
fn mounted_drive_prompt(clock: &str) -> Vec<String> {
    [
        "  \u{23bf} \u{a0}Waiting\u{2026}",
        " Bash command",
        " Delete the sparse bundle by its literal path",
        " rm -rf /Volumes/Data/consensflow-builds.sparsebundle; echo \"rm finished rc=$?\"",
        " \u{2502} Dangerous rm operation on mounted drive path: /Volumes/Data/consensflow-builds.sparsebundle",
    ]
    .map(str::to_owned)
    .into_iter()
    .chain(mounted_drive_prompt_below(clock))
    .collect()
}

/// What is under the request, the same in both reads.
fn mounted_drive_prompt_below(clock: &str) -> Vec<String> {
    vec![
        format!(" \u{26a0} Claude Code will automatically block this request in {clock}, to avoid holding up an unattended session"),
        " Do you want to proceed?".to_owned(),
        " \u{276f} 1. Yes".to_owned(),
        "   2. No".to_owned(),
        " Esc to cancel \u{b7} Tab to amend".to_owned(),
    ]
}

/// The prompt as the second notice read it: the tool's row and the title slid out of the tail, so
/// that it begins at the description.
fn mounted_drive_prompt_slid(clock: &str) -> Vec<String> {
    mounted_drive_prompt(clock).split_off(2)
}

#[test]
fn the_field_pair_is_told_twice_never_merged_and_the_countdown_alone_does_not_tell_it_again() {
    // The accepted cost (the chief, 2026-11-08): nothing the engine reads says the two rows that
    // slid out were the conversation's and not the request's, so the second read is another dialog
    // and an extra notice is told. The countdown ticking under each read is still blanked.
    for open_all_along in [true, false] {
        let context = Context::new();
        let tiers = at_work(&context, &mounted_drive_prompt("1:57"));
        if open_all_along {
            dialog_is_open(&context);
        }
        until_told(&context, &tiers);
        assert_eq!(told_counts(&context, &tiers), (1, 1));
        let told = of_dialogs(&tiers.notes("chief"));
        assert!(told[0].contains("\u{23bf}"), "{}", told[0]);
        counting_down(
            &context,
            116,
            60,
            |left| mounted_drive_prompt(&clock(left)),
            |_| {},
        );
        assert_eq!(told_counts(&context, &tiers), (1, 1), "ticking alone: once");
        // The status stops saying a dialog is open for the last seconds, as the field's did.
        context.adapter.with("zeus", |agent| agent.waiting = None);
        counting_down(
            &context,
            59,
            3,
            |left| mounted_drive_prompt_slid(&clock(left)),
            |_| {},
        );
        assert_eq!(
            told_counts(&context, &tiers),
            (2, 2),
            "the slid read is another dialog, told once; open all along: {open_all_along}"
        );
        assert_eq!(keys_to(&context, &tiers), Vec::<serde_json::Value>::new());
    }
}

/// A hook's ask whose command is `command` (a row each), its reason `reason` (a row each), as the
/// tail shows it: the rows of the command and of the reason are framed, as the field's were.
fn hook_ask_rows(command: &[&str], reason: &[&str]) -> Vec<String> {
    let mut screen: Vec<String> = command
        .iter()
        .map(|row| format!("\u{2502} {row}"))
        .collect();
    screen.push("\u{2502} Hook tool.check requires confirmation for this command:".to_owned());
    screen.extend(reason.iter().map(|row| format!("\u{2502} {row}")));
    screen.extend(
        [
            "\u{2502} [plugin:review-shell]",
            "plugin hooks.json to update hooks",
            "Do you want to proceed?",
            "\u{276f} 1. Yes",
            "  2. No",
            "Esc to cancel \u{b7} Tab to amend",
        ]
        .map(str::to_owned),
    );
    screen
}

/// B takes A's place under one open status after 595 s of A's ten minutes, and five seconds on:
/// B is told, and nothing is pressed into it; the "No" is pressed once, at B's own ten minutes.
fn b_replaces_a_late_and_is_told_and_never_pressed_before_its_own_deadline(
    a: &[String],
    b: &[String],
) {
    let context = Context::new();
    let tiers = at_work(&context, a);
    dialog_is_open(&context);
    until_told(&context, &tiers);
    assert_eq!(told_counts(&context, &tiers), (1, 1));
    passes(&context, 1, 595_000);
    context.host.set_snapshot(json!({ "tail": b }));
    passes(&context, 1, 5_000);
    assert_eq!(
        keys_to(&context, &tiers),
        Vec::<serde_json::Value>::new(),
        "the first's No was pressed into a request that was never told"
    );
    assert_eq!(told_counts(&context, &tiers), (2, 2), "B is told");
    let told = of_dialogs(&tiers.notes("chief"));
    assert_ne!(told[0], told[1]);
    // B's own ten minutes: nothing before them, "No" once at them.
    passes(&context, 5, 60_000);
    assert_eq!(keys_to(&context, &tiers), Vec::<serde_json::Value>::new());
    passes(&context, 6, 60_000);
    assert_eq!(
        keys_to(&context, &tiers),
        [json!(b"2")],
        "\"No\", once, into B"
    );
    passes(&context, 12, 60_000);
    assert_eq!(keys_to(&context, &tiers), [json!(b"2")], "never twice");
}

#[test]
fn a_command_that_prints_a_title_does_not_become_the_command_after_it() {
    // T-1316, probes.rs:32: two multi-line commands that print the report line "Bash command".
    let screen = |path: &str| {
        hook_ask_rows(
            &[
                &format!("rm -rf -- {path}"),
                "printf '%s\\n' '",
                "Bash command",
                "cleanup finished'",
            ],
            &["Review this shell command."],
        )
    };
    b_replaces_a_late_and_is_told_and_never_pressed_before_its_own_deadline(
        &screen("$T/review-one"),
        &screen("$T/review-two"),
    );
}

#[test]
fn a_hooks_reason_that_is_a_title_does_not_erase_the_command_above_it() {
    // T-1316, probes.rs:17: the reason "Bash command\n", which 2.1.319's own formatter draws as a
    // bare title row (the offline render of the formatter in the review of T-1316).
    let screen = |path: &str| hook_ask_rows(&[&format!("rm -rf -- {path}")], &["Bash command"]);
    b_replaces_a_late_and_is_told_and_never_pressed_before_its_own_deadline(
        &screen("$T/review-one"),
        &screen("$T/review-two"),
    );
}

#[test]
fn a_command_row_that_slid_out_of_a_request_with_a_title_in_it_is_another_dialog() {
    // T-1316, adapter-probes.rs:18: the read from which the command row slid out is not the read
    // that has it, whatever the rows left say.
    let whole = hook_ask_rows(&["rm -rf -- $T/review-one"], &["Bash command"]);
    let slid = whole[1..].to_vec();
    b_replaces_a_late_and_is_told_and_never_pressed_before_its_own_deadline(&whole, &slid);
}

#[test]
fn a_request_that_prints_the_computer_use_title_is_never_cut_there() {
    // The Computer Use prompt's own cut needs its own choices, read from the cursor rows, which a
    // request's words cannot supply: a Yes/No dialog that prints its title keeps every row.
    let screen = |path: &str| {
        hook_ask_rows(
            &[
                &format!("rm -rf -- {path}"),
                "Computer Use needs macOS permissions",
            ],
            &["Review this shell command."],
        )
    };
    b_replaces_a_late_and_is_told_and_never_pressed_before_its_own_deadline(
        &screen("$T/review-one"),
        &screen("$T/review-two"),
    );
}

#[test]
fn a_title_shaped_row_in_a_request_keeps_two_requests_apart_on_the_screen_path_too() {
    // Where its harness says nothing (no `Open` status), the same two requests are two dialogs.
    let screen = |path: &str| {
        hook_ask_rows(
            &[&format!("rm -rf -- {path}"), "Bash command", "done"],
            &["Review this shell command."],
        )
    };
    let context = Context::new();
    let tiers = at_work(&context, &screen("$T/review-one"));
    until_told(&context, &tiers);
    assert_eq!(told_counts(&context, &tiers), (1, 1));
    context
        .host
        .set_snapshot(json!({ "tail": screen("$T/review-two") }));
    passes(&context, 12, 1_000);
    assert_eq!(told_counts(&context, &tiers), (2, 2));
    assert_eq!(keys_to(&context, &tiers), Vec::<serde_json::Value>::new());
}

#[test]
fn two_requests_whose_tails_slid_alike_but_differ_in_the_command_are_two_prompts() {
    let context = Context::new();
    let tiers = at_work(&context, &mounted_drive_prompt_slid("1:57"));
    until_told(&context, &tiers);
    let other: Vec<String> = mounted_drive_prompt_slid("1:57")
        .into_iter()
        .map(|row| row.replace("consensflow-builds", "consensflow-other"))
        .collect();
    context.host.set_snapshot(json!({ "tail": other }));
    passes(&context, 12, 1_000);
    assert_eq!(told_counts(&context, &tiers), (2, 2));
}

#[test]
fn a_hooks_ask_the_screen_rewrapped_to_the_same_words_never_has_the_firsts_no_pressed_into_it() {
    // The notice's key flattens the rows of what a dialog says; the decline's comparison keeps
    // them (T-1316): a read with the same words in other rows is not told again, and it is not
    // the dialog the "No" was owed to either. The first, shown again, is declined, once.
    let first = hook_ask_rows(&["rm -rf $T/a", "$T/b"], &["Review this shell command."]);
    let rewrapped = hook_ask_rows(&["rm -rf", "$T/a $T/b"], &["Review this shell command."]);
    let context = Context::new();
    let tiers = at_work(&context, &first);
    dialog_is_open(&context);
    until_told(&context, &tiers);
    passes(&context, 9, 60_000);
    context.host.set_snapshot(json!({ "tail": rewrapped }));
    passes(&context, 4, 60_000);
    assert_eq!(
        told_counts(&context, &tiers),
        (1, 1),
        "the same words: one notice"
    );
    assert_eq!(keys_to(&context, &tiers), Vec::<serde_json::Value>::new());
    assert_eq!(answered(&tiers), Vec::<String>::new());
    context.host.set_snapshot(json!({ "tail": first }));
    passes(&context, 2, 5_000);
    assert_eq!(keys_to(&context, &tiers), [json!(b"2")]);
}

// ----- A second dialog under one open status is told (T-1300) -----

/// A hook's ask for `command`, as Claude Code 2.1.317 drew one (`HOOK_ASK`), its reason `reason`.
fn hook_ask_for(command: &str, reason: &str) -> Vec<String> {
    vec![
        format!("\u{2502} {command}"),
        "\u{2502} Hook tool.check requires confirmation for this command:".to_owned(),
        format!("\u{2502} {reason}"),
        "\u{2502} [plugin:cc-plugin-privacy]".to_owned(),
        "Do you want to proceed?".to_owned(),
        "\u{276f} 1. Yes".to_owned(),
        "  2. No".to_owned(),
        "Esc to cancel \u{b7} Tab to amend".to_owned(),
    ]
}

#[test]
fn a_second_hook_ask_under_the_same_open_status_is_told_and_has_its_own_ten_minutes() {
    let context = Context::new();
    let tiers = at_work(
        &context,
        &hook_ask_for("git status -s", "Holds your account email address."),
    );
    dialog_is_open(&context);
    until_told(&context, &tiers);
    assert_eq!(told_counts(&context, &tiers), (1, 1));
    // Five minutes on the human answers it, and a second ask opens in its place: the status says a
    // dialog is open all along.
    passes(&context, 5, 60_000);
    assert_eq!(keys_to(&context, &tiers), Vec::<serde_json::Value>::new());
    context.host.set_snapshot(json!({
        "tail": hook_ask_for("git log --oneline", "Holds a secret token.")
    }));
    passes(&context, 7, 1_000);
    assert_eq!(told_counts(&context, &tiers), (2, 2), "the second is told");
    let told = of_dialogs(&tiers.notes("chief"));
    assert!(told[1].contains("git log --oneline"), "{}", told[1]);
    // The first one's ten minutes are over, and nothing is pressed into the second.
    passes(&context, 6, 60_000);
    assert_eq!(keys_to(&context, &tiers), Vec::<serde_json::Value>::new());
    assert_eq!(answered(&tiers), Vec::<String>::new());
    // The second's own ten minutes: "No", once, and the first was never answered.
    passes(&context, 5, 60_000);
    assert_eq!(keys_to(&context, &tiers), [json!(b"2")], "\"No\", once");
    assert_eq!(answered(&tiers).len(), 1, "{:?}", answered(&tiers));
    passes(&context, 12, 60_000);
    assert_eq!(keys_to(&context, &tiers), [json!(b"2")], "never twice");
}

#[test]
fn a_look_that_reads_no_words_under_an_open_status_is_the_dialog_told_of() {
    let context = Context::new();
    let tiers = at_work(
        &context,
        &hook_ask_for("git status -s", "Holds your email."),
    );
    dialog_is_open(&context);
    until_told(&context, &tiers);
    assert_eq!(told_counts(&context, &tiers), (1, 1));
    // A redraw caught halfway: nothing to read, the status still says it is open.
    context.host.set_snapshot(json!({ "tail": [" (drawing)"] }));
    passes(&context, 12, 1_000);
    context.host.set_snapshot(json!({
        "tail": hook_ask_for("git status -s", "Holds your email.")
    }));
    passes(&context, 12, 1_000);
    assert_eq!(told_counts(&context, &tiers), (1, 1));
}

#[test]
fn a_hooks_ask_whose_choices_changed_under_the_same_words_is_never_pressed_with_the_digit_it_was_told_with(
) {
    // The key of a dialog is its words and its question; its choices are held apart. A hook's ask
    // told with "2. No" whose words stay and whose choices become "2. Yes, and don't ask again" and
    // "3. No" would take the "2" of its decline for a yes that is kept for good.
    let context = Context::new();
    let tiers = at_work(
        &context,
        &hook_ask_for("git status -s", "Holds your account email address."),
    );
    dialog_is_open(&context);
    until_told(&context, &tiers);
    assert_eq!(told_counts(&context, &tiers), (1, 1));
    let mut changed = hook_ask_for("git status -s", "Holds your account email address.");
    let at = changed
        .iter()
        .position(|row| row.trim_start().starts_with("2. No"))
        .expect("its No");
    changed[at] = "  2. Yes, and don't ask again".to_owned();
    changed.insert(at + 1, "  3. No".to_owned());
    context.host.set_snapshot(json!({ "tail": changed }));
    passes(&context, 14, 60_000);
    assert_eq!(keys_to(&context, &tiers), Vec::<serde_json::Value>::new());
    assert_eq!(answered(&tiers), Vec::<String>::new());
}
