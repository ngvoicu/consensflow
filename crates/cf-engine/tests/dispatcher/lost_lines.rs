//! A line of a window's record torn and passed over (T-223, T-248, T-290): the
//! reader goes on past it, and says where in the reading (`Record::lost`), with
//! the record each item is of (`Record::places`). A record lost in a turn may
//! have been part of the turn's answer, so a result taken past one ends with a
//! line for the requester that says so, and the window's activity says it while
//! the turn goes on. The turn is what follows the paste that began it in the
//! record, so a line lost before that paste's own record is no part of it, after
//! a restart or a resume as before. Devin's reading cannot say the record of an
//! item (its items are its store's): the count taken at the last hand-over
//! stands in. Under turn-end's rule (T-494) a turn that leaves a job running holds
//! its task, and its result is the final answer of the turn its job's end wakes, or
//! the last complete answer `cf task finish` takes: a line lost in the turn may have
//! been that answer, so either result is marked, ahead of what ConsensFlow says of
//! the leftovers, and so is the note that says it goes on in the background. A line
//! lost in a turn that comes to no result (its final answer, or the record that ends
//! it, may be the line) is said to the requester once, and Finish's refusal names it
//! (T-628, Calliope's T-624). Node had no such lines, so none of these is held to a
//! Node trace.

use cf_engine::testing::Context;
use cf_harness::records::{Pending, Role};
use cf_ledger::{NewNote, NewTask};

use crate::fixtures::{given, Tiers};

/// T-1 given to zeus, whose window has taken it in. The host proves each window's
/// program gone, as the app's does, so a follow-up goes on in the same conversation
/// (`ownership`), whose record holds what was lost before it.
fn working(context: &Context) -> Tiers<'_> {
    context.host.exits_showing(Some(0), None, None);
    let tiers = Tiers::new(context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");
    tiers
}

/// The results the chief was given or is to be given.
fn results(context: &Context, tiers: &Tiers<'_>) -> Vec<String> {
    context
        .inbox(tiers.id("chief"))
        .into_iter()
        .filter(|message| message.kind == "result")
        .map(|message| message.body)
        .collect()
}

/// What the window of `handle` says it is doing, if it says why.
fn reason(context: &Context, tiers: &Tiers<'_>, handle: &str) -> Option<String> {
    context.dispatcher.activity(tiers.id(handle)).reason
}

/// The line a result taken past the line `place` of `worker`'s record ends with.
fn marked(answer: &str, worker: &str, place: usize) -> String {
    format!(
        "{answer}\n\nA line of @{worker}'s record was lost (record {place}); this result may miss part of the turn."
    )
}

/// T-2, which follows T-1 into a window on its conversation.
fn follow_up(context: &Context, tiers: &Tiers<'_>) {
    context.create_task(
        tiers.project.id,
        NewTask {
            from: "chief".to_owned(),
            after: Some(1),
            body: "Now the lexer".to_owned(),
            ..NewTask::default()
        },
    );
}

/// A result taken past a line lost in its turn is marked, and the window says it.
fn lost_in_the_turn_is_marked(context: &Context) {
    let tiers = working(context);
    let worker = tiers.assignee(1);
    assert_eq!(reason(context, &tiers, &worker), None);
    let place = context.adapter.loses_a_line("zeus");
    context.pass().unwrap();
    assert_eq!(
        reason(context, &tiers, &worker),
        Some(format!(
            "a line of @{worker}'s record was lost (record {place})"
        )),
        "said while the turn goes on"
    );
    context.adapter.answer("zeus", "Parser done");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "done");
    assert_eq!(
        results(context, &tiers),
        [marked("Parser done", &worker, place)]
    );
}

#[test]
fn a_result_taken_past_a_line_lost_in_its_turn_ends_with_a_line_for_the_chief_and_the_window_says_it(
) {
    lost_in_the_turn_is_marked(&Context::new());
}

#[test]
fn devins_result_taken_past_a_line_lost_in_its_turn_is_marked_by_the_count_at_the_hand_over() {
    let context = Context::new();
    context.adapter.says_places.set(false);
    lost_in_the_turn_is_marked(&context);
}

