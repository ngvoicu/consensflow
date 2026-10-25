//! A tell's turn that goes on to its complete answer while its task is paused (T-442;
//! T-429 on 3.0.5, T-534's live runs on 3.0.6-alpha.2). `cf tell` pauses the task and puts
//! the chief's words to its window as a question; the window answered with `cf answer`,
//! went straight on in the same turn, and ended it with its complete answer, and nobody
//! had those words: the task was paused, so no result was taken, and no message carried
//! them. The resume then began a turn whose short follow-up became the result.
//!
//! The rule: that answer is put to whoever gave the task, once, in a note, and the task
//! stays paused; Finish takes the words as the result, a resume sends the window on.
//! Node had none of it, so none of these is held to a Node trace.

use cf_engine::testing::Context;
use cf_harness::records::Role;
use cf_ledger::NewTask;
use cf_proto::ledger::MessageView;
use serde_json::json;

use crate::fixtures::notes;

/// What the window ends its turn with, after the answer to the tell.
const FINAL: &str = "FINAL-RESULT: the parser is done on grammar v2";

/// What the window answers the tell with.
const ANSWER: &str = "TOLD-ANSWER: on the v2 grammar";

/// T-1 given to zeus and at work; the chief tells it, and the window, stopped, takes the
/// words in as a question and begins its turn on them. The project and the tell.
fn told_at_work(context: &Context) -> (i64, MessageView) {
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "working");
    context.adapter.busy("zeus");
    let told = context.tell(
        project.id,
        "zeus",
        1,
        "Stop, answer, and carry on to the end",
    );
    context.pass().unwrap();
    // The turn the stop cut ends; the question goes in once the window is at rest.
    context.adapter.answer("zeus", "Half a parser");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.message(told.id).state, "delivered");
    assert_eq!(context.task(project.id, 1).task.state, "paused");
    context.adapter.busy("zeus");
    (project.id, told)
}

/// The window answers the tell with `cf answer`, works on, and ends its turn with `FINAL`:
/// the record shows its words, a tool, the answer's tool, and the final words.
fn answers_then_goes_on_to_the_end(context: &Context, told: &MessageView) {
    let item = |role, text: &str, complete| {
        let mut item = context.adapter.item(role, text);
        item.complete = complete;
        item
    };
    context
        .ledger
        .borrow_mut()
        .answer(told.id, told.recipient_id, Some(&json!(ANSWER)), None)
        .unwrap();
    context.adapter.with("zeus", |agent| {
        agent
            .items
            .push(item(Role::Assistant, "Answering, then on", false));
        agent
            .items
            .push(item(Role::Tool, "m-3 answered @chief.", true));
        agent.items.push(item(Role::Tool, "(Bash completed)", true));
    });
    context.adapter.answer("zeus", FINAL);
}

/// The notes the chief was sent, oldest first, that are about T-1's paused turn.
fn kept_notes(context: &Context, project: i64) -> Vec<String> {
    notes(context, context.id(project, "chief"))
        .into_iter()
        .filter(|note| note.starts_with("T-1 is paused, and @"))
        .collect()
}

/// The results T-1 has, oldest first.
fn results(context: &Context, project: i64) -> Vec<String> {
    context
        .task(project, 1)
        .messages
        .into_iter()
        .filter(|message| message.kind == "result")
        .map(|message| message.body)
        .collect()
}

/// The note of T-1's paused turn, in the chief's inbox, as it is now.
fn kept_note(context: &Context, project: i64) -> MessageView {
    context
        .inbox(context.id(project, "chief"))
        .into_iter()
        .find(|message| message.body.starts_with("T-1 is paused, and @"))
        .expect("the note")
}

