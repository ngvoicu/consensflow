//! Turn-end's smallest rule (the chief's decision of 2026-10-21, on the advisor's answer of
//! T-8), as the engine keeps it: a worker's turn that ends with its harness waiting on work
//! it wakes the agent for keeps its task working, and its words are progress (A2); the
//! result is the final answer after the task's latest message (A4); Finish takes the last
//! complete answer of any working task and ends nothing; and no other task's message goes
//! into a window a task's message went into, whatever released that task (`ownership`).
//! Node's dispatcher had none of it, so none of these is held to a Node trace.

use cf_engine::delivery_text::marker_of;
use cf_engine::testing::Context;
use cf_harness::records::{Pending, Role};
use cf_ledger::NewTask;

use crate::fixtures::{closed, Tiers};

/// The notification Claude Code begins a turn with when a job of its ends.
const ENDED: &str =
    "<task-notification>\n<task-id>b1</task-id>\n<status>completed</status>\n</task-notification>";

/// A job, and a job and an agent, as a reading counts them.
const A_JOB: Pending = Pending {
    agents: 0,
    jobs: 1,
    retired: 0,
};
const A_JOB_AND_AN_AGENT: Pending = Pending {
    agents: 1,
    jobs: 1,
    retired: 0,
};

/// The results the chief was given or is to be given, of T-`number`.
fn results_of(tiers: &Tiers<'_>, number: i64) -> Vec<String> {
    tiers
        .task(number)
        .messages
        .into_iter()
        .filter(|message| message.kind == "result")
        .map(|message| message.body)
        .collect()
}

/// A project of zeus and diana, the host proving each window's program gone as the app's
/// does, and T-1 given to zeus by name, at work in its window.
fn zeus_at_work(context: &Context) -> Tiers<'_> {
    context.host.exits_showing(Some(0), None, None);
    let tiers = Tiers::new(context, &["zeus", "diana"]);
    context.give(tiers.project.id, "zeus", "Record the run");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");
    tiers
}

/// T-2, from the human, given to zeus too: it waits for T-1.
fn docs_for_zeus(context: &Context, tiers: &Tiers<'_>) {
    context.create_task(
        tiers.project.id,
        NewTask {
            from: "human".to_owned(),
            to: Some("zeus".to_owned()),
            body: "Write the docs".to_owned(),
            ..NewTask::default()
        },
    );
    context.pass().unwrap();
    assert_eq!(tiers.task(2).task.state, "queued");
}

/// Passes enough for what is begun to be done.
fn passes(context: &Context, count: usize) {
    for _ in 0..count {
        context.pass().unwrap();
    }
}

#[test]
fn a_turn_that_ends_with_its_job_running_keeps_its_task_working_and_its_progress_is_no_result() {
    // dip-agentos's T-20 (3.0.4): the worker's turn ended while it waited on a background
    // build, and the result was its progress lines, with nothing committed.
    let context = Context::new();
    let tiers = zeus_at_work(&context);
    context
        .adapter
        .answer_leaving("zeus", "The build runs; I will report when it ends", A_JOB);
    context.pass().unwrap();

    assert_eq!(tiers.task(1).task.state, "working", "held for its job");
    assert_eq!(results_of(&tiers, 1), Vec::<String>::new());
    let told = tiers.notes("chief");
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(
        told[0].starts_with(
            "T-1 goes on in the background: @zeus ended its turn with 1 background job running"
        ),
        "{told:?}"
    );
    assert!(told[0].contains("which does not end it."), "{told:?}");

    // The time that passes, and a window the host finds nothing left in, release nothing.
    context.advance(6 * 3_600_000);
    passes(&context, 3);
    assert_eq!(tiers.task(1).task.state, "working");
    assert_eq!(tiers.notes("chief").len(), 1, "told once");

    // The build's end wakes the agent, whose report is the result, once.
    context.adapter.wake("zeus", ENDED);
    context.pass().unwrap();
    context
        .adapter
        .answer("zeus", "Build green: 412 tests; committed as fe76a3c");
    passes(&context, 2);
    assert_eq!(tiers.task(1).task.state, "done");
    assert_eq!(
        results_of(&tiers, 1),
        ["Build green: 412 tests; committed as fe76a3c"]
    );
}

