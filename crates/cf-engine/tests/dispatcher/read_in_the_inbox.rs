//! What the chief read whole with `cf inbox read` in a turn, or answered, is
//! never pasted into its window when that turn ends (T-260). Seen on
//! 2026-10-22 on this board: a member's question waited behind the chief's
//! turn, the chief read it with `cf inbox read m-…` and answered it, and when
//! the turn ended the question was typed in with "Run in your shell: cf answer
//! m-…" (m-2927, m-2931, m-3103). The ledger receives what was read, and
//! settles a question once it is answered, so the dispatcher, which asks the
//! ledger for the head of the chief's queue, has nothing of it to paste; what
//! the chief did not read is pasted as it always was. Node pasted them all, so
//! none of these is held to a Node trace.

use cf_engine::delivery_text::marker_of;
use cf_engine::testing::Context;
use cf_harness::records::Role;
use cf_ledger::{MessageView, Read};

/// T-1 given to zeus's window, which has taken it in: the project.
fn working(context: &Context) -> i64 {
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "working");
    project.id
}

/// What the window of `handle` was given as messages, oldest first.
fn given(context: &Context, handle: &str) -> Vec<String> {
    context
        .adapter
        .agent(handle)
        .items
        .iter()
        .filter(|item| item.role == Role::User)
        .map(|item| item.text.to_string())
        .collect()
}

/// How many of the messages `handle`'s window was given are message `id`.
fn pasted(context: &Context, handle: &str, id: i64) -> usize {
    given(context, handle)
        .iter()
        .filter(|text| text.contains(&marker_of(id)))
        .count()
}

/// The chief is in a turn of its own when zeus asks it about T-1: the question
/// waits behind that turn, and zeus's turn ends on it.
fn asked_in_the_chiefs_turn(context: &Context, project: i64) -> MessageView {
    context.adapter.busy("chief");
    context.pass().unwrap();
    let question = context.ask(project, "zeus", "chief", 1, "Which grammar?");
    // zeus ends its turn on the question, as the worker role tells it to.
    context.adapter.answer("zeus", "I asked which grammar");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(
        context.message(question.id).state,
        "queued",
        "a busy chief is not interrupted"
    );
    question
}

/// The chief reads messages whole in its shell: `cf inbox read m-N`.
fn read_in_the_inbox(context: &Context, project: i64, id: i64) {
    let chief = context.id(project, "chief");
    context
        .ledger
        .borrow_mut()
        .receive_read(chief, &[id], Read::Inbox)
        .unwrap();
}

/// The chief's turn ends, and the dispatcher looks at its window.
fn the_chiefs_turn_ends(context: &Context) {
    context.adapter.with("chief", |agent| agent.settled = true);
    context.pass().unwrap();
    context.pass().unwrap();
}

#[test]
fn pastes_no_question_the_chief_read_in_its_inbox_and_answered_in_the_turn_it_waited_behind() {
    let context = Context::new();
    let project = working(&context);
    let question = asked_in_the_chiefs_turn(&context, project);
    read_in_the_inbox(&context, project, question.id);
    let answer = context.answer(project, question.id, "The new one");
    context.pass().unwrap();
    assert_eq!(
        pasted(&context, "zeus", answer.id),
        1,
        "zeus has its answer"
    );

    the_chiefs_turn_ends(&context);
    assert_eq!(pasted(&context, "chief", question.id), 0);
    assert_eq!(context.message(question.id).state, "read");
}

#[test]
fn pastes_no_question_the_chief_answered_in_the_turn_it_waited_behind_however_it_read_it() {
    let context = Context::new();
    let project = working(&context);
    let question = asked_in_the_chiefs_turn(&context, project);
    // Read in the list, or in the task's thread: answered all the same.
    let answer = context.answer(project, question.id, "The new one");
    the_chiefs_turn_ends(&context);
    assert_eq!(pasted(&context, "chief", question.id), 0);
    assert_eq!(context.message(question.id).state, "read");
    assert_eq!(pasted(&context, "zeus", answer.id), 1);
}

#[test]
fn pastes_no_result_the_chief_read_in_its_inbox_and_pastes_what_it_did_not_read() {
    let context = Context::new();
    let project = working(&context);
    context.adapter.busy("chief");
    context.adapter.answer("zeus", "Parser done");
    context.pass().unwrap();
    context.pass().unwrap();
    let result = context
        .inbox(context.id(project, "chief"))
        .into_iter()
        .find(|message| message.kind == "result")
        .expect("a result for the chief");
    assert_eq!(result.state, "queued", "a busy chief is not interrupted");
    let note = context.note(project, "zeus", "chief", "The tests are slow");
    read_in_the_inbox(&context, project, result.id);

    the_chiefs_turn_ends(&context);
    assert_eq!(pasted(&context, "chief", result.id), 0);
    assert_eq!(context.message(result.id).state, "read");
    assert_eq!(
        context.task(project, 1).task.state,
        "done",
        "a read is no decision"
    );
    assert_eq!(
        pasted(&context, "chief", note.id),
        1,
        "what it did not read is pasted"
    );
}