/// T-1 done past a line its record lost, and T-2 that follows it into a window
/// opened again on the record, done.
fn lost_before_the_follow_up(context: &Context) {
    context.adapter.resumes_records.set(true);
    let tiers = working(context);
    let worker = tiers.assignee(1);
    context.adapter.loses_a_line("zeus");
    context.pass().unwrap();
    context.adapter.answer("zeus", "Parser done");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "done");

    // T-2 follows T-1 into a window on the same conversation, whose record lost
    // the line before it.
    follow_up(context, &tiers);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(2).task.state, "working");
    assert_eq!(
        reason(context, &tiers, &worker),
        None,
        "its turn lost nothing"
    );
    context.adapter.answer("zeus", "Lexer done");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(2).task.state, "done");
    let results = results(context, &tiers);
    assert!(results.contains(&"Lexer done".to_owned()), "{results:?}");
}

#[test]
fn a_line_lost_before_a_message_was_handed_over_is_no_part_of_its_turn() {
    lost_before_the_follow_up(&Context::new());
}

#[test]
fn devins_line_lost_before_a_launch_s_first_message_is_no_part_of_its_turn() {
    let context = Context::new();
    context.adapter.says_places.set(false);
    lost_before_the_follow_up(&context);
}

/// A note pasted into the chief's open window, after a line its record lost.
fn lost_before_a_paste_into_an_open_window(context: &Context) {
    let tiers = Tiers::new(context, &["zeus"]);
    context.pass().unwrap();
    context.pass().unwrap();
    context.adapter.loses_a_line("chief");
    context.pass().unwrap();
    context.note_from_consensflow(tiers.project.id, "chief", "Word one");
    context.pass().unwrap();
    context.pass().unwrap();
    assert!(
        given(context, "chief")
            .iter()
            .any(|text| text.contains("Word one")),
        "{:?}",
        given(context, "chief")
    );
    assert_eq!(
        reason(context, &tiers, "chief"),
        None,
        "its turn lost nothing"
    );
}

#[test]
fn a_message_pasted_into_an_open_window_begins_a_turn_that_has_lost_nothing_yet() {
    lost_before_a_paste_into_an_open_window(&Context::new());
}

#[test]
fn devins_message_pasted_into_an_open_window_begins_a_turn_that_has_lost_nothing_yet() {
    let context = Context::new();
    context.adapter.says_places.set(false);
    lost_before_a_paste_into_an_open_window(&context);
}

/// The daemon stopped and started again, and the human resumed the project.
fn restarted<'a>(context: &'a Context, tiers: &Tiers<'_>) -> cf_engine::testing::Restarted<'a> {
    context.ledger.borrow_mut().suspend_for_restart().unwrap();
    let after = context.make();
    after.settle_after_restart().unwrap();
    after.resume_project(tiers.project.id).unwrap();
    after
}

#[test]
fn a_line_lost_before_a_turn_is_no_part_of_it_after_a_restart() {
    // Nothing of the engine's is kept across the restart: the record says where the
    // turn began. Since turn-end another task goes on in a new conversation after a
    // restart, and a task in its own (T-494): so T-2 follows T-1 into its conversation,
    // past the line lost there, before the restart.
    let context = Context::new();
    context.adapter.resumes_records.set(true);
    let tiers = working(&context);
    context.adapter.answer("zeus", "Parser done");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "done");
    context.adapter.loses_a_line("zeus");
    follow_up(&context, &tiers);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(2).task.state, "working");

    let after = restarted(&context, &tiers);
    for _ in 0..3 {
        after.pass().unwrap();
    }
    assert_eq!(tiers.task(2).task.state, "working");
    context.adapter.answer("zeus", "Lexer done");
    after.pass().unwrap();
    after.pass().unwrap();
    assert_eq!(tiers.task(2).task.state, "done");
    let results = results(&context, &tiers);
    assert!(results.contains(&"Lexer done".to_owned()), "{results:?}");
}