#[test]
fn a_task_held_again_in_a_window_opened_again_is_told_again_and_its_lane_says_it_only_while_it_is()
{
    let context = Context::new();
    let tiers = zeus_at_work(&context);
    let zeus = tiers.id("zeus");
    context
        .adapter
        .answer_leaving("zeus", "Watching the run", A_JOB);
    context.pass().unwrap();
    assert_eq!(
        context.dispatcher.held_for(zeus).map(|hold| hold.task),
        Some(1)
    );
    // The window goes by itself: the task is paused, and nothing is held any more.
    context.exit("zeus");
    passes(&context, 2);
    assert_eq!(tiers.task(1).task.state, "paused");
    assert_eq!(context.dispatcher.held_for(zeus), None);
    // Resumed, it is held again in the window opened again, and told again.
    context.resume_task(tiers.project.id, 1, "Go on watching");
    passes(&context, 3);
    context
        .adapter
        .answer_leaving("zeus", "Still watching the run", A_JOB);
    context.pass().unwrap();
    let told = tiers
        .notes("chief")
        .into_iter()
        .filter(|note| note.starts_with("T-1 goes on in the background"))
        .count();
    assert_eq!(told, 2, "told for each time it is held");
    assert_eq!(
        context.dispatcher.held_for(zeus).map(|hold| hold.task),
        Some(1)
    );
}

#[test]
fn a_human_whose_task_goes_on_in_the_background_is_told_of_the_drawers_finish_and_the_chief_of_cf()
{
    // Calliope on 0eec91d2: the human finishes a task from its drawer, and has no `cf`.
    let context = Context::new();
    context.host.exits_showing(Some(0), None, None);
    let tiers = Tiers::new(&context, &["zeus", "diana"]);
    context.create_task(
        tiers.project.id,
        NewTask {
            from: "human".to_owned(),
            to: Some("zeus".to_owned()),
            body: "Record the run".to_owned(),
            ..NewTask::default()
        },
    );
    passes(&context, 2);
    assert_eq!(tiers.task(1).task.state, "working");
    context
        .adapter
        .answer_leaving("zeus", "Watching the run", A_JOB);
    context.pass().unwrap();
    let told = tiers.notes("human");
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(
        told[0].contains("To take what it said last as the result now: Finish in the task's drawer, which does not end it."),
        "{told:?}"
    );
    assert!(!told[0].contains("cf task finish"), "{told:?}");
}

#[test]
fn a_turn_woken_while_its_job_still_runs_is_progress_too() {
    // poker-lab's T-442 with its Monitor: each event wakes a turn, and the monitor runs on.
    let context = Context::new();
    let tiers = zeus_at_work(&context);
    context
        .adapter
        .answer_leaving("zeus", "Watching the run", A_JOB);
    context.pass().unwrap();
    for event in ["tick 1", "tick 2", "tick 3"] {
        context.adapter.wake("zeus", event);
        context.pass().unwrap();
        context
            .adapter
            .answer_leaving("zeus", &format!("Saw {event}"), A_JOB);
        context.pass().unwrap();
        assert_eq!(tiers.task(1).task.state, "working", "{event}");
    }
    assert_eq!(tiers.notes("chief").len(), 1, "told once");
    context.adapter.wake("zeus", ENDED);
    context.pass().unwrap();
    context.adapter.answer("zeus", "The run passed: 0 failures");
    passes(&context, 2);
    assert_eq!(results_of(&tiers, 1), ["The run passed: 0 failures"]);
}

#[test]
fn finish_takes_the_last_complete_answer_of_a_held_task_once_and_ends_nothing() {
    let context = Context::new();
    let (tiers, session) = session_kept_open(&context);
    let session_pane = context.host.last(&session).unwrap().pane;
    context
        .adapter
        .answer_leaving("zeus", "Serving the docs on 8000", A_JOB);
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");

    let finished = context
        .finish_task(tiers.project.id, 1, "chief")
        .expect("finished");
    assert_eq!(finished.task.state, "done");
    assert_eq!((finished.jobs, finished.agents), (1, 0));
    let results = results_of(&tiers, 1);
    assert_eq!(results.len(), 1);
    assert!(results[0].starts_with(&format!("Serving the docs on 8000\n\n(ConsensFlow: Finished at @chief's word while @{session}'s harness still counted 1 background job running, which finishing ends none of")), "{results:?}");
    passes(&context, 2);
    assert!(!closed(&context, &session_pane), "the human keeps it open");

    // Asked again, it is refused: the task is done. A turn its job's end wakes later
    // answers no task.
    let again = context
        .finish_task(tiers.project.id, 1, "chief")
        .unwrap_err();
    assert_eq!(again.to_string(), "cannot finish T-1: it is done");
    context.adapter.wake("zeus", ENDED);
    context.pass().unwrap();
    context.adapter.answer("zeus", "The server stopped");
    passes(&context, 2);
    assert_eq!(results_of(&tiers, 1).len(), 1, "no second result");
}

