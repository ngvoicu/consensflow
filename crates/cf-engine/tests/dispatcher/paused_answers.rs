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
use cf_harness::records::{End, Role, Source};
use cf_ledger::NewTask;
use cf_proto::ledger::MessageView;
use serde_json::json;

use crate::fixtures::{given, notes, Tiers};
use crate::turn_end::{monitor, pending};

/// What the window ends its turn with, after the answer to the tell.
pub(crate) const FINAL: &str = "FINAL-RESULT: the parser is done on grammar v2";

/// What the window answers the tell with.
pub(crate) const ANSWER: &str = "TOLD-ANSWER: on the v2 grammar";

/// T-1 given to zeus and at work; the chief tells it, and the window, stopped, takes the
/// words in as a question and begins its turn on them. The project and the tell.
pub(crate) fn told_at_work(context: &Context) -> (i64, MessageView) {
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "working");
    (project.id, tell_working(context, project.id, "zeus"))
}

/// The chief tells the window of T-1, at work, `Stop, answer, and carry on to the end`: the
/// turn the stop cut ends, the question goes in once the window is at rest, and the window
/// begins its turn on it. The tell.
fn tell_working(context: &Context, project: i64, window: &str) -> MessageView {
    context.adapter.busy("zeus");
    let told = context.tell(project, window, 1, "Stop, answer, and carry on to the end");
    context.pass().unwrap();
    context.adapter.answer("zeus", "Half a parser");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.message(told.id).state, "delivered");
    assert_eq!(context.task(project, 1).task.state, "paused");
    context.adapter.busy("zeus");
    told
}

/// The window answers the tell with `cf answer` and works on: the record shows a call that
/// says nothing, a tool, the answer's tool, and another tool, and its turn goes on. The call
/// says no words, so the note of these tests is of the turn's end alone; what a window writes
/// first, before its first tool, has its tests at the end of this file (T-1151).
pub(crate) fn answers_and_goes_on(context: &Context, told: &MessageView) {
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
        agent.items.push(item(Role::Assistant, "", false));
        agent
            .items
            .push(item(Role::Tool, "m-3 answered @chief.", true));
        agent.items.push(item(Role::Tool, "(Bash completed)", true));
    });
}

/// The window answers the tell with `cf answer`, works on, and ends its turn with `FINAL`.
fn answers_then_goes_on_to_the_end(context: &Context, told: &MessageView) {
    answers_and_goes_on(context, told);
    context.adapter.answer("zeus", FINAL);
}

/// The notes the chief was sent, oldest first, that are about T-1's paused turn.
pub(crate) fn kept_notes(context: &Context, project: i64) -> Vec<String> {
    notes(context, context.id(project, "chief"))
        .into_iter()
        .filter(|note| {
            note.starts_with("T-1 is paused, and @") || note.starts_with("T-1, paused when @")
        })
        .collect()
}

/// The results T-1 has, oldest first.
pub(crate) fn results(context: &Context, project: i64) -> Vec<String> {
    context
        .task(project, 1)
        .messages
        .into_iter()
        .filter(|message| message.kind == "result")
        .map(|message| message.body)
        .collect()
}

/// The chief ends the turn it is in, and the dispatcher gives it what waits: the answer to
/// the tell first, then the note, a message at a turn.
fn chief_ends_its_turn(context: &Context) {
    context.adapter.answer("chief", "Read.");
    context.pass().unwrap();
    context.pass().unwrap();
}