#[test]
fn the_final_answer_a_window_ends_the_turn_of_a_tell_with_is_put_to_the_chief_once_and_the_task_stays_paused(
) {
    let context = Context::new();
    let (project, told) = told_at_work(&context);
    assert_eq!(
        kept_notes(&context, project),
        Vec::<String>::new(),
        "the turn the stop cut said nothing to keep"
    );
    answers_then_goes_on_to_the_end(&context, &told);
    for _ in 0..4 {
        context.pass().unwrap();
    }

    let thread = context.task(project, 1);
    assert_eq!(
        thread.task.state, "paused",
        "no result is taken for the chief"
    );
    assert_eq!(results(&context, project), Vec::<String>::new());
    assert_eq!(
        kept_notes(&context, project),
        [format!(
            "T-1 is paused, and @zeus's turn on the words put to its window (m-{}) is over. What it said last:\n\n{FINAL}\n\nIf that is the result, take it: cf task finish T-1. To send it on: cf task resume T-1 \"…\".",
            told.id
        )],
        "told once, however many passes look at the window"
    );
    // The answer to the tell reached the chief as its own message.
    assert!(thread
        .messages
        .iter()
        .any(|message| message.kind == "answer" && message.body == ANSWER));
}

#[test]
fn finish_takes_the_final_answer_of_a_paused_task_as_its_result() {
    let context = Context::new();
    let (project, told) = told_at_work(&context);
    answers_then_goes_on_to_the_end(&context, &told);
    context.pass().unwrap();
    assert_eq!(kept_notes(&context, project).len(), 1);

    let finished = context.finish_task(project, 1, "chief").expect("finished");
    assert_eq!(finished.task.state, "done");
    assert_eq!(
        results(&context, project),
        [format!(
            "{FINAL}\n\n(ConsensFlow: Finished at @chief's word: what @zeus said last is the result.)"
        )],
        "the answer after the tell, not a follow-up"
    );
    // A pass that follows takes no other result.
    context.pass().unwrap();
    assert_eq!(context.task(project, 1).task.state, "done");
    assert_eq!(results(&context, project).len(), 1);
}

#[test]
fn a_resume_withdraws_the_note_the_chief_has_not_read_and_the_resumed_turn_is_the_result() {
    let context = Context::new();
    let (project, told) = told_at_work(&context);
    // The chief is at work on a turn of its own: the note waits in its queue.
    context.adapter.busy("chief");
    answers_then_goes_on_to_the_end(&context, &told);
    context.pass().unwrap();
    let note = kept_note(&context, project);
    assert_eq!(note.state, "queued");

    context.resume_task(project, 1, "Go on, or say you finished");
    let withdrawn = context.message(note.id);
    assert_eq!(withdrawn.state, "cancelled");
    assert_eq!(withdrawn.reason.as_deref(), Some("T-1 resumed"));
    context.pass().unwrap();
    context.adapter.answer("zeus", "ALREADY-FINISHED");
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(context.task(project, 1).task.state, "done");
    assert_eq!(results(&context, project), ["ALREADY-FINISHED"]);
}

#[test]
fn finishing_a_paused_task_withdraws_the_note_the_chief_has_not_read() {
    let context = Context::new();
    let (project, told) = told_at_work(&context);
    context.adapter.busy("chief");
    answers_then_goes_on_to_the_end(&context, &told);
    context.pass().unwrap();
    let note = kept_note(&context, project);
    assert_eq!(note.state, "queued");

    context.finish_task(project, 1, "chief").expect("finished");
    assert_eq!(context.message(note.id).state, "cancelled");
}

#[test]
fn a_window_that_answers_the_tell_in_plain_text_has_its_words_put_to_the_chief_too() {
    // OpenCode (T-534): no `cf answer`, the answer and the final words in one reply.
    let context = Context::new();
    let (project, _) = told_at_work(&context);
    context
        .adapter
        .answer("zeus", &format!("{ANSWER}\n{FINAL}"));
    context.pass().unwrap();
    context.pass().unwrap();

    assert_eq!(context.task(project, 1).task.state, "paused");
    let kept = kept_notes(&context, project);
    assert_eq!(kept.len(), 1, "{kept:?}");
    assert!(kept[0].contains(&format!("{ANSWER}\n{FINAL}")), "{kept:?}");
}

#[test]
fn a_tell_answered_with_cf_answer_alone_says_nothing_more() {
    let context = Context::new();
    let (project, told) = told_at_work(&context);
    context
        .ledger
        .borrow_mut()
        .answer(told.id, told.recipient_id, Some(&json!(ANSWER)), None)
        .unwrap();
    let mut done = context.adapter.item(Role::Tool, "m-3 answered @chief.");
    done.complete = true;
    context.adapter.with("zeus", |agent| {
        agent.items.push(done);
        agent.settled = true;
    });
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(kept_notes(&context, project), Vec::<String>::new());
    assert_eq!(context.task(project, 1).task.state, "paused");
}