#[test]
fn finish_takes_the_last_complete_answer_of_a_task_whose_record_says_its_turn_is_still_on() {
    // T-442's restart (poker-lab, 3.0.4): the reader read the turn at work for good, the
    // answer complete in the record; and any working task, held or not (the owner,
    // 2026-10-18).
    let context = Context::new();
    let tiers = zeus_at_work(&context);
    context.adapter.answer("zeus", "Report: the parser is done");
    context.adapter.with("zeus", |agent| agent.settled = false);
    context.pass().unwrap();
    assert_eq!(
        tiers.task(1).task.state,
        "working",
        "the reader says it works"
    );

    let finished = context
        .finish_task(tiers.project.id, 1, "human")
        .expect("finished");
    assert_eq!(finished.task.state, "done");
    assert_eq!(
        results_of(&tiers, 1),
        ["Report: the parser is done\n\n(ConsensFlow: Finished at the human's word: what @zeus said last is the result. @zeus's turn was still under way, so that is what its record had said so far: a job its turn starts from now on is not counted, nor are the background agents or workflows it left, which its harness counts only as a turn ends.)"]
    );
}

#[test]
fn finish_takes_the_answer_before_a_turn_its_jobs_end_woke_that_has_written_nothing_yet() {
    // T-442's shape: held for its job, whose end woke a turn that has written nothing (or
    // hangs there): the notification is no message of the task's, and the answer before
    // it is the last complete one after the task's latest.
    let context = Context::new();
    let tiers = zeus_at_work(&context);
    context
        .adapter
        .answer_leaving("zeus", "Watching the run", A_JOB);
    context.pass().unwrap();
    context.adapter.wake("zeus", ENDED);
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");
    // Settled with nothing written since the wake: still no result of its own.
    context.adapter.with("zeus", |agent| agent.settled = true);
    passes(&context, 2);
    assert_eq!(tiers.task(1).task.state, "working");
    assert_eq!(results_of(&tiers, 1), Vec::<String>::new());

    let finished = context
        .finish_task(tiers.project.id, 1, "chief")
        .expect("finished");
    assert_eq!(finished.task.state, "done");
    let results = results_of(&tiers, 1);
    assert!(
        results[0].starts_with("Watching the run\n\n(ConsensFlow: Finished at @chief's word"),
        "{results:?}"
    );
}

#[test]
fn finish_of_a_turn_still_under_way_names_the_jobs_its_record_counts_and_says_the_count_may_be_short(
) {
    // T-442's shape, finished mid-turn (Astraeus on 0eec91d2): held for its command and its
    // monitor, the monitor's event woke a turn that is still under way, and the chief
    // finishes the task. Its record counts both jobs still running.
    const TWO_JOBS: Pending = Pending {
        agents: 0,
        jobs: 2,
        retired: 0,
    };
    let context = Context::new();
    let tiers = zeus_at_work(&context);
    context
        .adapter
        .answer_leaving("zeus", "Watching the run", TWO_JOBS);
    context.pass().unwrap();
    context.adapter.wake(
        "zeus",
        "<task-notification>\n<task-id>m1</task-id>\n<summary>Monitor event: \"ticks\"</summary>\n<event>tick 1</event>\n</task-notification>",
    );
    context
        .adapter
        .with("zeus", |agent| agent.pending = TWO_JOBS);
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");

    let finished = context
        .finish_task(tiers.project.id, 1, "chief")
        .expect("finished");
    assert_eq!((finished.jobs, finished.agents), (2, 0));
    let results = results_of(&tiers, 1);
    assert!(
        results[0].starts_with("Watching the run\n\n(ConsensFlow: Finished at @chief's word while @zeus's harness still counted 2 background jobs running"),
        "{results:?}"
    );
    assert!(
        results[0].contains(
            "@zeus's turn was still under way, so that is what its record had said so far"
        ),
        "{results:?}"
    );
}