#[test]
fn a_line_lost_in_a_turn_after_a_restart_marks_its_result() {
    let context = Context::new();
    context.adapter.resumes_records.set(true);
    let tiers = working(&context);
    let worker = tiers.assignee(1);

    let after = restarted(&context, &tiers);
    for _ in 0..3 {
        after.pass().unwrap();
    }
    assert_eq!(tiers.task(1).task.state, "working");
    let place = context.adapter.loses_a_line("zeus");
    context.adapter.answer("zeus", "Parser done");
    after.pass().unwrap();
    after.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "done");
    assert_eq!(
        results(&context, &tiers),
        [marked("Parser done", &worker, place)]
    );
}

/// The notification Claude Code begins a turn with when a job of its ends.
const ENDED: &str =
    "<task-notification>\n<task-id>b1</task-id>\n<status>completed</status>\n</task-notification>";

/// A job, as a reading counts it.
const A_JOB: Pending = Pending {
    agents: 0,
    jobs: 1,
    retired: 0,
    noticed: 0,
};

/// T-1's turn loses a line, and ends with its job running: the task is held, and
/// what its job's end wakes the agent to answer is the result, marked.
fn lost_in_a_held_turn(context: &Context) {
    let tiers = working(context);
    let worker = tiers.assignee(1);
    let place = context.adapter.loses_a_line("zeus");
    context
        .adapter
        .answer_leaving("zeus", "The build runs; I will report when it ends", A_JOB);
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working", "held for its job");
    assert_eq!(results(context, &tiers), Vec::<String>::new());

    context.adapter.wake("zeus", ENDED);
    context.pass().unwrap();
    context.adapter.answer("zeus", "Build green");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "done");
    assert_eq!(
        results(context, &tiers),
        [marked("Build green", &worker, place)]
    );
}

#[test]
fn a_line_lost_in_a_turn_held_for_its_job_marks_the_result_its_jobs_end_brings() {
    lost_in_a_held_turn(&Context::new());
}

#[test]
fn devins_line_lost_in_a_turn_held_for_its_job_marks_the_result_its_jobs_end_brings() {
    let context = Context::new();
    context.adapter.says_places.set(false);
    lost_in_a_held_turn(&context);
}

/// T-1's turn loses a line and ends with its job running, and the chief finishes
/// the task: its result is the last complete answer, marked, and then what
/// ConsensFlow says of the job finishing ends none of.
fn lost_in_a_finished_turn(context: &Context) {
    let tiers = working(context);
    let worker = tiers.assignee(1);
    let place = context.adapter.loses_a_line("zeus");
    context
        .adapter
        .answer_leaving("zeus", "Serving the docs on 8000", A_JOB);
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working", "held for its job");

    let finished = context
        .finish_task(tiers.project.id, 1, "chief")
        .expect("finished");
    assert_eq!(finished.task.state, "done");
    let results = results(context, &tiers);
    assert_eq!(results.len(), 1, "{results:?}");
    let marked = marked("Serving the docs on 8000", &worker, place);
    assert!(
        results[0].starts_with(&format!(
            "{marked}\n\n(ConsensFlow: Finished at @chief's word while @{worker}'s harness still counted 1 background job running"
        )),
        "{results:?}"
    );
}

#[test]
fn a_task_finished_past_a_line_lost_in_its_turn_has_its_result_marked_ahead_of_the_leftovers() {
    lost_in_a_finished_turn(&Context::new());
}

#[test]
fn devins_task_finished_past_a_line_lost_in_its_turn_has_its_result_marked_ahead_of_the_leftovers()
{
    let context = Context::new();
    context.adapter.says_places.set(false);
    lost_in_a_finished_turn(&context);
}

#[test]
fn a_task_finished_past_a_line_lost_before_its_turn_has_its_result_unmarked() {
    // The record says where the turn began, so a line lost before the task's paste is no
    // part of what finishing takes.
    let context = Context::new();
    context.adapter.resumes_records.set(true);
    let tiers = working(&context);
    context.adapter.answer("zeus", "Parser done");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "done");
    context.adapter.loses_a_line("zeus");
    follow_up(&context, &tiers);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(2).task.state, "working");
    context
        .adapter
        .answer_leaving("zeus", "Lexing in the background", A_JOB);
    context.pass().unwrap();
    assert_eq!(tiers.task(2).task.state, "working", "held for its job");
    context
        .finish_task(tiers.project.id, 2, "chief")
        .expect("finished");
    let results = results(&context, &tiers);
    assert!(
        results
            .iter()
            .any(|result| result.starts_with("Lexing in the background\n\n(ConsensFlow: Finished")),
        "{results:?}"
    );
}

