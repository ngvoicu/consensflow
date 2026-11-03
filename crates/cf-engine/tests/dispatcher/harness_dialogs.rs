//! A dialog of a window's harness that waits for a key mid-session and is not its prompt
//! (T-1183). Claude Code 3.0.6 offered a worker's window to install an LSP plugin, and the
//! chief never heard of it (field, 3.0.6, 2026-10-26). A dialog its harness says is open, or one
//! its screen shows by its shape where its harness says nothing, is told once to whoever gave
//! the window's task and to the human, with its question and its choices; one only the screen
//! shows holds the window as one its harness says it waits on. The window's own question tool
//! is no such dialog, and nor is a conversation that lists choices above the prompt.

use cf_engine::testing::{Context, DIALOG_OPEN, TRUST_PROMPT};
use cf_engine::ActivityState;
use cf_harness::contract::Waiting;
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
    "\u{273d} Honking\u{2026} (2s \u{b7} \u{2193} 21 tokens)",
];

const QUESTION: &str = "Would you like to install this LSP plugin?";

/// T-1 given to zeus and at work in its window, its screen showing `screen`.
fn at_work<'a>(context: &'a Context, screen: &[&str]) -> Tiers<'a> {
    let tiers = Tiers::new(context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");
    context.host.set_snapshot(json!({ "tail": screen }));
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

#[test]
fn a_dialog_its_harness_says_is_open_is_told_once_to_the_tasks_giver_and_the_human() {
    let context = Context::new();
    let tiers = at_work(&context, &LSP);
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
        note.contains(&format!("\u{201c}{QUESTION}\u{201d}")),
        "{note}"
    );
    for choice in [
        "Yes, install",
        "No, not now",
        "Never for this plugin",
        "Disable all LSP recommendations",
    ] {
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
            .is_some_and(|reason| reason.contains(QUESTION)),
        "{activity:?}"
    );
    // Nothing is pressed in it, and it is not told of again while it stays.
    let keys_before = context.host.inputs().len();
    passes(&context, 20, 1_000);
    assert_eq!(of_dialogs(&tiers.notes("chief")).len(), 1);
    assert_eq!(context.host.inputs().len(), keys_before);
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