#[test]
fn finish_is_refused_where_no_complete_answer_follows_the_latest_message_of_the_task() {
    let context = Context::new();
    let tiers = zeus_at_work(&context);
    // At work, its words so far a call: nothing complete.
    context.adapter.busy("zeus");
    context.pass().unwrap();
    let refused = context
        .finish_task(tiers.project.id, 1, "chief")
        .unwrap_err();
    assert!(
        refused
            .to_string()
            .starts_with("cannot finish T-1: @zeus has written no complete answer since its latest message about it"),
        "{refused}"
    );
    assert_eq!(tiers.task(1).task.state, "working");

    // An answer before the latest message is no answer to it: held, then paused and
    // resumed with words, the window has written nothing since.
    context
        .adapter
        .answer_leaving("zeus", "Started the run", A_JOB);
    context.pass().unwrap();
    context.pause_task(tiers.project.id, 1);
    passes(&context, 2);
    context.resume_task(tiers.project.id, 1, "Go on with the run");
    passes(&context, 2);
    context.adapter.busy("zeus");
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");
    let refused = context
        .finish_task(tiers.project.id, 1, "chief")
        .unwrap_err();
    assert!(
        refused.to_string().contains("no complete answer"),
        "{refused}"
    );
    assert_eq!(results_of(&tiers, 1), Vec::<String>::new());
}

#[test]
fn finish_is_refused_for_a_task_that_is_not_working_or_that_no_open_window_of_a_member_works_on() {
    let context = Context::new();
    let tiers = zeus_at_work(&context);
    docs_for_zeus(&context, &tiers);
    let refused = context
        .finish_task(tiers.project.id, 2, "chief")
        .unwrap_err();
    assert_eq!(refused.to_string(), "cannot finish T-2: it is queued");
    let refused = context
        .finish_task(tiers.project.id, 9, "chief")
        .unwrap_err();
    assert_eq!(refused.to_string(), "no task T-9 in this project");
    context.pause_task(tiers.project.id, 1);
    let refused = context
        .finish_task(tiers.project.id, 1, "chief")
        .unwrap_err();
    assert_eq!(refused.to_string(), "cannot finish T-1: it is paused");
}

/// How T-1 is released from its window, with its job and its agent still running.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Release {
    /// Its own result: a turn that ended with nothing its harness waits on, a process the
    /// host lists left in the window.
    Result,
    Finish,
    Cancel,
    Failure,
    /// The worker's own `cf task done`.
    Done,
    Reassign,
}

/// The releases a task given by name can have: none is given back to the board.
const NAMED: [Release; 5] = [
    Release::Result,
    Release::Finish,
    Release::Cancel,
    Release::Failure,
    Release::Done,
];

/// T-1 released from zeus's window by `how`, with what its turn left still running.
fn release(context: &Context, tiers: &Tiers<'_>, how: Release) {
    let project = tiers.project.id;
    match how {
        Release::Result => {
            context
                .adapter
                .answer("zeus", "Recorded, and the run goes on");
            context.pass().unwrap();
        }
        _ => {
            context.adapter.answer_leaving(
                "zeus",
                "Started the run and two readers",
                A_JOB_AND_AN_AGENT,
            );
            context.pass().unwrap();
            assert_eq!(tiers.task(1).task.state, "working", "{how:?}: held");
        }
    }
    match how {
        Release::Result => {}
        Release::Finish => {
            context.finish_task(project, 1, "chief").unwrap();
        }
        Release::Cancel => {
            context
                .ledger
                .borrow_mut()
                .cancel_task(project, 1, "chief")
                .unwrap();
        }
        Release::Failure => {
            context.adapter.wake("zeus", ENDED);
            context.pass().unwrap();
            context.adapter.with("zeus", |agent| agent.failed = true);
            context
                .adapter
                .answer_leaving("zeus", "It broke", A_JOB_AND_AN_AGENT);
            context.pass().unwrap();
            assert_eq!(tiers.task(1).task.state, "failed", "{how:?}");
            context.adapter.with("zeus", |agent| agent.failed = false);
        }
        Release::Done => {
            context
                .ledger
                .borrow_mut()
                .record_result(project, 1, "Done, says the worker")
                .unwrap();
        }
        Release::Reassign => {
            context.reassign_task(project, 1).unwrap();
        }
    }
    assert_ne!(
        tiers.task(1).task.state,
        "working",
        "{how:?}: released from the window"
    );
}

