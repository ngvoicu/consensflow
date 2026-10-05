//! Switching the chief to another agent: the hand-over (the old window
//! closes, the new one opens with the handoff), what was on its way to the
//! old window, and a switch after the chief's turn.

use cf_engine::SwitchWhen;
use cf_harness::records::Role;

use crate::chiefs::{chief_of, to, with_codex};
use crate::fixtures::assert_match;
use crate::traces::held_to;

const SUITES: &[&str] = &["switching the chief to another agent"];

#[test]
fn hands_over_to_the_new_chief() {
    let context = with_codex();
    let project = context.with_staff(&["zeus"]);
    let chief = context.id(project.id, "chief");
    context.pass().unwrap();
    let said = context.adapter.item(Role::User, "The codeword is tern");
    context
        .adapter
        .with("chief", |agent| agent.items.push(said));
    context.adapter.answer("chief", "Noted: tern");
    context.adapter.busy("chief");
    context.pass().unwrap();
    let ready = context.note(project.id, "zeus", "chief", "Ready");

    context
        .switch_chief(project.id, to("codex", "astraeus"), SwitchWhen::Now, false)
        .unwrap();
    assert_eq!(
        context.project(project.id).state,
        "open",
        "a switch is not a close"
    );
    let killed: Vec<String> = context
        .host
        .killed()
        .into_iter()
        .map(|pane| pane.id)
        .collect();
    assert_eq!(killed, [format!("p{}-chief", project.id)]);
    let now = chief_of(&context, project.id);
    assert_eq!(
        (now.harness.as_deref(), now.agent.as_deref()),
        (Some("codex"), Some("astraeus"))
    );
    let prepared = context.codex.prepared().last().cloned().unwrap();
    assert_eq!(
        (
            &prepared["role"],
            &prepared["resume"],
            &prepared["agent"]["model"]
        ),
        (
            &serde_json::json!("chief"),
            &serde_json::Value::Null,
            &serde_json::json!("gpt-6-astra")
        ),
        "a fresh conversation, on the chosen model"
    );
    let message = prepared["message"].as_str().unwrap();
    assert_match(
        message,
        r"^\[ConsensFlow m-\d+ · note from ConsensFlow\]\nYou are the chief now\. The human switched this project's chief from Claude Code \(apollo\) to you, Codex \(astraeus\)\.",
    );
    assert_match(message, "cut off in the middle of a turn");
    assert_match(
        message,
        r#"The human's last message to the chief: "The codeword is tern""#,
    );
    assert_eq!(
        context.message(ready.id).state,
        "queued",
        "the handoff goes first"
    );

    // The new chief takes the handoff and answers it; then its queue goes on.
    context.pass().unwrap();
    assert_eq!(context.dispatcher.activity(chief).state.as_str(), "working");
    context
        .codex
        .answer("chief", "I have taken over; next is the parser.");
    context.pass().unwrap();
    let last = context.codex.agent("chief").items.last().cloned().unwrap();
    assert_match(&last.text, r"note from @zeus\]\nReady");
    let history: Vec<String> = context
        .ledger
        .borrow()
        .chief_history(project.id)
        .unwrap()
        .into_iter()
        .map(|conversation| conversation.conversation.harness)
        .collect();
    assert_eq!(
        history,
        ["claude-code"],
        "the old chief's words are history"
    );
    held_to(
        context.close(),
        SUITES,
        "hands over to the new chief: the old window closes, the project stays open, the new one opens with the handoff, and what was queued follows it",
    );
}

#[test]
fn gives_a_delivery_that_had_not_landed_back_to_the_queue_with_its_attempt_and_keeps_one_that_had()
{
    let context = with_codex();
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    let landed = context.note(project.id, "zeus", "chief", "One");
    context.pass().unwrap();
    assert_eq!(
        context.message(landed.id).state,
        "delivering",
        "not yet confirmed"
    );
    context
        .switch_chief(project.id, to("codex", "astraeus"), SwitchWhen::Now, false)
        .unwrap();
    assert_eq!(
        context.message(landed.id).state,
        "delivered",
        "the last look saw it arrive"
    );

    context.pass().unwrap();
    context.codex.answer("chief", "Taken over");
    context.pass().unwrap();
    context.codex.with("chief", |agent| agent.arrive = false);
    let lost = context.note(project.id, "zeus", "chief", "Two");
    context.pass().unwrap();
    assert_eq!(context.message(lost.id).state, "delivering");
    context
        .switch_chief(
            project.id,
            to("claude-code", "apollo"),
            SwitchWhen::Now,
            false,
        )
        .unwrap();
    let lost = context.message(lost.id);
    assert_eq!(
        (lost.state.as_str(), lost.attempts),
        ("queued", 0),
        "the window went before it could land: its attempt comes back"
    );
    held_to(
        context.close(),
        SUITES,
        "gives a delivery that had not landed back to the queue with its attempt, and keeps one that had",
    );
}

#[test]
fn keeps_the_chief_whose_window_would_not_close_for_a_switch_and_switches_after_its_turn() {
    let context = with_codex();
    let project = context.with_staff(&["zeus"]);
    let chief = context.id(project.id, "chief");
    context.pass().unwrap();
    context.host.refuse_kills.set(true);
    let refused = context
        .switch_chief(project.id, to("codex", "astraeus"), SwitchWhen::Now, false)
        .unwrap_err();
    assert_match(
        &refused.to_string(),
        "the chief's window would not close: the switch waits, and is tried again after its turn",
    );
    assert_eq!(
        chief_of(&context, project.id).harness.as_deref(),
        Some("claude-code")
    );
    assert_eq!(
        context.dispatcher.pending_switch(chief),
        Some(to("codex", "astraeus"))
    );
    context.host.refuse_kills.set(false);
    context.pass().unwrap();
    let now = chief_of(&context, project.id);
    assert_eq!(
        (now.harness.as_deref(), now.agent.as_deref()),
        (Some("codex"), Some("astraeus"))
    );
    held_to(
        context.close(),
        SUITES,
        "keeps the chief whose window would not close for a switch, and switches after its turn",
    );
}