/// The notes the chief was told that T-1's turn may have lost its last line.
fn may_have_lost(tiers: &Tiers<'_>) -> Vec<String> {
    tiers
        .notes("chief")
        .into_iter()
        .filter(|note| note.starts_with("T-1's turn may have lost its last line"))
        .collect()
}

/// T-1's turn loses a line and comes to no result: its record says no end of it
/// (`under_way`), or an end with no complete answer. The chief is told once, and Finish,
/// refused for want of an answer, names the line.
fn lost_in_a_turn_that_does_not_end(context: &Context, under_way: bool) {
    let tiers = working(context);
    let worker = tiers.assignee(1);
    if under_way {
        context.adapter.busy("zeus");
    } else {
        // Its record says the turn ended on words that are no complete answer.
        context
            .adapter
            .writes("zeus", "Reading the grammar", "2026-10-26T06:00:00.000Z");
        context.adapter.with("zeus", |agent| agent.settled = true);
    }
    let place = context.adapter.loses_a_line("zeus");
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(tiers.task(1).task.state, "working");
    let told = may_have_lost(&tiers);
    assert_eq!(told.len(), 1, "told once: {told:?}");
    assert!(
        told[0].contains(&format!(
            "a line of @{worker}'s record was lost (record {place}) in the turn"
        )),
        "{told:?}"
    );
    assert!(told[0].contains("cf task finish T-1"), "{told:?}");

    let refused = context
        .finish_task(tiers.project.id, 1, "chief")
        .unwrap_err()
        .to_string();
    assert_eq!(
        refused,
        format!(
            "cannot finish T-1: @{worker} has written no complete answer since its latest message about it; a line of @{worker}'s record was lost (record {place}) in the turn, which may have been its answer: send it a message about the task, or cancel it"
        )
    );
}

#[test]
fn a_line_lost_in_a_turn_that_has_not_ended_is_told_to_the_chief_once_and_finish_names_it() {
    lost_in_a_turn_that_does_not_end(&Context::new(), true);
}

#[test]
fn a_line_lost_in_a_turn_that_ended_with_no_answer_is_told_to_the_chief_once_and_finish_names_it() {
    lost_in_a_turn_that_does_not_end(&Context::new(), false);
}

#[test]
fn devins_line_lost_in_a_turn_that_has_not_ended_is_told_to_the_chief_once_and_finish_names_it() {
    let context = Context::new();
    context.adapter.says_places.set(false);
    lost_in_a_turn_that_does_not_end(&context, true);
}

#[test]
fn a_held_turn_that_lost_a_line_says_so_in_the_note_that_it_goes_on_and_is_not_told_again() {
    let context = Context::new();
    let tiers = working(&context);
    let worker = tiers.assignee(1);
    let place = context.adapter.loses_a_line("zeus");
    context
        .adapter
        .answer_leaving("zeus", "The build runs", A_JOB);
    context.pass().unwrap();
    let held: Vec<String> = tiers
        .notes("chief")
        .into_iter()
        .filter(|note| note.starts_with("T-1 goes on in the background"))
        .collect();
    assert_eq!(held.len(), 1, "{held:?}");
    assert!(
        held[0].ends_with(&marked("The build runs", &worker, place)),
        "{held:?}"
    );
    // The turn its job's end wakes is under way: the line was said.
    context.adapter.wake("zeus", ENDED);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(may_have_lost(&tiers), Vec::<String>::new());
}

