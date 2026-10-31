//! What its recipient read whole with `cf inbox read`, or answered, is never
//! typed into its window after (T-260). Seen on 2026-10-22 on this board: a
//! member's question waited in the chief's queue while the chief was in a turn,
//! the chief read it with `cf inbox read` and answered it with `cf answer`, and
//! when its turn ended the question was typed in, "Run in your shell: cf answer
//! m-…", as if new (m-2927, m-2931, m-3103). The rules, as the chief decided:
//! - a queued message its recipient read with `cf inbox read` is `read`, and is
//!   not the head of its queue after: a question, a result or a note alike, but
//!   never a task's words or a note that says a conversation its role, which
//!   only their paste proves;
//! - a note that says a conversation its role and was delivered (its paste cut
//!   to its opening and `cf inbox read`, T-1021) and is then read in the inbox is
//!   `read`, and its role recorded as what the conversation was told, in one
//!   write: unless a later note says it a role, or its conversation began after
//!   it was delivered;
//! - a question already answered is never typed in, however it was read: still
//!   queued when it is answered, or put back in the queue by a paste that was not
//!   proved, it is `read`, its receipt naming the answer; one the asker's own
//!   window answered first is withdrawn, as one held at the gate is.

use cf_ledger::{MessageView, Read};
use serde_json::json;

use crate::fixture::{world, World};

/// A task given to zeus, its brief received, and the question zeus asks the
/// chief about it, which waits in the chief's queue: the task, the question.
fn asked(w: &mut World) -> (i64, MessageView) {
    let created = w.give("zeus", "Parser");
    let (task, brief) = (created.task.number, created.message.expect("a brief").id);
    w.deliver(brief);
    let question = w.ask("zeus", task, "Which grammar?");
    assert_eq!(w.states(&[question.id]), ["queued"]);
    assert_eq!(w.next("chief"), Some(question.id));
    (task, question)
}

/// The chief reads messages whole with `cf inbox read`.
fn read_in_the_inbox(w: &mut World, ids: &[i64]) {
    let chief = w.id("chief");
    w.ledger
        .receive_read(chief, ids, Read::Inbox)
        .expect("read in the inbox");
}

#[test]
fn a_question_the_chief_read_in_its_inbox_and_answered_is_never_the_head_of_its_queue() {
    let mut w = world();
    let (task, question) = asked(&mut w);
    read_in_the_inbox(&mut w, &[question.id]);
    assert_eq!(w.states(&[question.id]), ["read"]);
    assert_eq!(w.receipt(question.id), json!({ "read": "inbox" }));
    assert_eq!(
        w.next("chief"),
        None,
        "the chief has it: it is not typed in"
    );
    assert_eq!(w.state(task), "waiting", "a read is no answer");

    let answer = w.answer(question.id, "The new one");
    assert_eq!(
        w.states(&[question.id, answer.id]),
        ["read", "queued"],
        "the answer goes to zeus, and the question stays as it was read"
    );
    assert_eq!(w.receipt(question.id), json!({ "read": "inbox" }));
    assert_eq!(w.next("chief"), None);
    assert_eq!(w.next("zeus"), Some(answer.id));
}

#[test]
fn a_question_answered_as_it_waited_is_read_naming_its_answer_and_never_typed_in() {
    let mut w = world();
    let (_, question) = asked(&mut w);
    // The chief answers it with no `cf inbox read`: from the list, or the task's thread.
    let answer = w.answer(question.id, "The new one");
    assert_eq!(w.states(&[question.id]), ["read"]);
    assert_eq!(w.receipt(question.id), json!({ "answered": answer.id }));
    assert_eq!(w.next("chief"), None);
    assert!(
        w.ledger.pending(w.id("chief")).expect("a read").is_empty(),
        "nothing is on its way to the chief"
    );
}

#[test]
fn a_question_answered_while_its_paste_was_on_its_way_is_not_queued_again_when_it_is_retried() {
    let mut w = world();
    let (_, question) = asked(&mut w);
    w.begin(question.id);
    let answer = w.answer(question.id, "The new one");
    assert_eq!(
        w.states(&[question.id]),
        ["delivering"],
        "a paste on its way is the window's to settle"
    );
    w.ledger
        .retry_delivery(question.id, "the marker was not seen", false)
        .expect("retried");
    assert_eq!(w.states(&[question.id]), ["read"]);
    assert_eq!(w.receipt(question.id), json!({ "answered": answer.id }));
    assert_eq!(w.next("chief"), None);
}