#[test]
fn no_other_task_goes_into_a_window_an_earlier_task_left_work_in_whatever_released_it() {
    // Both reviews of round 7: a cancel let T-2 into T-1's window, and the end of T-1's job
    // woke a turn there whose words were taken for T-2's result.
    for how in NAMED {
        let context = Context::new();
        let tiers = zeus_at_work(&context);
        let old = context.host.last("zeus").unwrap().pane;
        docs_for_zeus(&context, &tiers);
        release(&context, &tiers, how);
        passes(&context, 4);

        assert!(closed(&context, &old), "{how:?}: T-1's window closed first");
        let new = context.host.last("zeus").unwrap().pane;
        assert_ne!(new, old, "{how:?}: T-2 has a window of its own");
        assert_eq!(tiers.task(2).task.state, "working", "{how:?}");
        let brief = context
            .adapter
            .prepared()
            .last()
            .and_then(|launch| launch["message"].as_str().map(str::to_owned))
            .unwrap_or_default();
        assert!(brief.contains("Write the docs"), "{how:?}: {brief}");
        context.adapter.answer("zeus", "Docs written");
        passes(&context, 2);
        assert_eq!(results_of(&tiers, 2), ["Docs written"], "{how:?}");
    }
}

/// T-1 for the standard workers, at work in the window of zeus's session, which the human
/// opened and keeps open; the host proves each window's program gone. The session's handle.
fn session_kept_open(context: &Context) -> (Tiers<'_>, String) {
    context.host.exits_showing(Some(0), None, None);
    let tiers = Tiers::new(context, &["zeus", "diana"]);
    tiers.open();
    passes(context, 2);
    let session = tiers.assignee(1);
    context.open_window(tiers.project.id, &session).unwrap();
    assert_eq!(tiers.task(1).task.state, "working");
    (tiers, session)
}

/// T-2, the human's follow-up of T-1, for the session that did it.
fn follow_up(context: &Context, tiers: &Tiers<'_>) {
    context.create_task(
        tiers.project.id,
        NewTask {
            from: "human".to_owned(),
            after: Some(1),
            body: "Write the docs".to_owned(),
            ..NewTask::default()
        },
    );
}

#[test]
fn a_window_the_human_keeps_open_keeps_the_next_task_waiting_and_what_wakes_in_it_is_no_ones_result(
) {
    for how in NAMED {
        let context = Context::new();
        let (tiers, session) = session_kept_open(&context);
        let old = context.host.last(&session).unwrap().pane;
        release(&context, &tiers, how);
        follow_up(&context, &tiers);
        passes(&context, 3);

        assert!(!closed(&context, &old), "{how:?}: the human keeps it open");
        assert_eq!(tiers.task(2).task.state, "queued", "{how:?}: T-2 waits");
        let waiting: Vec<String> = tiers
            .notes("human")
            .into_iter()
            .filter(|note| note.starts_with("T-2 waits for a window of its own"))
            .collect();
        assert_eq!(waiting.len(), 1, "{how:?}: told once: {waiting:?}");
        assert!(
            waiting[0].contains("the human keeps it open"),
            "{waiting:?}"
        );

        // T-1's job ends and wakes its agent in the old window: its words are no one's.
        let before = results_of(&tiers, 1);
        context.adapter.wake("zeus", ENDED);
        context.pass().unwrap();
        context
            .adapter
            .answer("zeus", "The run finished: 3 files recorded");
        passes(&context, 3);
        assert_eq!(results_of(&tiers, 1), before, "{how:?}");
        assert_eq!(results_of(&tiers, 2), Vec::<String>::new(), "{how:?}");
        assert_eq!(tiers.task(2).task.state, "queued", "{how:?}");
        assert_eq!(
            tiers
                .notes("human")
                .iter()
                .filter(|note| note.starts_with("T-2 waits"))
                .count(),
            1,
            "{how:?}: told once"
        );

        // Hidden, it closes, and T-2 goes into a window of its own.
        context.hide_window(tiers.project.id, &session).unwrap();
        passes(&context, 4);
        assert!(closed(&context, &old), "{how:?}");
        assert_eq!(tiers.task(2).task.state, "working", "{how:?}");
        context.adapter.answer("zeus", "Docs written");
        passes(&context, 2);
        assert_eq!(results_of(&tiers, 2), ["Docs written"], "{how:?}");
    }
}