#[test]
fn lets_a_chief_at_work_finish_its_turn_first_and_gives_it_nothing_else_meanwhile() {
    let context = with_codex();
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    context.adapter.busy("chief");
    let ready = context.note(project.id, "zeus", "chief", "Ready");
    context
        .switch_chief(project.id, to("codex", "astraeus"), SwitchWhen::Turn, false)
        .unwrap();
    assert_eq!(
        chief_of(&context, project.id).harness.as_deref(),
        Some("claude-code"),
        "not while it works"
    );
    assert!(context.host.killed().is_empty());

    context.adapter.answer("chief", "Done with that");
    context.pass().unwrap();
    assert_eq!(
        chief_of(&context, project.id).harness.as_deref(),
        Some("codex"),
        "its turn is over: it goes"
    );
    assert_eq!(
        context.message(ready.id).state,
        "queued",
        "Ready waited for the new chief"
    );
    assert!(!context
        .adapter
        .agent("chief")
        .items
        .iter()
        .any(|item| item.text.contains("Ready")));
    held_to(
        context.close(),
        SUITES,
        "lets a chief at work finish its turn first, and gives it nothing else meanwhile",
    );
}

#[test]
fn first_asks_the_chief_where_things_stand_when_the_human_wants_that_and_switches_once_it_has_answered(
) {
    let context = with_codex();
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    context
        .switch_chief(project.id, to("codex", "astraeus"), SwitchWhen::Now, true)
        .unwrap();
    context.pass().unwrap();
    let asked = context
        .adapter
        .agent("chief")
        .items
        .last()
        .cloned()
        .unwrap();
    assert_match(
        &asked.text,
        r"moving this project's chief to codex \(astraeus\) once you answer\. Write down where things stand",
    );
    context.pass().unwrap();
    assert_eq!(
        chief_of(&context, project.id).harness.as_deref(),
        Some("claude-code"),
        "it has not answered yet"
    );

    context
        .adapter
        .answer("chief", "Where things stand: the parser is half done.");
    context.pass().unwrap();
    assert_eq!(
        chief_of(&context, project.id).harness.as_deref(),
        Some("codex")
    );
    let history = context.ledger.borrow().chief_history(project.id).unwrap();
    let words: Vec<&str> = history
        .last()
        .unwrap()
        .items
        .iter()
        .map(|item| item.text.as_str())
        .collect();
    assert!(words.contains(&"Where things stand: the parser is half done."));
    held_to(
        context.close(),
        SUITES,
        "first asks the chief where things stand, when the human wants that, and switches once it has answered",
    );
}

#[test]
fn never_gives_the_new_chief_the_note_that_asked_the_old_one_where_things_stand() {
    let context = with_codex();
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    context.adapter.busy("chief");
    let asking = |n: usize| {
        let chief = chief_of(&context, project.id).id;
        let inbox = context.ledger.borrow().inbox(chief, 100).unwrap();
        inbox
            .into_iter()
            .filter(|message| message.body.contains("Write down where things stand"))
            .nth(n)
            .unwrap()
    };
    // Asked twice while the chief works: the second ask replaces the first.
    context
        .switch_chief(project.id, to("codex", "astraeus"), SwitchWhen::Turn, true)
        .unwrap();
    let first = asking(0);
    context
        .switch_chief(project.id, to("codex", "astraeus"), SwitchWhen::Turn, true)
        .unwrap();
    assert_eq!(
        context.message(first.id).state,
        "cancelled",
        "replaced, never sent"
    );
    context.pass().unwrap();
    // The human does not wait for the answer: Switch chief, now.
    context
        .switch_chief(project.id, to("codex", "astraeus"), SwitchWhen::Now, false)
        .unwrap();
    assert_eq!(
        chief_of(&context, project.id).harness.as_deref(),
        Some("codex")
    );
    assert_eq!(context.message(asking(0).id).state, "cancelled");
    context.pass().unwrap();
    context.codex.answer("chief", "I have taken over.");
    context.pass().unwrap();
    context.pass().unwrap();
    assert!(
        !context
            .codex
            .agent("chief")
            .items
            .iter()
            .any(|item| item.text.contains("Write down where things stand")),
        "the new chief never saw it"
    );
    held_to(
        context.close(),
        SUITES,
        "never gives the new chief the note that asked the old one where things stand",
    );
}

#[test]
fn switches_a_chief_out_of_quota_at_once_and_the_new_chief_is_not_out() {
    let context = with_codex();
    let project = context.with_staff(&["zeus"]);
    let chief = context.id(project.id, "chief");
    context.pass().unwrap();
    context
        .ledger
        .borrow_mut()
        .mark_out(chief, "2026-09-20T12:00:00.000Z", "quota")
        .unwrap();
    context.pass().unwrap();
    assert_eq!(context.dispatcher.activity(chief).state.as_str(), "out");
    context
        .switch_chief(project.id, to("codex", "astraeus"), SwitchWhen::Turn, false)
        .unwrap();
    let now = chief_of(&context, project.id);
    assert_eq!(
        now.harness.as_deref(),
        Some("codex"),
        "an out chief has no turn to finish"
    );
    assert_eq!(now.out_until, None);
    context.pass().unwrap();
    assert_ne!(context.dispatcher.activity(chief).state.as_str(), "out");
    held_to(
        context.close(),
        SUITES,
        "switches a chief out of quota at once, and the new chief is not out",
    );
}
