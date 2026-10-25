//! What a restart does with a message that was on its way into a window
//! (`describe('a restart while a message is on its way')`): given back to
//! the queue with its attempt, or settled by a record that shows it, and
//! never sent again that the harness took.

use std::rc::Rc;

use cf_engine::delivery_text::delivery_text;
use cf_engine::testing::Context;
use cf_harness::records::Role;
use cf_ledger::NewNote;
use serde_json::json;

use crate::fixtures::{assert_match, last_message};
use crate::traces::held_to;

const SUITES: &[&str] = &["a restart while a message is on its way"];

/// The words the window was given, after the header's line: what each
/// message of the chief's said.
fn given(context: &Context, handle: &str) -> Vec<String> {
    let items = context.adapter.agent(handle).items;
    items
        .iter()
        .filter(|item| item.role == Role::User)
        .map(|item| item.text.split('\n').nth(1).unwrap_or_default().to_owned())
        .collect()
}

#[test]
fn gives_a_message_its_window_never_showed_back_to_the_queue_with_its_attempt_and_the_chief_comes_back_to_it_and_to_what_follows(
) {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    context.adapter.with("chief", |agent| agent.arrive = false);
    let one = context.note(project.id, "zeus", "chief", "One");
    context.pass().unwrap();
    assert_eq!(context.message(one.id).state, "delivering");

    // The app quits with it on its way: the daemon stops before the window does.
    context.ledger.borrow_mut().suspend_for_restart().unwrap();
    let after = context.make();
    after.settle_after_restart().unwrap();
    let queued = context.message(one.id);
    assert_eq!(
        (queued.state.as_str(), queued.attempts),
        ("queued", 0),
        "its window died before it landed: the attempt comes back"
    );
    // The human resumes the project, and the chief comes back to it.
    after.resume_project(project.id).unwrap();
    let two = context.note(project.id, "zeus", "chief", "Two");
    after.pass().unwrap();
    after.pass().unwrap();
    context.adapter.answer("chief", "Read one.");
    after.pass().unwrap();
    after.pass().unwrap();
    assert_eq!(
        (context.message(one.id).state, context.message(two.id).state),
        ("delivered".to_owned(), "delivered".to_owned())
    );
    assert_eq!(given(&context, "chief"), ["One", "Two"]);
    held_to(
        context.close(),
        SUITES,
        "gives a message its window never showed back to the queue with its attempt, and the chief comes back to it and to what follows",
    );
}

/// Astra's review of 2026-10-17: the start that took a home alpha.82 left past
/// migration 0019 stopped after its commit, before the tasks of what it let go of were
/// reconciled. Node had left T-1 `working`, though the chief's answer to its worker's
/// question rode in words Node was pasting and was never received. The next start read
/// schema 19 and reconciled nothing, the words' marker was not in the record, and on
/// the human's Resume the task was finished from the worker's reply before the answer.
#[test]
fn a_task_whose_answer_a_stopped_start_let_go_of_is_not_finished_from_its_old_reply_on_resume() {
    let context = Context::new();
    // The worker's window comes back on its conversation, its record as it was.
    context.adapter.resumes_records.set(true);
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    context.give(project.id, "zeus", "Write the parser");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "working");
    // Its worker asks the chief, and its turn ends there; the chief answers.
    let question = context.ask(project.id, "zeus", "chief", 1, "Which format?");
    context
        .adapter
        .answer("zeus", "I asked the chief which format to use.");
    let answer = context.answer(project.id, question.id, "JSON");
    // Node was pasting words that carried the answer, T-1 `working` as its own confirm
    // left it; 0019 let go of the answer, and the start stopped there.
    context.edit_ledger(|file| {
        let db = rusqlite::Connection::open(file).unwrap();
        db.execute_batch(&format!(
            "INSERT INTO message (project_id, recipient_id, kind, task_id, body, state, attempts, created_at)
               SELECT project_id, recipient_id, 'task', task_id, 'Go on with T-1.', 'delivering', 1, created_at
                 FROM message WHERE id = {answer};
             INSERT INTO event (project_id, at, kind, data)
               SELECT project_id, created_at, 'delivery.begun',
                      json_object('message', (SELECT max(id) FROM message), 'attempt', 1)
                 FROM message WHERE id = {answer};
             INSERT INTO event (project_id, at, kind, data)
               SELECT project_id, created_at, 'message.uncarried',
                      json_object('carrier', (SELECT max(id) FROM message), 'released', json_array(id))
                 FROM message WHERE id = {answer};
             UPDATE task SET state = 'working' WHERE id = (SELECT task_id FROM message WHERE id = {answer});",
            answer = answer.id
        ))
        .unwrap();
    });
    assert_eq!(
        context.task(project.id, 1).task.state,
        "waiting",
        "the start after it reconciles T-1: its question has no answer received"
    );

    context.ledger.borrow_mut().suspend_for_restart().unwrap();
    let after = context.make();
    after.settle_after_restart().unwrap();
    after.resume_project(project.id).unwrap();
    for _ in 0..4 {
        after.pass().unwrap();
    }
    assert_eq!(context.message(answer.id).state, "delivered");
    assert_eq!(
        context.task(project.id, 1).task.state,
        "working",
        "not finished from the reply before its answer"
    );
    context.close();
}