#[test]
fn a_task_reassigned_from_a_window_its_work_runs_in_is_answered_in_its_new_window_alone() {
    // Reassigned, T-1's window is stopped first, even the one the human keeps open; the
    // task goes to a window of another session, whose answer alone is its result.
    let context = Context::new();
    let (tiers, session) = session_kept_open(&context);
    let old = context.host.last(&session).unwrap().pane;
    release(&context, &tiers, Release::Reassign);
    assert!(
        closed(&context, &old),
        "stopped, though the human kept it open"
    );
    passes(&context, 3);
    let moved = tiers.assignee(1);
    assert_ne!(moved, session, "another session has it");
    let new = context.host.last(&moved).unwrap().pane;
    assert_ne!(new, old);
    assert_eq!(tiers.task(1).task.state, "working");
    context
        .adapter
        .answer("diana", "Recorded again, from the start");
    passes(&context, 2);
    assert_eq!(results_of(&tiers, 1), ["Recorded again, from the start"]);
}

#[test]
fn a_window_that_will_not_close_keeps_the_next_task_waiting() {
    let context = Context::new();
    let tiers = zeus_at_work(&context);
    docs_for_zeus(&context, &tiers);
    context.host.refuse_kills.set(true);
    release(&context, &tiers, Release::Cancel);
    passes(&context, 3);
    assert_eq!(tiers.task(2).task.state, "queued");
    let waiting: Vec<String> = tiers
        .notes("human")
        .into_iter()
        .filter(|note| note.starts_with("T-2 waits"))
        .collect();
    assert_eq!(waiting.len(), 1, "{waiting:?}");
    assert!(waiting[0].contains("it would not close"), "{waiting:?}");

    context.host.refuse_kills.set(false);
    passes(&context, 4);
    assert_eq!(tiers.task(2).task.state, "working");
}

#[test]
fn a_task_in_another_sessions_hands_is_not_said_to_be_in_the_hands_of_the_window_it_left() {
    // Calliope on 0eec91d2: whether the window's task is still in its session's hands is told
    // by whom the task is given to, not by its state alone. T-1 is released from the window
    // the human keeps open (the ledger's release, as a reassign makes it) and another session
    // takes it; T-2, for the first session, waits for the window, and its requester is told
    // it is the human's to let go, not that T-1 is still in its hands.
    let context = Context::new();
    let (tiers, session) = session_kept_open(&context);
    context
        .ledger
        .borrow_mut()
        .release_task(tiers.project.id, 1, "by @human")
        .unwrap();
    context.adapter.answer("zeus", "Stopped where I was");
    passes(&context, 3);
    assert_ne!(tiers.assignee(1), session, "another session has T-1");
    assert_eq!(tiers.task(1).task.state, "working");
    context.create_task(
        tiers.project.id,
        NewTask {
            from: "human".to_owned(),
            to: Some(session.clone()),
            body: "Write the docs".to_owned(),
            ..NewTask::default()
        },
    );
    passes(&context, 3);
    assert_eq!(tiers.task(2).task.state, "queued");
    let waiting: Vec<String> = tiers
        .notes("human")
        .into_iter()
        .filter(|note| note.starts_with("T-2 waits"))
        .collect();
    assert_eq!(waiting.len(), 1, "{waiting:?}");
    assert!(!waiting[0].contains("still in its hands"), "{waiting:?}");
    assert!(
        waiting[0].contains("the human keeps it open"),
        "{waiting:?}"
    );
}

#[test]
fn the_next_task_goes_on_in_a_new_conversation_where_the_window_before_closed_with_its_program_alive(
) {
    let context = Context::new();
    let tiers = zeus_at_work(&context);
    let conversation = context.adapter.prepared().last().unwrap()["resume"].clone();
    docs_for_zeus(&context, &tiers);
    // The close could not prove the window's program gone: it may still write to the
    // conversation.
    context.host.exits_with_a_survivor(4242);
    release(&context, &tiers, Release::Cancel);
    passes(&context, 4);
    assert_eq!(tiers.task(2).task.state, "working");
    let launch = context.adapter.prepared().last().unwrap().clone();
    assert_eq!(
        launch["resume"],
        serde_json::Value::Null,
        "a new conversation"
    );
    let _ = conversation;
    let told: Vec<String> = tiers
        .notes("human")
        .into_iter()
        .filter(|note| note.starts_with("T-2 goes to @zeus in a new conversation"))
        .collect();
    assert_eq!(told.len(), 1, "{told:?}");
}