#[test]
fn a_turn_that_ends_with_work_its_harness_wakes_it_for_is_not_over_and_says_nothing_yet() {
    let context = Context::new();
    let (project, told) = told_at_work(&context);
    answers_then_goes_on_to_the_end(&context, &told);
    context.adapter.answer_leaving_a_job("zeus", FINAL);
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(kept_notes(&context, project), Vec::<String>::new());
}

#[test]
fn finish_refuses_a_paused_task_whose_window_has_said_nothing_since_the_tell() {
    let context = Context::new();
    let (project, _) = told_at_work(&context);
    let refused = context.finish_task(project, 1, "chief").unwrap_err();
    assert!(
        refused
            .to_string()
            .starts_with("cannot finish T-1: @zeus has written no complete answer"),
        "{refused}"
    );
    assert_eq!(context.task(project, 1).task.state, "paused");
}

#[test]
fn a_worker_cannot_record_the_result_of_a_paused_task_itself() {
    // Only the chief's or the human's word finishes a paused task: the window's own
    // `cf task done` stays refused, as it was.
    let context = Context::new();
    let (project, _) = told_at_work(&context);
    let refused = context
        .ledger
        .borrow_mut()
        .record_result(project, 1, FINAL)
        .unwrap_err();
    assert_eq!(
        refused.to_string(),
        "cannot record a result for T-1: it is paused"
    );
}

#[test]
fn a_pause_after_a_resume_with_no_tell_since_tells_nothing_of_a_turn_that_ended_before_the_stop() {
    // The latest message of the window is the resume's, not a tell: the answer after it
    // is no answer to the chief's words, and a harness may write as complete what a stop
    // cut.
    let context = Context::new();
    let (project, told) = told_at_work(&context);
    answers_then_goes_on_to_the_end(&context, &told);
    context.pass().unwrap();
    context.resume_task(project, 1, "Go on");
    context.pass().unwrap();
    context.pass().unwrap();
    context.adapter.busy("zeus");
    context.pause_task(project, 1);
    context.adapter.answer("zeus", "Cut short");
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(context.task(project, 1).task.state, "paused");
    assert_eq!(
        kept_notes(&context, project).len(),
        1,
        "only the first tell's answer was told"
    );
}

#[test]
fn a_task_the_human_gave_is_put_to_the_human_in_the_words_of_the_drawer() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.create_task(
        project.id,
        NewTask {
            from: "human".to_owned(),
            to: Some("zeus".to_owned()),
            body: "Parser".to_owned(),
            ..NewTask::default()
        },
    );
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "working");
    context.adapter.busy("zeus");
    let told = context.tell(project.id, "zeus", 1, "Stop, answer, and carry on");
    context.pass().unwrap();
    context.adapter.answer("zeus", "Half a parser");
    context.pass().unwrap();
    context.pass().unwrap();
    context.adapter.busy("zeus");
    answers_then_goes_on_to_the_end(&context, &told);
    context.pass().unwrap();

    assert_eq!(
        notes(&context, context.id(project.id, "human")),
        [format!(
            "T-1 is paused, and @zeus's turn on the words put to its window (m-{}) is over. What it said last:\n\n{FINAL}\n\nTo send it on, Resume it in the task's drawer.",
            told.id
        )]
    );
}

#[test]
fn a_turn_whose_last_words_are_progress_and_no_complete_answer_says_nothing() {
    let context = Context::new();
    let (project, told) = told_at_work(&context);
    context
        .ledger
        .borrow_mut()
        .answer(told.id, told.recipient_id, Some(&json!(ANSWER)), None)
        .unwrap();
    let mut progress = context
        .adapter
        .item(Role::Assistant, "Still reading the parser");
    progress.complete = false;
    context.adapter.with("zeus", |agent| {
        agent.items.push(progress);
        agent.settled = true;
    });
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(kept_notes(&context, project), Vec::<String>::new());
}

#[test]
fn a_complete_answer_of_no_words_says_nothing_either() {
    let context = Context::new();
    let (project, _) = told_at_work(&context);
    context.adapter.answer("zeus", "  ");
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(kept_notes(&context, project), Vec::<String>::new());
}