/// Sol's review of 2026-10-18: 3.0.4 stopped with T-1 paused, the words that resume it
/// queued with no paste of them begun, and the chief's note that came after riding in
/// them. The upgrade runs migration 0019, which leaves the note in the words, for no
/// record holds a paste of them: on the human's Resume they go in with the note, and the
/// worker's answer after them is T-1's result. (Let go of, the note waited behind the
/// words, whose turn's answer finished T-1 first: "Output must be YAML" never reached
/// the worker.)
#[test]
fn a_note_that_rode_in_the_words_that_resume_a_task_goes_in_with_them_after_the_upgrade() {
    let context = Context::new();
    context.adapter.resumes_records.set(true);
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    context.give(project.id, "zeus", "Write the parser");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "working");
    // The chief pauses T-1 and resumes it, and its note rides in the words.
    context.pause_task(project.id, 1);
    context.resume_task(project.id, 1, "Carry on");
    let note = context
        .ledger
        .borrow_mut()
        .note(
            project.id,
            &NewNote {
                from: Some("chief".to_owned()),
                to: "zeus".to_owned(),
                body: "Output must be YAML".to_owned(),
                task: Some(1),
            },
        )
        .unwrap();
    let words = context
        .task(project.id, 1)
        .messages
        .into_iter()
        .find(|message| message.body == "Resumed: Carry on")
        .unwrap();
    // 3.0.4 stops there, and the upgrade runs migration 0019 on what it left (and
    // the viewer's 0020, the Resume's 0021 and 0022, and the exit proof's 0023).
    context.edit_ledger(|file| {
        let db = rusqlite::Connection::open(file).unwrap();
        cf_ledger::testing::undo_migration_0023(&db).unwrap();
        cf_ledger::testing::undo_migration_0022(&db).unwrap();
        cf_ledger::testing::undo_migration_0021(&db).unwrap();
        cf_ledger::testing::undo_migration_0020(&db).unwrap();
        db.execute_batch("PRAGMA user_version = 18").unwrap();
    });
    context.ledger.borrow_mut().suspend_for_restart().unwrap();
    let after = context.make();
    after.settle_after_restart().unwrap();
    after.resume_project(project.id).unwrap();
    for _ in 0..3 {
        after.pass().unwrap();
    }
    assert_eq!(
        (
            context.message(words.id).state,
            context.message(note.id).state
        ),
        ("delivered".to_owned(), "delivered".to_owned()),
        "the note went in with the words"
    );
    let pasted = context
        .adapter
        .agent("zeus")
        .items
        .iter()
        .rev()
        .find(|item| item.role == Role::User)
        .map(|item| item.text.to_string())
        .unwrap();
    assert!(pasted.contains("Output must be YAML"), "{pasted}");
    assert_eq!(
        pasted.matches("[ConsensFlow m-").count(),
        1,
        "one paste, under the words' marker: {pasted}"
    );
    context.adapter.answer("zeus", "The parser writes YAML.");
    after.pass().unwrap();
    after.pass().unwrap();
    let thread = context.task(project.id, 1);
    assert_eq!(thread.task.state, "done");
    let result = thread.messages.iter().find(|m| m.kind == "result").unwrap();
    assert_eq!(result.body, "The parser writes YAML.");
    context.close();
}