#[test]
fn after_a_restart_another_task_goes_on_in_a_new_conversation_and_the_same_task_in_its_own() {
    let context = Context::new();
    let (tiers, _session) = session_kept_open(&context);
    release(&context, &tiers, Release::Finish);
    follow_up(&context, &tiers);
    passes(&context, 2);
    assert_eq!(
        tiers.task(2).task.state,
        "queued",
        "the human keeps T-1's window"
    );

    // The app quits: this daemon never saw that window close.
    context.ledger.borrow_mut().suspend_for_restart().unwrap();
    let after = context.make();
    after.settle_after_restart().unwrap();
    after.resume_project(tiers.project.id).unwrap();
    for _ in 0..4 {
        after.pass().unwrap();
    }
    assert_eq!(tiers.task(2).task.state, "working");
    let launch = context.adapter.prepared().last().unwrap().clone();
    assert!(
        launch["message"]
            .as_str()
            .unwrap_or_default()
            .contains("Write the docs"),
        "{launch}"
    );
    assert_eq!(
        launch["resume"],
        serde_json::Value::Null,
        "a new conversation"
    );
}

#[test]
fn a_task_held_for_its_monitor_when_the_app_quit_is_held_again_after_the_restart_and_told_how_to_finish_it(
) {
    // Calliope on 0eec91d2: the app quits while T-1 is held for its monitor. This daemon
    // never saw the window close, so nothing proves its program gone, and the monitor's
    // bookkeeping is not retired (the decision forbids taking the quit for the window's
    // exit): the window reopened on the same task reads the monitor pending still, the task
    // is held until Finish, and its requester is told so again.
    let context = Context::new();
    let tiers = zeus_at_work(&context);
    context
        .adapter
        .answer_leaving("zeus", "Watching the run", A_JOB);
    context.pass().unwrap();
    assert_eq!(tiers.notes("chief").len(), 1);
    // The conversation's record, which the window opened again reads as it was: the stand-in
    // keeps a record a window, and is given it back.
    let record = context.adapter.with("zeus", |agent| agent.items.clone());

    context.ledger.borrow_mut().suspend_for_restart().unwrap();
    let after = context.make();
    after.settle_after_restart().unwrap();
    after.resume_project(tiers.project.id).unwrap();
    after.pass().unwrap();
    context.adapter.with("zeus", |agent| {
        agent.items = record;
        agent.pending = A_JOB;
        agent.settled = true;
    });
    for _ in 0..3 {
        after.pass().unwrap();
    }
    assert_eq!(tiers.task(1).task.state, "working", "held for its monitor");
    assert_eq!(results_of(&tiers, 1), Vec::<String>::new());
    let told: Vec<String> = tiers
        .notes("chief")
        .into_iter()
        .filter(|note| note.starts_with("T-1 goes on in the background"))
        .collect();
    assert_eq!(told.len(), 2, "told again after the restart: {told:?}");
    assert!(told[1].contains("cf task finish T-1"), "{told:?}");

    let finished = after
        .finish_task(tiers.project.id, 1, "chief")
        .expect("finished");
    assert_eq!(finished.task.state, "done");
    assert_eq!(finished.jobs, 1);
}

#[test]
fn a_paused_task_keeps_its_window_and_another_task_waits_for_it() {
    // Astra's review of round 7: reopened and paused, the task's window was closed with its
    // work, as the look at the task in hand took no paused one.
    let context = Context::new();
    let tiers = zeus_at_work(&context);
    let old = context.host.last("zeus").unwrap().pane;
    context
        .adapter
        .answer_leaving("zeus", "Started the run", A_JOB);
    context.pass().unwrap();
    context.pause_task(tiers.project.id, 1);
    passes(&context, 2);
    docs_for_zeus(&context, &tiers);
    passes(&context, 3);
    assert!(
        !closed(&context, &old),
        "the window stays for T-1's resumption"
    );
    assert_eq!(tiers.task(2).task.state, "queued");
    let waiting: Vec<String> = tiers
        .notes("human")
        .into_iter()
        .filter(|note| note.starts_with("T-2 waits"))
        .collect();
    assert_eq!(waiting.len(), 1, "{waiting:?}");
    assert!(
        waiting[0].contains("T-1 is still in its hands (paused)"),
        "{waiting:?}"
    );

    // Resumed, T-1 goes on in the same window, its job held as before.
    context.resume_task(tiers.project.id, 1, "Go on");
    passes(&context, 2);
    assert_eq!(context.host.last("zeus").unwrap().pane, old);
    assert_eq!(tiers.task(1).task.state, "working");
}