/// The note of T-1's paused turn, in the chief's inbox, as it is now.
fn kept_note(context: &Context, project: i64) -> MessageView {
    context
        .inbox(context.id(project, "chief"))
        .into_iter()
        .find(|message| {
            message.body.starts_with("T-1 is paused, and @")
                || message.body.starts_with("T-1, paused when @")
        })
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

/// The note T-1's paused turn left, as the words of a task that has moved on say it.
pub(crate) fn moved_on_words(handle: &str, told: &MessageView) -> String {
    format!(
        "T-1, paused when @{handle}'s turn on the words put to its window (m-{}) began, has moved on since (resumed, or back on the board). This is not its result: that comes as a message of its own, the answer of the turn its resume, or its next assignment, begins, and it is the one to decide. What the turn on those words ended with:\n\n{FINAL}",
        told.id
    )
}

#[test]
fn a_resume_before_the_chief_read_the_note_keeps_it_in_words_true_of_the_resumed_task_and_the_resumed_turn_is_the_result(
) {
    let context = Context::new();
    let (project, told) = told_at_work(&context);
    // The chief is at work on a turn of its own: the note waits in its queue.
    context.adapter.busy("chief");
    answers_then_goes_on_to_the_end(&context, &told);
    context.pass().unwrap();
    let note = kept_note(&context, project);
    assert_eq!(note.state, "queued");

    // The chief resumes the task, as the answer it was given told it to, before the note.
    context.resume_task(project, 1, "Go on, or say you finished");
    let kept = context.message(note.id);
    assert_eq!(kept.state, "queued", "the resume does not take it back");
    assert_eq!(kept.body, moved_on_words("zeus", &told));
    context.pass().unwrap();
    context.adapter.answer("zeus", "ALREADY-FINISHED");
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(context.task(project, 1).task.state, "done");
    assert_eq!(results(&context, project), ["ALREADY-FINISHED"]);

    // The chief's turn ends, and it is given the note: the words the window wrote before.
    for _ in 0..3 {
        chief_ends_its_turn(&context);
    }
    assert_eq!(context.message(note.id).state, "delivered");
    let pasted: Vec<String> = given(&context, "chief")
        .into_iter()
        .filter(|text| text.contains(&moved_on_words("zeus", &told)))
        .collect();
    assert_eq!(pasted.len(), 1, "given once: {pasted:?}");
}

/// T-958 (Gabriel's other Mac, T-68): the chief resumed while the window was still on its
/// turn on the tell, which ended with the full report; the note gave it, and the resumed
/// turn wrote the report again, in other words, as the result. Each says which to decide:
/// the note that it is not the result, and the result's paste that it is, naming the note.
#[test]
fn the_result_after_a_note_of_the_turn_on_a_tell_says_it_is_the_one_to_decide() {
    let context = Context::new();
    let (project, told) = told_at_work(&context);
    context.adapter.busy("chief");
    answers_then_goes_on_to_the_end(&context, &told);
    context.pass().unwrap();
    let note = kept_note(&context, project);
    context.resume_task(project, 1, "Carry on with the brief");
    assert!(
        context.message(note.id).body.contains(
            "This is not its result: that comes as a message of its own, the answer of the turn its resume, or its next assignment, begins, and it is the one to decide."
        ),
        "{}",
        context.message(note.id).body
    );
    context.pass().unwrap();
    context
        .adapter
        .answer("zeus", "The parser is done, said again in other words");
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(context.task(project, 1).task.state, "done");
    for _ in 0..4 {
        chief_ends_its_turn(&context);
    }
    let pasted = given(&context, "chief");
    let result = pasted
        .iter()
        .find(|text| text.contains(" · result from "))
        .unwrap_or_else(|| panic!("the result is given: {pasted:?}"));
    assert!(
        result.contains(&format!(
            "The turn on your words while T-1 was paused ended with a report too, given in m-{}: this result is the one to decide. Decide with: cf task accept T-1",
            note.id
        )),
        "{result}"
    );
}

/// Calliope's T-987: a note names the result that came after it, and no result after
/// that. The note of T-1's turn on the tell, its result naming it, and the chief's reopen
/// of the done task (`cf task reopen`, as the footer says; an accepted task is reopened by
/// no one): the reopen's result says only how to decide.
#[test]
fn a_result_after_a_reopen_names_no_note_of_an_earlier_round() {
    let context = Context::new();
    let (project, told) = told_at_work(&context);
    context.adapter.busy("chief");
    answers_then_goes_on_to_the_end(&context, &told);
    context.pass().unwrap();
    let note = kept_note(&context, project);
    context.resume_task(project, 1, "Carry on with the brief");
    context.pass().unwrap();
    context.adapter.answer("zeus", "The parser is done.");
    for _ in 0..3 {
        context.pass().unwrap();
    }
    for _ in 0..4 {
        chief_ends_its_turn(&context);
    }
    let first = given(&context, "chief");
    assert!(
        first
            .iter()
            .any(|text| text.contains(&format!("given in m-{}", note.id))),
        "the first result names the note: {first:?}"
    );
    context
        .ledger
        .borrow_mut()
        .reopen_task(project, 1, "chief", "Fix the typo in its name")
        .unwrap();
    for _ in 0..2 {
        context.pass().unwrap();
    }
    context.adapter.answer("zeus", "The typo is fixed.");
    for _ in 0..3 {
        context.pass().unwrap();
    }
    for _ in 0..4 {
        chief_ends_its_turn(&context);
    }
    let pasted = given(&context, "chief");
    let last = pasted
        .iter()
        .rfind(|text| text.contains(" · result from "))
        .unwrap_or_else(|| panic!("the results are given: {pasted:?}"));
    assert!(last.contains("The typo is fixed."), "{last}");
    assert!(
        last.ends_with("\n\nDecide with: cf task accept T-1 · cf task reopen T-1 \"…\""),
        "{last}"
    );
}

/// A result whose task's window was told nothing while paused says nothing of a note.
#[test]
fn a_result_with_no_note_of_a_turn_on_a_tell_says_only_how_to_decide() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    context.adapter.answer("zeus", "Done.");
    for _ in 0..4 {
        context.pass().unwrap();
    }
    let pasted = given(&context, "chief");
    let result = pasted
        .iter()
        .find(|text| text.contains(" · result from "))
        .unwrap_or_else(|| panic!("the result is given: {pasted:?}"));
    assert!(
        result.ends_with("\n\nDecide with: cf task accept T-1 · cf task reopen T-1 \"…\""),
        "{result}"
    );
}