#[test]
fn a_question_with_no_answer_retried_is_queued_again() {
    let mut w = world();
    let (_, question) = asked(&mut w);
    w.begin(question.id);
    w.ledger
        .retry_delivery(question.id, "the marker was not seen", false)
        .expect("retried");
    assert_eq!(w.states(&[question.id]), ["queued"]);
    assert_eq!(w.next("chief"), Some(question.id));
}

#[test]
fn a_question_the_askers_own_window_answered_first_is_withdrawn() {
    let mut w = world();
    let created = w.give("zeus", "Parser");
    let (task, brief) = (created.task.number, created.message.expect("a brief").id);
    w.deliver(brief);
    let question = w.ask_with_options("zeus", task);
    let zeus = w.id("zeus");
    let answer = w
        .ledger
        .answer(question.id, zeus, None, Some(&json!([["red"]])))
        .expect("the window's own answer");
    assert_eq!(w.states(&[question.id, answer.id]), ["cancelled", "read"]);
    assert_eq!(
        w.message(question.id).reason.as_deref(),
        Some("answered by @zeus")
    );
    assert_eq!(w.next("chief"), None);
}

#[test]
fn a_result_or_a_note_read_in_the_inbox_is_read_and_never_typed_in() {
    let mut w = world();
    let task = w.open("Parser").task.number;
    let moved = w.assign(task, "zeus");
    w.deliver(moved.message.expect("its brief").id);
    let result = w
        .ledger
        .record_result(w.project, task, "Parser, done")
        .expect("a result")
        .message
        .expect("its result");
    let note = w.note("zeus", "chief", task, "The tests are slow");
    assert_eq!(w.states(&[result.id, note.id]), ["queued", "queued"]);
    read_in_the_inbox(&mut w, &[result.id, note.id]);
    assert_eq!(w.states(&[result.id, note.id]), ["read", "read"]);
    assert_eq!(w.receipt(result.id), json!({ "read": "inbox" }));
    assert_eq!(w.next("chief"), None);
    assert_eq!(w.state(task), "done", "a read is no decision");
    // The decision then leaves the result as it was read: the chief had it.
    w.ledger
        .accept_task(w.project, task, "chief")
        .expect("accepted");
    assert_eq!(w.states(&[result.id]), ["read"]);
}

#[test]
fn a_tasks_words_or_a_role_read_in_the_inbox_wait_for_their_paste() {
    let mut w = world();
    let created = w.give("zeus", "Parser");
    let brief = created.message.expect("a brief").id;
    let zeus = w.id("zeus");
    w.ledger
        .receive_read(zeus, &[brief], Read::Inbox)
        .expect("read in the inbox");
    assert_eq!(w.states(&[brief]), ["queued"]);
    assert_eq!(w.next("zeus"), Some(brief));
    assert_eq!(w.state(created.task.number), "queued");

    let role = w
        .ledger
        .note_role(w.project, "chief", "You are the chief now.", "the role")
        .expect("a role");
    read_in_the_inbox(&mut w, &[role.id]);
    assert_eq!(w.states(&[role.id]), ["queued"]);
    assert_eq!(w.next("chief"), Some(role.id));
}

/// The chief's conversation, and a note that says it `role`, delivered: its paste
/// was cut, so it is not recorded as told.
fn role_delivered(w: &mut World, role: &str) -> (i64, MessageView) {
    let chief = w.id("chief");
    let conversation = match w.ledger.current_conversation(chief).expect("read") {
        Some(conversation) => conversation,
        None => w
            .ledger
            .start_conversation(chief, "codex")
            .expect("a conversation"),
    };
    let note = w
        .ledger
        .note_role(w.project, "chief", "Read it all with cf inbox read.", role)
        .expect("a role");
    w.deliver(note.id);
    (conversation.id, note)
}