#[test]
fn sends_nothing_again_that_its_harness_took_though_the_copy_of_its_window_missed_it() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let chief = context.id(project.id, "chief");
    context.pass().unwrap();
    let note = context.note(project.id, "zeus", "chief", "Taken");
    context.pass().unwrap();
    // The harness took it, and its own record has it; the daemon stopped
    // before its next look copied the window, so the copy does not.
    assert_eq!(context.message(note.id).state, "delivering");
    let copied = context
        .ledger
        .borrow()
        .copied_item_with(chief, &format!("m-{}", note.id))
        .unwrap();
    assert_eq!(copied, None);
    let items = context.adapter.agent("chief").items;
    let taken = items
        .iter()
        .find(|item| item.role == Role::User && item.text.contains("Taken"))
        .unwrap();

    context.ledger.borrow_mut().suspend_for_restart().unwrap();
    let after = context.make();
    after.settle_after_restart().unwrap();
    let settled = context.message(note.id);
    assert_eq!(settled.state, "delivered");
    assert_eq!(settled.receipt, json!({ "item": &*taken.id }));
    after.resume_project(project.id).unwrap();
    after.pass().unwrap();
    after.pass().unwrap();
    let again = context
        .adapter
        .agent("chief")
        .items
        .iter()
        .filter(|item| item.role == Role::User && item.text.contains("Taken"))
        .count();
    assert_eq!(again, 0, "the window that came back is not given it again");
    held_to(
        context.close(),
        SUITES,
        "sends nothing again that its harness took, though the copy of its window missed it",
    );
}

#[test]
fn confirms_a_message_whose_header_the_copy_of_its_window_already_shows() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let chief = context.id(project.id, "chief");
    context.pass().unwrap();
    context.adapter.with("chief", |agent| agent.arrive = false);
    let note = context.note(project.id, "zeus", "chief", "Shown");
    context.pass().unwrap();
    // The window showed it and the copy has it; the daemon stopped before it confirmed it.
    let shown = context
        .adapter
        .item(Role::User, &delivery_text(&context.message(note.id), &[]));
    let conversation = context
        .ledger
        .borrow()
        .current_conversation(chief)
        .unwrap()
        .unwrap();
    let written = serde_json::to_value(&shown).unwrap();
    context
        .ledger
        .borrow_mut()
        .copy_transcript(conversation.id, &[written], 0)
        .unwrap();

    context.ledger.borrow_mut().suspend_for_restart().unwrap();
    context.make().settle_after_restart().unwrap();
    let settled = context.message(note.id);
    assert_eq!(settled.state, "delivered");
    assert_eq!(settled.receipt, json!({ "item": &*shown.id }));
    held_to(
        context.close(),
        SUITES,
        "confirms a message whose header the copy of its window already shows",
    );
}