#[test]
fn a_resume_after_the_chief_read_the_note_leaves_it_as_it_was_read_and_the_resumed_turn_is_the_result(
) {
    let context = Context::new();
    let (project, told) = told_at_work(&context);
    answers_then_goes_on_to_the_end(&context, &told);
    context.pass().unwrap();
    // The chief ends its turn on the answer, and is given the note, and reads it.
    for _ in 0..2 {
        chief_ends_its_turn(&context);
    }
    let note = kept_note(&context, project);
    assert_eq!(note.state, "delivered", "the chief has it");
    let paused_words = note.body.clone();
    assert!(paused_words.starts_with("T-1 is paused, and @zeus's turn"));

    context.resume_task(project, 1, "Go on, or say you finished");
    assert_eq!(
        context.message(note.id).body,
        paused_words,
        "what was read stays"
    );
    context.pass().unwrap();
    context.adapter.answer("zeus", "ALREADY-FINISHED");
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(context.task(project, 1).task.state, "done");
    assert_eq!(results(&context, project), ["ALREADY-FINISHED"]);
    let pasted = given(&context, "chief")
        .into_iter()
        .filter(|text| text.contains(FINAL))
        .count();
    assert_eq!(pasted, 1, "the chief was given the words once");
}

#[test]
fn a_resume_while_the_window_is_still_at_work_on_its_answer_loses_nothing_of_the_turns_end() {
    // The chief is given the answer to its tell and resumes at once, as the tell told it to,
    // while the window goes on in the same turn: the resume waits behind the turn, and the
    // turn's final answer is not the answer to the resume's words (T-429's shape, with the
    // resume first).
    let context = Context::new();
    let (project, told) = told_at_work(&context);
    context
        .ledger
        .borrow_mut()
        .answer(told.id, told.recipient_id, Some(&json!(ANSWER)), None)
        .unwrap();
    context.resume_task(project, 1, "Go on, or say you finished");
    // Queued at the resume, and for as long as the window's record shows no work of its
    // own after it (`resumed_at_work`: with some, the task reads working meanwhile).
    assert_eq!(context.task(project, 1).task.state, "queued");
    context.adapter.answer("zeus", FINAL);
    context.pass().unwrap();

    assert_eq!(
        kept_notes(&context, project),
        [moved_on_words("zeus", &told)],
        "the turn on the tell ended with these words, though the task had moved on"
    );
    // The resume's words go in next; the follow-up they get is the result.
    for _ in 0..2 {
        context.pass().unwrap();
    }
    context.adapter.answer("zeus", "ALREADY-FINISHED");
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(context.task(project, 1).task.state, "done");
    assert_eq!(results(&context, project), ["ALREADY-FINISHED"]);
    assert_eq!(kept_notes(&context, project).len(), 1, "told once");
}