#[test]
fn an_answer_the_record_holds_is_its_result_when_the_window_goes_by_itself_right_after() {
    // Sol's review of round 7: the window's program ended while the host was looked at, and
    // the answer already seen was lost. The answer is recorded as it is seen.
    let context = Context::new();
    let tiers = zeus_at_work(&context);
    context.adapter.answer("zeus", "Parser done");
    context.pass().unwrap();
    context.exit("zeus");
    passes(&context, 2);
    assert_eq!(tiers.task(1).task.state, "done");
    assert_eq!(results_of(&tiers, 1), ["Parser done"]);
}

/// What the ledger keeps of where the words that resume T-1 arrived.
#[derive(Debug, Clone, Copy)]
enum Receipt {
    /// The item of the record that is their paste.
    Kept,
    /// Nothing, as Node's ledger left a receipt.
    Missing,
    /// An item the record does not hold (the record lost it).
    ItemMissing,
}

/// T-1, held for its job, paused and resumed with words that arrive in its window, the
/// ledger keeping `receipt` of where. The human had asked about the words while they were
/// queued, quoting their header, and was answered, before the words arrived; the window
/// settles with nothing written since the words. With `pasted` false, the record holds no
/// paste of them either: what arrived is the quotation alone, and a reply to it.
fn quoted_before_the_paste(context: &Context, receipt: Receipt, pasted: bool) -> Tiers<'_> {
    let tiers = zeus_at_work(context);
    context
        .adapter
        .answer_leaving("zeus", "Started the run", A_JOB);
    context.pass().unwrap();
    context.pause_task(tiers.project.id, 1);
    passes(context, 2);
    context.resume_task(tiers.project.id, 1, "Go on with the run");
    let words = context
        .inbox(tiers.id("zeus"))
        .into_iter()
        .find(|message| message.body.contains("Go on with the run"))
        .expect("the words that resume it");
    passes(context, 2);
    assert_eq!(context.message(words.id).state, "delivered");
    let marker = marker_of(words.id);
    let quoted = context
        .adapter
        .item(Role::User, &format!("Why is {marker}T-1 still queued?"));
    let answered = context
        .adapter
        .item(Role::Assistant, "It waits for the window to be free");
    context.adapter.with("zeus", |agent| {
        let at = agent
            .items
            .iter()
            .position(|item| item.role == Role::User && item.text.contains(&marker))
            .expect("the paste of the words");
        if matches!(receipt, Receipt::ItemMissing) {
            agent.items[at].id = "an-item-the-record-lost".into();
        }
        if !pasted {
            agent.items.remove(at);
        }
        agent.items.insert(at, answered);
        agent.items.insert(at, quoted);
        agent.settled = true;
    });
    if matches!(receipt, Receipt::Missing) {
        context
            .ledger
            .borrow_mut()
            .forget_receipt(words.id)
            .unwrap();
    }
    passes(context, 2);
    tiers
}

#[test]
fn the_result_is_what_follows_the_paste_its_arrival_was_proved_by_never_a_quotation_of_its_header()
{
    // A quotation of the latest message's header in the window before it arrived, and an
    // answer after that quotation: the answer is to no message of the task's latest, and
    // no result, whether the ledger names the item the words arrived as (its receipt), or
    // names none (Node's ledger), or names one the record lost (Astraeus on 0eec91d2: the
    // quotation was then taken for the delivery, and Finish took the answer to it).
    for receipt in [Receipt::Kept, Receipt::Missing, Receipt::ItemMissing] {
        let context = Context::new();
        let tiers = quoted_before_the_paste(&context, receipt, true);
        assert_eq!(tiers.task(1).task.state, "working", "{receipt:?}");
        assert_eq!(results_of(&tiers, 1), Vec::<String>::new(), "{receipt:?}");
        let refused = context
            .finish_task(tiers.project.id, 1, "chief")
            .unwrap_err();
        assert!(
            refused.to_string().contains("no complete answer"),
            "{receipt:?}: {refused}"
        );
    }
}

#[test]
fn where_no_paste_proves_where_the_latest_message_arrived_there_is_no_result_and_finish_says_why() {
    // No receipt, and no paste in the record: only the quotation holds the header. Nothing
    // proves where the words arrived, so nothing is an answer to them.
    let context = Context::new();
    let tiers = quoted_before_the_paste(&context, Receipt::Missing, false);
    assert_eq!(tiers.task(1).task.state, "working");
    assert_eq!(results_of(&tiers, 1), Vec::<String>::new());
    let refused = context
        .finish_task(tiers.project.id, 1, "chief")
        .unwrap_err();
    assert!(
        refused
            .to_string()
            .contains("holds no proof of where its latest message"),
        "{refused}"
    );
}