#[test]
fn a_line_lost_before_a_compaction_summary_that_quotes_the_brief_is_still_said_by_the_window() {
    // The summary restates the brief's paste, and is no paste of it: the turn the window
    // says it lost a line in is still the brief's.
    let context = Context::new();
    let tiers = working(&context);
    let worker = tiers.assignee(1);
    let place = context.adapter.loses_a_line("zeus");
    context.pass().unwrap();
    let said = Some(format!(
        "a line of @{worker}'s record was lost (record {place})"
    ));
    assert_eq!(reason(&context, &tiers, &worker), said);
    let brief = context
        .adapter
        .agent("zeus")
        .items
        .iter()
        .find(|item| item.role == Role::User)
        .expect("the brief's paste")
        .text
        .to_string();
    let mut summary = context.adapter.item(Role::User, &brief);
    summary.compaction = true;
    context
        .adapter
        .with("zeus", |agent| agent.items.push(summary));
    context.pass().unwrap();
    assert_eq!(reason(&context, &tiers, &worker), said);
}

#[test]
fn devins_note_handed_over_while_its_task_is_held_leaves_the_count_its_lost_lines_are_counted_from()
{
    // T-1's turn loses a line and ends with its job running; while it is held, the chief's
    // ConsensFlow's note about it goes into the window at rest, which is no message the
    // task's result is taken from. The result the job's end brings is still marked.
    let context = Context::new();
    context.adapter.says_places.set(false);
    let tiers = working(&context);
    let worker = tiers.assignee(1);
    let place = context.adapter.loses_a_line("zeus");
    context
        .adapter
        .answer_leaving("zeus", "The build runs", A_JOB);
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working", "held for its job");
    context
        .ledger
        .borrow_mut()
        .note(
            tiers.project.id,
            &NewNote {
                from: None,
                to: worker.clone(),
                task: Some(1),
                body: "Mind the disk".to_owned(),
            },
        )
        .unwrap();
    context.pass().unwrap();
    context.pass().unwrap();
    assert!(
        given(&context, "zeus")
            .iter()
            .any(|text| text.contains("Mind the disk")),
        "{:?}",
        given(&context, "zeus")
    );
    context.adapter.wake("zeus", ENDED);
    context.pass().unwrap();
    context.adapter.answer("zeus", "Build green");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "done");
    assert_eq!(
        results(&context, &tiers),
        [marked("Build green", &worker, place)]
    );
}

#[test]
fn devins_window_begun_afresh_counts_its_lost_lines_from_none_whatever_the_last_conversation_lost()
{
    // The host proves no window's program gone, so T-2 goes on in a new conversation.
    let context = Context::new();
    context.adapter.says_places.set(false);
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    let worker = tiers.assignee(1);
    context.adapter.loses_a_line("zeus");
    context.pass().unwrap();
    context.adapter.answer("zeus", "Parser done");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "done");

    follow_up(&context, &tiers);
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(tiers.task(2).task.state, "working");
    assert_eq!(
        context.adapter.prepared().last().unwrap()["resume"],
        serde_json::Value::Null,
        "a new conversation"
    );
    let place = context.adapter.loses_a_line("zeus");
    context.adapter.answer("zeus", "Lexer done");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(2).task.state, "done");
    let results = results(&context, &tiers);
    assert!(
        results.contains(&marked("Lexer done", &worker, place)),
        "{results:?}"
    );
}

#[test]
fn devins_window_switched_to_another_conversation_counts_its_lost_lines_afresh() {
    // T-2 follows T-1 past a line T-1's conversation lost, and the human switches the
    // window to another conversation, which loses a line: the window says that one.
    let context = Context::new();
    context.adapter.says_places.set(false);
    context.adapter.resumes_records.set(true);
    let tiers = working(&context);
    let worker = tiers.assignee(1);
    context.adapter.loses_a_line("zeus");
    context.adapter.answer("zeus", "Parser done");
    context.pass().unwrap();
    context.pass().unwrap();
    follow_up(&context, &tiers);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(2).task.state, "working");
    assert_eq!(reason(&context, &tiers, &worker), None, "T-2 lost nothing");

    context.adapter.switch_to("zeus", "native-other");
    context.pass().unwrap();
    let place = context.adapter.loses_a_line("zeus");
    context.pass().unwrap();
    assert_eq!(
        reason(&context, &tiers, &worker),
        Some(format!(
            "a line of @{worker}'s record was lost (record {place})"
        ))
    );
}
