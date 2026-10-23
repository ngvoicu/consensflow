//! What tells a replacement of a role from any other message: the ledger knows
//! it by the role text it was written to say, and no words say it. A worker's
//! turn that answers a replacement is no answer to its task; a turn that answers
//! a note that only quotes the replacement's words is.

use cf_engine::testing::{Context, REPLACEMENT_OPENING};
use cf_harness::records::Role;
use cf_ledger::NewNote;

#[test]
fn a_note_about_the_task_that_quotes_the_opening_of_a_replacement_does_not_suppress_the_workers_answer(
) {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Write the parser");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "working");
    // Its turn settles with nothing written: the window is at rest, and the task
    // is open (a turn that answered would be its result at once, and no note
    // would find the task working).
    context.adapter.with("zeus", |agent| agent.settled = true);
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "working");

    // The chief asks the worker, mid-task, to review the wording of a note, and
    // quotes the opening of a replacement of a role for it, word for word.
    let note = context
        .ledger
        .borrow_mut()
        .note(
            project.id,
            &NewNote {
                from: Some("chief".to_owned()),
                to: "zeus".to_owned(),
                task: Some(1),
                body: format!("Is this wording clear enough?\n\n{REPLACEMENT_OPENING}"),
            },
        )
        .unwrap();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.message(note.id).state, "delivered");
    let said = context.adapter.agent("zeus").items;
    assert!(
        said.iter()
            .any(|item| item.text.contains(REPLACEMENT_OPENING)),
        "the worker was given the quoted words"
    );

    // Its answer after them is its task's result.
    context.adapter.answer("zeus", "Parser done");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(
        context.task(project.id, 1).task.state,
        "done",
        "the words are not a role: the worker's answer is the result"
    );
    let result = context
        .inbox(context.id(project.id, "chief"))
        .into_iter()
        .find(|message| message.kind == "result")
        .expect("the result");
    assert!(result.body.contains("Parser done"), "{}", result.body);
    context.close();
}

/// The same words typed into the worker's window by the human, mid-task: no
/// message of the board, so the answer to the task is read from the turn that holds
/// them, where only a header could say they are a replacement, and none does. A note
/// about the task is told words (3.0.6, B3), the last thing pasted, after which the
/// answer is read, so the case above no longer meets that reading (the merge of
/// main, T-340).
#[test]
fn words_the_human_types_that_quote_the_opening_of_a_replacement_do_not_suppress_the_workers_answer(
) {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Write the parser");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "working");

    let typed = context.adapter.item(
        Role::User,
        &format!("Is this wording clear enough?\n\n{REPLACEMENT_OPENING}"),
    );
    context
        .adapter
        .with("zeus", |agent| agent.items.push(typed));
    context.adapter.answer("zeus", "Parser done");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(
        context.task(project.id, 1).task.state,
        "done",
        "the words are not a role: the worker's answer is the result"
    );
    let result = context
        .inbox(context.id(project.id, "chief"))
        .into_iter()
        .find(|message| message.kind == "result")
        .expect("the result");
    assert!(result.body.contains("Parser done"), "{}", result.body);
    context.close();
}