fn told(w: &World, conversation: i64) -> Option<String> {
    w.ledger.conversation_role_text(conversation).expect("read")
}

#[test]
fn a_role_delivered_and_then_read_in_the_inbox_is_read_and_recorded_as_told_in_one_write() {
    let mut w = world();
    let (conversation, note) = role_delivered(&mut w, "the long role");
    assert_eq!(told(&w, conversation), None);
    let delivered = w.message(note.id);

    read_in_the_inbox(&mut w, &[note.id]);
    assert_eq!(w.states(&[note.id]), ["read"]);
    assert_eq!(told(&w, conversation).as_deref(), Some("the long role"));
    let read = w.message(note.id);
    assert_eq!(
        (read.delivered_at, read.receipt),
        (delivered.delivered_at, delivered.receipt),
        "when its paste went in, and the proof of it, are kept"
    );
    // A second read changes nothing.
    w.ledger
        .set_conversation_role_text(conversation, "since")
        .expect("written");
    read_in_the_inbox(&mut w, &[note.id]);
    assert_eq!(told(&w, conversation).as_deref(), Some("since"));
}

#[test]
fn a_role_read_in_the_inbox_is_recorded_only_from_its_recipient_and_only_once_delivered() {
    let mut w = world();
    let chief = w.id("chief");
    let conversation = w
        .ledger
        .start_conversation(chief, "codex")
        .expect("a conversation");
    let note = w
        .ledger
        .note_role(w.project, "chief", "Read it all.", "the role")
        .expect("a role");
    w.begin(note.id);
    read_in_the_inbox(&mut w, &[note.id]);
    assert_eq!(
        w.states(&[note.id]),
        ["delivering"],
        "its paste is still on its way"
    );
    w.confirm(note.id);
    let zeus = w.id("zeus");
    w.ledger
        .receive_read(zeus, &[note.id], Read::Inbox)
        .expect("read by another");
    w.ledger
        .receive_read(chief, &[note.id], Read::Task)
        .expect("read in a thread");
    assert_eq!(w.states(&[note.id]), ["delivered"]);
    assert_eq!(told(&w, conversation.id), None);
}

#[test]
fn a_role_read_in_the_inbox_after_a_later_one_was_written_is_read_and_records_nothing() {
    let mut w = world();
    let (conversation, first) = role_delivered(&mut w, "the first role");
    let (_, later) = role_delivered(&mut w, "the later role");
    read_in_the_inbox(&mut w, &[first.id]);
    assert_eq!(w.states(&[first.id]), ["read"]);
    assert_eq!(
        told(&w, conversation),
        None,
        "the later one is the role now"
    );
    read_in_the_inbox(&mut w, &[later.id]);
    assert_eq!(told(&w, conversation).as_deref(), Some("the later role"));
}

#[test]
fn a_role_read_in_the_inbox_by_a_conversation_that_began_after_it_was_delivered_records_nothing() {
    let mut w = world();
    let (conversation, note) = role_delivered(&mut w, "the role");
    w.ledger.end_conversation(conversation).expect("ended");
    let chief = w.id("chief");
    let now = w
        .ledger
        .start_conversation(chief, "codex")
        .expect("another conversation");
    read_in_the_inbox(&mut w, &[note.id]);
    assert_eq!(w.states(&[note.id]), ["read"]);
    assert_eq!(told(&w, now.id), None, "the note went into the other one");
}

#[test]
fn what_the_thread_of_cf_task_get_carried_receives_only_its_answers() {
    let mut w = world();
    let (_, question) = asked(&mut w);
    let chief = w.id("chief");
    w.ledger
        .receive_read(chief, &[question.id], Read::Task)
        .expect("read in a thread");
    assert_eq!(w.states(&[question.id]), ["queued"]);
    assert_eq!(w.next("chief"), Some(question.id));
}

#[test]
fn what_another_read_in_their_inbox_is_not_received() {
    let mut w = world();
    let (_, question) = asked(&mut w);
    let diana = w.id("diana");
    w.ledger
        .receive_read(diana, &[question.id], Read::Inbox)
        .expect("a read by another");
    assert_eq!(w.states(&[question.id]), ["queued"]);
}