#[test]
fn confirms_a_message_by_the_whole_id_the_harness_gave_the_item_that_shows_it_however_long() {
    // The reviewers' sequence: an item's id of 300 characters, which the copy
    // once cut where it kept it, so that the receipt named an id the harness
    // never gave. The copy keeps it whole, and the receipt names it.
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let chief = context.id(project.id, "chief");
    context.pass().unwrap();
    context.adapter.with("chief", |agent| agent.arrive = false);
    let note = context.note(project.id, "zeus", "chief", "Shown");
    context.pass().unwrap();
    let mut shown = context
        .adapter
        .item(Role::User, &delivery_text(&context.message(note.id), &[]));
    let long = "r".repeat(300);
    shown.id = std::sync::Arc::from(long.as_str());
    let conversation = context
        .ledger
        .borrow()
        .current_conversation(chief)
        .unwrap()
        .unwrap();
    let written = serde_json::to_value(&shown).unwrap();
    context
        .ledger
        .borrow_mut()
        .copy_transcript(conversation.id, &[written], 0)
        .unwrap();
    let marker = format!("m-{}", note.id);
    assert_eq!(
        context
            .ledger
            .borrow()
            .copied_item_with(chief, &marker)
            .unwrap(),
        Some(long.clone())
    );

    context.ledger.borrow_mut().suspend_for_restart().unwrap();
    context.make().settle_after_restart().unwrap();
    let settled = context.message(note.id);
    assert_eq!(settled.state, "delivered");
    assert_eq!(settled.receipt, json!({ "item": long }));
}

#[test]
fn opens_a_members_session_again_on_its_own_conversation_with_the_brief_it_was_opened_for() {
    let context = Context::new();
    let project = context.with_tiers(&["zeus", "diana"]);
    context.pool_task(project.id, "worker", "Write the parser");
    context.pass().unwrap();
    let session = context.id(project.id, "zeus-amber-pine");
    let native = context
        .ledger
        .borrow()
        .current_conversation(session)
        .unwrap()
        .unwrap()
        .native_session
        .unwrap();
    // The window opened, but its record never showed the brief before the stop.
    context.adapter.with("zeus", |agent| agent.items.clear());

    context.ledger.borrow_mut().suspend_for_restart().unwrap();
    let after = context.make();
    after.settle_after_restart().unwrap();
    after.resume_project(project.id).unwrap();
    after.pass().unwrap();
    let launch = context.adapter.prepared().pop().unwrap();
    assert_eq!(
        (
            launch["participant"]["handle"].clone(),
            launch["resume"].clone()
        ),
        (json!("zeus-amber-pine"), json!(native))
    );
    assert_match(
        launch["message"].as_str().unwrap(),
        r"^\[ConsensFlow m-\d+ · T-1 · task from @chief\]\nWrite the parser$",
    );
    after.pass().unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "working");
    held_to(
        context.close(),
        SUITES,
        "opens a member's session again on its own conversation with the brief it was opened for",
    );
}

#[test]
fn gives_a_paste_back_to_the_queue_when_its_window_closes_while_the_harness_takes_it() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    // The window closes after the harness has taken the paste, before it answers.
    let host = Rc::clone(&context.host);
    *context.adapter.deliver.borrow_mut() = Some(Rc::new(move |taking| {
        let host = Rc::clone(&host);
        Box::pin(async move {
            let outcome = taking();
            host.exit("chief").await;
            Ok(outcome)
        })
    }));
    let note = context.note(project.id, "zeus", "chief", "Lost");
    context.pass().unwrap();
    // The harness had taken the paste, and its record has it: the conversation
    // holds it, so it is confirmed, where Node gave it back to the queue (a
    // departure: `traces`' `DEPARTED`).
    assert_eq!(
        context.message(note.id).state,
        "delivered",
        "not left on its way, and not given back"
    );

    *context.adapter.deliver.borrow_mut() = None;
    context.resume_project(project.id).unwrap();
    // The chief's turn was cut with it: it is told so, and goes on from it.
    assert_match(
        &last_message(&context),
        r"^\[ConsensFlow m-\d+ · note from ConsensFlow\]\nConsensFlow stopped in the middle of your turn",
    );
    context.adapter.answer("chief", "Going on.");
    context.pass().unwrap();
    context.pass().unwrap();
    let again = context
        .adapter
        .agent("chief")
        .items
        .iter()
        .filter(|item| item.role == Role::User && item.text.contains("Lost"))
        .count();
    assert_eq!(again, 0, "the window that came back is not given it again");
    held_to(
        context.close(),
        SUITES,
        "gives a paste back to the queue when its window closes while the harness takes it",
    );
}
