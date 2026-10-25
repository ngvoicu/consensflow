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
//! the leftovers. Node had no such lines, so none of these is held to a Node trace.

use cf_engine::testing::Context;
use cf_harness::records::Pending;
use cf_ledger::NewTask;

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