#[test]
fn a_task_taken_back_to_the_board_keeps_the_note_the_chief_has_not_read_in_the_words_of_a_task_that_moved_on(
) {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    let window = tiers.assignee(1);
    let told = tell_working(&context, tiers.project.id, &window);
    context.adapter.busy("chief");
    answers_then_goes_on_to_the_end(&context, &told);
    context.pass().unwrap();
    let note = kept_note(&context, tiers.project.id);
    assert_eq!(note.state, "queued");

    context
        .ledger
        .borrow_mut()
        .release_task(tiers.project.id, 1, "reassigned by the human")
        .unwrap();
    let kept = context.message(note.id);
    assert_eq!(
        (kept.state.as_str(), kept.body),
        ("queued", moved_on_words(&window, &told))
    );
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
fn finishing_after_the_chief_read_the_note_leaves_the_note_as_it_was_read() {
    let context = Context::new();
    let (project, told) = told_at_work(&context);
    answers_then_goes_on_to_the_end(&context, &told);
    context.pass().unwrap();
    for _ in 0..2 {
        chief_ends_its_turn(&context);
    }
    let note = kept_note(&context, project);
    assert_eq!(note.state, "delivered");

    context.finish_task(project, 1, "chief").expect("finished");
    assert_eq!(context.message(note.id).state, "delivered");
    assert_eq!(context.message(note.id).body, note.body);
}

#[test]
fn a_window_that_answers_the_tell_in_plain_text_has_its_words_put_to_the_chief_too() {
    // T-534: no `cf answer`, the answer and the final words in one reply.
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
            "T-1 is paused, and @zeus's turn on the words put to its window (m-{}) is over. What it said last:\n\n{FINAL}\n\nTo take it as the result, Finish it in the task's drawer; to send it on, Resume it there.",
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

#[test]
fn a_tells_turn_that_ended_with_a_monitor_that_said_it_ended_is_told_once_a_look_finds_no_command()
{
    // Calliope's P2 on round 2: a worker's tell answered, its turn on with a Monitor armed
    // that ran out of time (Claude Code kills its command as it writes the notice). The
    // pass that sees the notice looks at nothing, the next looks and finds no command, and
    // the turn's end is told, as a working task's would be a result.
    let context = Context::new();
    let (project, told) = told_at_work(&context);
    context.background.commands_run(Some(0));
    answers_and_goes_on(&context, &told);
    context.adapter.answer_leaving_monitors(
        "zeus",
        FINAL,
        pending(1, 1),
        vec![monitor("ticks", Source::Command, Some(End::Expired))],
    );
    context.pass().unwrap();
    assert_eq!(
        context.background.looks(),
        0,
        "no look on the pass that saw it"
    );
    assert_eq!(
        kept_notes(&context, project),
        Vec::<String>::new(),
        "none before the look"
    );
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(context.background.looks(), 1, "one look found it over");
    assert_eq!(context.task(project, 1).task.state, "paused");
    assert_eq!(
        kept_notes(&context, project),
        [paused_words("zeus", &told)],
        "told once"
    );
}

#[test]
fn a_tells_turn_whose_host_cannot_tell_the_windows_commands_is_not_told_over_until_a_look_finds_none(
) {
    // T-906 (T-858's live `tell` on zeewin2): Windows' host answered that it cannot tell
    // the window's commands, and the paused task's turn, its Monitor's notice come, was
    // never told over. Nothing is told blind: told once a look finds no command.
    let context = Context::new();
    let (project, told) = told_at_work(&context);
    context.background.commands_run(None);
    answers_and_goes_on(&context, &told);
    context.adapter.answer_leaving_monitors(
        "zeus",
        FINAL,
        pending(1, 1),
        vec![monitor("ticks", Source::Command, Some(End::Expired))],
    );
    for _ in 0..8 {
        context.advance(60_000);
        context.pass().unwrap();
    }
    assert!(context.background.looks() >= 2, "it looked");
    assert_eq!(
        kept_notes(&context, project),
        Vec::<String>::new(),
        "where the host cannot tell, nothing says the turn is over"
    );
    context.background.commands_run(Some(0));
    for _ in 0..2 {
        context.advance(60_000);
        context.pass().unwrap();
    }
    assert_eq!(context.task(project, 1).task.state, "paused");
    assert_eq!(kept_notes(&context, project), [paused_words("zeus", &told)]);
}

#[test]
fn a_tells_turn_whose_monitor_command_still_runs_is_told_when_it_has_ended() {
    let context = Context::new();
    let (project, told) = told_at_work(&context);
    context.background.commands_run(Some(1));
    answers_and_goes_on(&context, &told);
    context.adapter.answer_leaving_monitors(
        "zeus",
        FINAL,
        pending(1, 1),
        vec![monitor("ticks", Source::Command, Some(End::Expired))],
    );
    // Two looks that find a command running, the second after the wait between looks: it is
    // told of a working task at the second (`running_looks`), and a paused task's turn is not
    // over before then either.
    for _ in 0..3 {
        context.advance(4_000);
        context.pass().unwrap();
    }
    assert!(context.background.looks() >= 2);
    assert_eq!(
        kept_notes(&context, project),
        Vec::<String>::new(),
        "a command of the monitor runs still"
    );
    // The command ends: the next look, once the wait between looks is over, finds none.
    context.background.commands_run(Some(0));
    for _ in 0..4 {
        context.advance(4_000);
        context.pass().unwrap();
    }
    assert_eq!(kept_notes(&context, project), [paused_words("zeus", &told)]);
}

#[test]
fn a_second_tells_turn_first_seen_settled_with_another_monitor_that_said_it_ended_is_told_only_after_a_look_of_its_own(
) {
    // Calliope's P3 on round 3: the look of the first tell's turn found no command and its
    // result is kept, as the harness counts a monitor that said it ended for the window's
    // life. A second tell's turn that arms another, which expires between two passes, is
    // first seen already settled, with the count one higher: what the first look found says
    // nothing of the new monitor's command, and is not taken for a look at it.
    let context = Context::new();
    let (project, told) = told_at_work(&context);
    context.background.commands_run(Some(0));
    answers_and_goes_on(&context, &told);
    let first = monitor("ticks", Source::Command, Some(End::Expired));
    context
        .adapter
        .answer_leaving_monitors("zeus", FINAL, pending(1, 1), vec![first.clone()]);
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(context.background.looks(), 1);
    assert_eq!(kept_notes(&context, project), [paused_words("zeus", &told)]);

    // The chief tells it again; the pass that puts the question into the window is the last
    // that reads the first turn, and the window's whole second turn is written before the
    // next: the new monitor armed and expired between the two passes.
    let again = context.tell(project, "zeus", 1, "Once more");
    context.pass().unwrap();
    assert_eq!(
        context.message(again.id).state,
        "delivering",
        "pasted, not yet read"
    );
    let mut items = context.adapter.agent("zeus").items;
    let mut second_words = context.adapter.item(Role::Assistant, "SECOND-RESULT");
    second_words.complete = true;
    items.push(second_words);
    context.adapter.with("zeus", |agent| {
        agent.items = items;
        agent.settled = true;
        agent.pending = pending(2, 2);
        agent.monitors = vec![first, monitor("more", Source::Command, Some(End::Expired))];
    });
    context.pass().unwrap();
    assert_eq!(
        kept_notes(&context, project).len(),
        1,
        "not told on the strength of the first turn's look"
    );
    assert_eq!(context.background.looks(), 1);
    context.pass().unwrap();
    assert_eq!(context.background.looks(), 2, "a look of its own");
    assert_eq!(kept_notes(&context, project).len(), 2);
}

#[test]
fn finish_after_the_look_found_the_monitor_over_says_nothing_of_the_monitor_in_the_result() {
    // Calliope's P3 on round 3: the note said the turn was over and Finish's result said the
    // harness still counted a monitor that said it ended, from the raw reading.
    let context = Context::new();
    let (project, told) = told_at_work(&context);
    context.background.commands_run(Some(0));
    answers_and_goes_on(&context, &told);
    context.adapter.answer_leaving_monitors(
        "zeus",
        FINAL,
        pending(1, 1),
        vec![monitor("ticks", Source::Command, Some(End::Expired))],
    );
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(kept_notes(&context, project), [paused_words("zeus", &told)]);

    let finished = context.finish_task(project, 1, "chief").expect("finished");
    assert_eq!((finished.jobs, finished.agents), (0, 0));
    assert_eq!(
        results(&context, project),
        [format!(
            "{FINAL}\n\n(ConsensFlow: Finished at @chief's word: what @zeus said last is the result.)"
        )],
        "the monitor is over, as the note said"
    );
}

#[test]
fn finish_waits_for_a_tell_the_human_has_not_passed_on_and_takes_no_answer_from_before_it() {
    // The gated arm of the refusal (the project's human approves what the staff says to one
    // another): the tell waits at the gate, the task is paused for it, and the record holds
    // the turn from before it.
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context
        .ledger
        .borrow_mut()
        .set_gate(project.id, true)
        .unwrap();
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    let brief = context.task(project.id, 1).messages[0].id;
    context
        .ledger
        .borrow_mut()
        .approve_message(brief, "human")
        .unwrap();
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(context.task(project.id, 1).task.state, "working");
    context
        .adapter
        .answer_leaving_a_job("zeus", "Stage one is done; the build runs");
    context.pass().unwrap();
    let told = context.tell(project.id, "zeus", 1, "Stop and tell me where you are");
    assert_eq!(told.state, "gated");
    assert_eq!(context.task(project.id, 1).task.state, "paused");

    let refused = context.finish_task(project.id, 1, "chief").unwrap_err();
    assert!(
        refused.to_string().starts_with(
            "cannot finish T-1: the words you told @zeus are on their way to its window"
        ),
        "{refused}"
    );
    assert_eq!(results(&context, project.id), Vec::<String>::new());

    // Passed on and pasted, the window answers it, and Finish takes that answer.
    context
        .ledger
        .borrow_mut()
        .approve_message(told.id, "human")
        .unwrap();
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(context.message(told.id).state, "delivered");
    context
        .adapter
        .answer("zeus", "Where I am: the parser is done");
    context.pass().unwrap();
    context
        .finish_task(project.id, 1, "chief")
        .expect("finished");
    assert!(results(&context, project.id)[0].starts_with("Where I am: the parser is done"));
}

/// The note T-1's window left for its requester while the task was paused, in its words.
fn paused_words(handle: &str, told: &MessageView) -> String {
    format!(
        "T-1 is paused, and @{handle}'s turn on the words put to its window (m-{}) is over. What it said last:\n\n{FINAL}\n\nIf that is the result, take it: cf task finish T-1. To send it on: cf task resume T-1 \"…\".",
        told.id
    )
}

#[test]
fn finish_waits_for_the_answer_to_a_tell_that_is_not_in_the_window_yet_and_takes_no_answer_from_before_it(
) {
    // Between `cf tell` and its paste the record holds the turn from before the tell: the
    // window wrote a complete answer, and a job it left running keeps the task working.
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    context
        .adapter
        .answer_leaving_a_job("zeus", "Stage one is done; the build runs");
    context.pass().unwrap();
    assert_eq!(
        context.task(project.id, 1).task.state,
        "working",
        "held for its job"
    );
    context.tell(project.id, "zeus", 1, "Stop and tell me where you are");
    assert_eq!(context.task(project.id, 1).task.state, "paused");

    let refused = context.finish_task(project.id, 1, "chief").unwrap_err();
    assert!(
        refused.to_string().starts_with(
            "cannot finish T-1: the words you told @zeus are on their way to its window"
        ),
        "{refused}"
    );
    assert_eq!(results(&context, project.id), Vec::<String>::new());
    assert_eq!(context.task(project.id, 1).task.state, "paused");
}

#[test]
fn finish_takes_the_last_complete_answer_of_a_task_paused_without_a_tell() {
    // The chief's explicit call, for any paused task: what its window wrote last since the
    // latest message it was pasted.
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    context.adapter.busy("zeus");
    context.pause_task(project.id, 1);
    context.pass().unwrap();
    context.adapter.answer("zeus", "Half a parser");
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "paused");

    let finished = context
        .finish_task(project.id, 1, "chief")
        .expect("finished");
    assert_eq!(finished.task.state, "done");
    assert!(results(&context, project.id)[0].starts_with("Half a parser"));
}

#[test]
fn a_resumed_turn_that_fails_fails_the_task_and_leaves_the_note_of_the_tells_answer() {
    // Calliope's P3: the early resume's "moved on" note is queued (the chief is at work) when
    // the resumed window's turn fails and the task fails with it; dropping the task's queued
    // messages took the note, and the answer with it, since the tell has its note already.
    let context = Context::new();
    let (project, told) = told_at_work(&context);
    context.adapter.busy("chief");
    context
        .ledger
        .borrow_mut()
        .answer(told.id, told.recipient_id, Some(&json!(ANSWER)), None)
        .unwrap();
    context.resume_task(project, 1, "Go on, or say you finished");
    context.adapter.answer("zeus", FINAL);
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(
        context.task(project, 1).task.state,
        "working",
        "the resume went in"
    );
    let note = kept_note(&context, project);
    assert_eq!(note.state, "queued");

    // The harness reports the resumed turn failed.
    context.adapter.with("zeus", |agent| {
        agent.failed = true;
        agent.settled = true;
    });
    for _ in 0..2 {
        context.pass().unwrap();
    }
    assert_eq!(context.task(project, 1).task.state, "failed");
    let kept = context.message(note.id);
    assert_eq!(
        kept.state, "queued",
        "the answer to the tell is not dropped with the task"
    );
    assert_eq!(kept.body, moved_on_words("zeus", &told));
}

// ---------------------------------------------------------------------------------------
// T-1151: the words a window writes in reply to a tell, before its first tool call.
//
// `live tell devin` on the Mac (T-1131's sweep, 2026-11-02, Devin 3000.11.234, swe-2): the
// chief told the window "First write the line TOLD-ANSWER-K3 as text. Then run `sleep 15`.
// Then end your reply with FINAL-RESULT-ALPHA-9Z". Devin did: it wrote the line, ran the
// command, ran `cf answer m-2` (with the last line, in the run that did) and ended with the
// last line. The note said "What it said last: FINAL-RESULT-ALPHA-9Z" and nothing carried the
// line it wrote first: the rule took only what follows the turn's last tool call, and called
// the rest progress. The kept ledgers of the sweep's Claude, Codex and Muse runs hold the
// same record shape (text, tool, tool, `cf answer`, text); theirs reached the chief only
// because the model put the words into its `cf answer` (Claude, Codex) or wrote them again at
// the end (Muse).

/// What the window writes in reply to the tell, before it runs anything.
const WRITTEN: &str = "TOLD-LINE: stopped on the v2 grammar, not running it again";

/// Items pushed to zeus's record, in order, as `(role, text, complete)`.
fn record_of_zeus(context: &Context, items: &[(Role, &str, bool)]) {
    let built: Vec<_> = items
        .iter()
        .map(|&(role, text, complete)| {
            let mut item = context.adapter.item(role, text);
            item.complete = complete;
            item
        })
        .collect();
    context
        .adapter
        .with("zeus", |agent| agent.items.extend(built));
}

/// The turn Devin 3000.11.234 recorded on the tell (the kept ledger of T-1131's live run, its
/// items 15 to 21): the text, the command (`sleep 15`, put in the background after 10 s), a
/// second call, then, where it ran `cf answer`, that call and its output, and its last words.
fn devin_turn(context: &Context, told: &MessageView, answered_with: Option<&str>) {
    record_of_zeus(
        context,
        &[
            (Role::Assistant, WRITTEN, false),
            (
                Role::Tool,
                "No output produced after 10.0 seconds. Command running in background with ID: d186ce. Run get_output with timeout if you need to wait.",
                true,
            ),
            (Role::Assistant, "", false),
            (Role::Tool, "Exited with code 0 and no output\n\nExit code: 0", true),
        ],
    );
    if let Some(words) = answered_with {
        context
            .ledger
            .borrow_mut()
            .answer(told.id, told.recipient_id, Some(&json!(words)), None)
            .unwrap();
        record_of_zeus(
            context,
            &[
                (Role::Assistant, "", false),
                (
                    Role::Tool,
                    "Output from command in shell 98fc41:\nm-3 answered @chief.\n\nExit code: 0",
                    true,
                ),
            ],
        );
    }
    context.adapter.answer("zeus", FINAL);
}

/// The note of a paused task whose window wrote `reply` before its first tool call.
fn paused_words_after(reply: &str, handle: &str, told: &MessageView) -> String {
    format!(
        "T-1 is paused, and @{handle}'s turn on the words put to its window (m-{}) is over. What it wrote first, before any tool:\n\n{reply}\n\nWhat it said last:\n\n{FINAL}\n\nIf that is the result, take it: cf task finish T-1. To send it on: cf task resume T-1 \"…\".",
        told.id
    )
}

/// The same note of a task that has moved on.
fn moved_on_words_after(reply: &str, handle: &str, told: &MessageView) -> String {
    format!(
        "T-1, paused when @{handle}'s turn on the words put to its window (m-{}) began, has moved on since (resumed, or back on the board). This is not its result: that comes as a message of its own, the answer of the turn its resume, or its next assignment, begins, and it is the one to decide. What the turn on those words wrote first, before any tool:\n\n{reply}\n\nand ended with:\n\n{FINAL}",
        told.id
    )
}

/// zeus's turn on the tell ends, and the dispatcher looks at it.
fn passes_after_the_turn(context: &Context) {
    for _ in 0..4 {
        context.pass().unwrap();
    }
}

#[test]
fn the_words_a_window_wrote_before_its_first_tool_in_the_turn_of_a_tell_reach_the_chief_beside_its_last_when_it_ran_no_cf_answer(
) {
    // The first of T-1131's two Devin runs: no `cf answer` at all, the line written as text.
    let context = Context::new();
    let (project, told) = told_at_work(&context);
    devin_turn(&context, &told, None);
    passes_after_the_turn(&context);

    assert_eq!(
        kept_notes(&context, project),
        [paused_words_after(WRITTEN, "zeus", &told)]
    );
    assert_eq!(context.task(project, 1).task.state, "paused");
    assert_eq!(results(&context, project), Vec::<String>::new());
}

#[test]
fn the_words_a_window_wrote_before_its_first_tool_reach_the_chief_though_it_answered_the_tell_with_other_words(
) {
    // The second run, which T-1131 reran alone: `cf answer m-2` carried the last line, and the
    // line written first was in the record and nowhere the chief looks.
    let context = Context::new();
    let (project, told) = told_at_work(&context);
    devin_turn(&context, &told, Some(FINAL));
    passes_after_the_turn(&context);

    assert_eq!(
        kept_notes(&context, project),
        [paused_words_after(WRITTEN, "zeus", &told)]
    );
    let thread = context.task(project, 1);
    assert!(thread
        .messages
        .iter()
        .any(|message| message.kind == "answer" && message.body == FINAL));
}

#[test]
fn the_progress_a_window_writes_between_its_tools_stays_out_of_the_note() {
    let context = Context::new();
    let (project, told) = told_at_work(&context);
    record_of_zeus(
        &context,
        &[
            (Role::Assistant, WRITTEN, false),
            (Role::Tool, "(Bash completed)", true),
            (Role::Assistant, "Now reading the lexer", false),
            (Role::Tool, "(Read completed)", true),
            (Role::Assistant, "And the parser", false),
            (Role::Tool, "(Read completed)", true),
        ],
    );
    context.adapter.answer("zeus", FINAL);
    passes_after_the_turn(&context);

    assert_eq!(
        kept_notes(&context, project),
        [paused_words_after(WRITTEN, "zeus", &told)],
        "its reply and its last words: the steps between are in the transcript"
    );
}

#[test]
fn a_turn_that_runs_a_tool_before_it_writes_anything_has_no_reply_to_tell() {
    let context = Context::new();
    let (project, told) = told_at_work(&context);
    record_of_zeus(
        &context,
        &[
            (Role::Assistant, "", false),
            (Role::Tool, "(Bash completed)", true),
            (Role::Assistant, "Reading on", false),
            (Role::Tool, "(Read completed)", true),
        ],
    );
    context.adapter.answer("zeus", FINAL);
    passes_after_the_turn(&context);

    assert_eq!(kept_notes(&context, project), [paused_words("zeus", &told)]);
}

#[test]
fn a_turn_of_one_run_has_no_reply_apart_from_its_last_words() {
    let context = Context::new();
    let (project, told) = told_at_work(&context);
    context.adapter.answer("zeus", FINAL);
    passes_after_the_turn(&context);

    assert_eq!(kept_notes(&context, project), [paused_words("zeus", &told)]);
}

#[test]
fn a_reply_a_harness_marks_as_progress_is_still_the_reply_to_the_tell() {
    // Codex marks the text it writes beside a tool call `commentary` where its phase says so,
    // whatever it says; the chief asked a question, and that text is the first words of its
    // answer.
    let context = Context::new();
    let (project, told) = told_at_work(&context);
    let mut said = context.adapter.item(Role::Assistant, WRITTEN);
    said.commentary = true;
    said.complete = false;
    let tool = context.adapter.item(Role::Tool, "Script completed");
    context.adapter.with("zeus", |agent| {
        agent.items.push(said);
        agent.items.push(tool);
    });
    context.adapter.answer("zeus", FINAL);
    passes_after_the_turn(&context);

    assert_eq!(
        kept_notes(&context, project),
        [paused_words_after(WRITTEN, "zeus", &told)]
    );
}

#[test]
fn what_a_harness_wrote_itself_is_no_reply() {
    // Claude Code's close of a turn it found cut is an assistant's item with no model behind it.
    let context = Context::new();
    let (project, told) = told_at_work(&context);
    let mut closed = context
        .adapter
        .item(Role::Assistant, "No response requested.");
    closed.synthetic = true;
    closed.commentary = true;
    let tool = context.adapter.item(Role::Tool, "(Bash completed)");
    context.adapter.with("zeus", |agent| {
        agent.items.push(closed);
        agent.items.push(tool);
    });
    context.adapter.answer("zeus", FINAL);
    passes_after_the_turn(&context);

    assert_eq!(kept_notes(&context, project), [paused_words("zeus", &told)]);
}

#[test]
fn a_reply_the_turn_ends_with_again_is_told_once() {
    // Muse wrote the line first and again at the end: one copy.
    let context = Context::new();
    let (project, told) = told_at_work(&context);
    record_of_zeus(
        &context,
        &[
            (Role::Assistant, FINAL, true),
            (Role::Tool, "(Bash completed)", true),
        ],
    );
    context.adapter.answer("zeus", FINAL);
    passes_after_the_turn(&context);

    assert_eq!(kept_notes(&context, project), [paused_words("zeus", &told)]);
}

#[test]
fn a_reply_the_window_also_sent_with_cf_answer_is_not_told_again() {
    // Claude and Codex wrote the line and put it into `cf answer`: the chief has it as a
    // message of its own.
    let context = Context::new();
    let (project, told) = told_at_work(&context);
    devin_turn(&context, &told, Some(&format!("Done: {WRITTEN}, and on")));
    passes_after_the_turn(&context);

    assert_eq!(kept_notes(&context, project), [paused_words("zeus", &told)]);
}

#[test]
fn a_cf_answer_to_an_earlier_tell_does_not_stand_for_the_reply_to_this_one() {
    // The first tell was answered with `cf answer`, in words that hold the line; the second
    // tell's turn wrote the same line first and answered nothing: the chief has no message of
    // its own for it, and the note must hold it.
    let context = Context::new();
    let (project, told) = told_at_work(&context);
    devin_turn(&context, &told, Some(&format!("Done: {WRITTEN}, and on")));
    passes_after_the_turn(&context);
    assert_eq!(kept_notes(&context, project), [paused_words("zeus", &told)]);

    let again = context.tell(project, "zeus", 1, "Once more");
    context.pass().unwrap();
    assert_eq!(
        context.message(again.id).state,
        "delivering",
        "pasted, not yet read"
    );
    record_of_zeus(
        &context,
        &[
            (Role::Assistant, WRITTEN, false),
            (Role::Tool, "(Bash completed)", true),
        ],
    );
    context.adapter.answer("zeus", "SECOND-RESULT");
    passes_after_the_turn(&context);

    let kept = kept_notes(&context, project);
    assert_eq!(kept.len(), 2, "{kept:?}");
    assert!(
        kept[1].contains(&format!(
            "What it wrote first, before any tool:\n\n{WRITTEN}\n\nWhat it said last:\n\nSECOND-RESULT"
        )),
        "{}",
        kept[1]
    );
}

#[test]
fn finish_takes_only_the_last_words_of_a_turn_that_wrote_a_reply_as_the_result() {
    let context = Context::new();
    let (project, told) = told_at_work(&context);
    devin_turn(&context, &told, Some(FINAL));
    passes_after_the_turn(&context);
    assert_eq!(kept_notes(&context, project).len(), 1);
    let _ = told;

    let finished = context.finish_task(project, 1, "chief").expect("finished");
    assert_eq!(finished.task.state, "done");
    assert_eq!(
        results(&context, project),
        [format!(
            "{FINAL}\n\n(ConsensFlow: Finished at @chief's word: what @zeus said last is the result.)"
        )]
    );
}

#[test]
fn a_resume_before_the_chief_read_the_note_keeps_the_reply_in_the_words_of_a_task_that_moved_on() {
    let context = Context::new();
    let (project, told) = told_at_work(&context);
    context.adapter.busy("chief");
    devin_turn(&context, &told, Some(FINAL));
    context.pass().unwrap();
    let note = kept_note(&context, project);
    assert_eq!(note.state, "queued");
    assert_eq!(note.body, paused_words_after(WRITTEN, "zeus", &told));

    context.resume_task(project, 1, "Go on, or say you finished");
    let kept = context.message(note.id);
    assert_eq!(kept.state, "queued", "the resume does not take it back");
    assert_eq!(kept.body, moved_on_words_after(WRITTEN, "zeus", &told));
}

#[test]
fn the_human_is_given_the_reply_too() {
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
    context.adapter.busy("zeus");
    let told = context.tell(project.id, "zeus", 1, "Stop, answer, and carry on");
    context.pass().unwrap();
    context.adapter.answer("zeus", "Half a parser");
    context.pass().unwrap();
    context.pass().unwrap();
    context.adapter.busy("zeus");
    devin_turn(&context, &told, None);
    passes_after_the_turn(&context);

    assert_eq!(
        notes(&context, context.id(project.id, "human")),
        [format!(
            "T-1 is paused, and @zeus's turn on the words put to its window (m-{}) is over. What it wrote first, before any tool:\n\n{WRITTEN}\n\nWhat it said last:\n\n{FINAL}\n\nTo take it as the result, Finish it in the task's drawer; to send it on, Resume it there.",
            told.id
        )]
    );
}
