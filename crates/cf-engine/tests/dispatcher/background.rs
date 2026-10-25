//! A worker's turn that ends with its answer is its task's result at once, and **its
//! end ends nothing it left running** (turn-end's smallest rule, the chief's decision of
//! 2026-10-21; until then what the turn left was ended with the result, which ended
//! poker-lab's T-442 test run while its worker waited on it with Monitor). Work its
//! harness wakes the agent for keeps the task working (`turn_end`, `pending_agents`);
//! the rest goes on until the window closes. A free worker's window closes after its
//! result, as any does, and the close ends what the host can tell is the window's, which
//! it lists: whoever gave the window's task is told what. Node's dispatcher never asked
//! the host, so none of these is held to a Node trace: the engine's asks of the host
//! about it are not in the traces at all.

use crate::fixtures::{closed, given, Tiers};
use cf_engine::testing::Context;
use cf_ledger::NewTask;
use cf_proto::panes::Servers;

pub(crate) const LOCK: &str = "python3 lock.py record --run 7";
pub(crate) const LOG: &str = "tail -f run.log";

/// What the requester is told when the window of T-1 closed with `LOCK` running.
const LOCK_CLOSED: &str =
    "T-1's window closed with background work running, which its close ended: python3 lock.py record --run 7";

/// T-1 given to zeus, whose agent starts a run, answers that it did and ends its
/// turn: the pass that looks at it is made.
pub(crate) fn answered_leaving(context: &Context) -> Tiers<'_> {
    let tiers = working(context);
    context.adapter.answer("zeus", "Started the recording run");
    context.pass().unwrap();
    tiers
}

/// T-1 given to zeus, whose window has taken it in.
pub(crate) fn working(context: &Context) -> Tiers<'_> {
    let tiers = Tiers::new(context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");
    tiers
}

/// The results the chief was given or is to be given.
pub(crate) fn results(context: &Context, tiers: &Tiers<'_>) -> Vec<String> {
    context
        .inbox(tiers.id("chief"))
        .into_iter()
        .filter(|message| message.kind == "result")
        .map(|message| message.body)
        .collect()
}

#[test]
fn a_turn_that_ends_with_the_work_it_started_still_running_is_its_tasks_result_at_once() {
    let context = Context::new();
    let tiers = answered_leaving(&context);

    assert_eq!(
        tiers.task(1).task.state,
        "done",
        "no hold: its answer is it"
    );
    assert_eq!(results(&context, &tiers), ["Started the recording run"]);
}

#[test]
fn nothing_the_turn_left_is_ended_and_the_host_is_never_asked_to_end_it() {
    // T-442's shape (poker-lab, 3.0.4): the worker's turn left a run going, and its end
    // ended it. The window's work goes on, and nothing is said of it beside the result.
    let context = Context::new();
    let tiers = working(&context);
    let session = tiers.assignee(1);
    context.open_window(tiers.project.id, &session).unwrap();
    context.adapter.answer("zeus", "Started the recording run");
    context.pass().unwrap();
    context.pass().unwrap();

    assert_eq!(tiers.task(1).task.state, "done");
    assert_eq!(results(&context, &tiers), ["Started the recording run"]);
    assert_eq!(tiers.notes("chief"), Vec::<String>::new());
}

#[test]
fn a_turn_that_failed_is_the_tasks_failure_and_nothing_is_ended_for_it() {
    let context = Context::new();
    let tiers = working(&context);
    // The harness reports the turn failed, with the job still running.
    context.adapter.with("zeus", |agent| agent.failed = true);
    context.adapter.answer("zeus", "Started the recording run");
    context.pass().unwrap();

    assert_eq!(tiers.task(1).task.state, "failed");
    assert_eq!(results(&context, &tiers), Vec::<String>::new());
    let told = tiers.notes("chief");
    assert!(
        told.iter().all(|note| !note.contains("background work")),
        "{told:?}"
    );
}

#[test]
fn a_chiefs_window_is_not_touched() {
    // The chief's turn ends with a job running, and a result of a worker's waits
    // for it: nothing is asked of the host, and nothing is ended.
    let context = Context::new();
    let tiers = working(&context);
    context.adapter.answer("chief", "Started the recording run");
    context.pass().unwrap();
    context.pass().unwrap();

    assert_eq!(tiers.task(1).task.state, "working");
}

#[test]
fn a_window_the_human_keeps_open_stays_open_with_its_work_running() {
    let context = Context::new();
    let tiers = working(&context);
    let session = tiers.assignee(1);
    context.open_window(tiers.project.id, &session).unwrap();
    let pane = context.host.last("zeus").unwrap().pane;
    context.adapter.answer("zeus", "Started the recording run");
    context.pass().unwrap();

    assert_eq!(tiers.task(1).task.state, "done");
    assert!(!closed(&context, &pane), "the human holds it open");
}

#[test]
fn a_free_window_closes_after_its_result_and_its_close_says_what_it_ended() {
    let context = Context::new();
    let tiers = working(&context);
    let pane = context.host.last("zeus").unwrap().pane;
    context.host.exits_leaving(&[LOCK]);
    context.adapter.answer("zeus", "Started the recording run");
    context.pass().unwrap();

    assert!(closed(&context, &pane), "its task is done and it closes");
    assert_eq!(tiers.notes("chief"), [LOCK_CLOSED]);
}

#[test]
fn the_close_note_goes_to_whoever_gave_the_windows_task_not_to_a_task_queued_since() {
    // Astra's review of round 7: the note went to the newest message's task, a brief
    // still queued, and its requester was told of work it never had.
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    let project = tiers.project.id;
    context.give(project, "zeus", "Record the run");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");
    // A second task for the same member, from the human: it waits for the first.
    context.create_task(
        project,
        NewTask {
            from: "human".to_owned(),
            to: Some("zeus".to_owned()),
            body: "Docs".to_owned(),
            ..NewTask::default()
        },
    );
    context.pass().unwrap();
    assert_eq!(tiers.task(2).task.state, "queued");
    // T-1 is done; its window closes before T-2 goes in, with T-1's work running.
    context.host.exits_leaving(&[LOCK]);
    context.adapter.answer("zeus", "Started the recording run");
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(tiers.task(1).task.state, "done");
    let chief = tiers.notes("chief");
    assert!(chief.contains(&LOCK_CLOSED.to_owned()), "{chief:?}");
    let human = tiers.notes("human");
    assert!(
        human.iter().all(|note| !note.contains("background work")),
        "{human:?}"
    );
}

#[test]
fn a_turn_a_woken_agent_runs_after_the_task_is_done_is_no_result_and_breaks_nothing() {
    // What happens to an agent that wakes in a window the human keeps open once its
    // task is done: its turn is the window's own, and no result for a task that
    // is no longer working.
    let context = Context::new();
    let tiers = working(&context);
    context
        .open_window(tiers.project.id, &tiers.assignee(1))
        .unwrap();
    context.adapter.answer("zeus", "Started the recording run");
    context.pass().unwrap();
    let told = tiers.notes("chief");
    assert_eq!(tiers.task(1).task.state, "done");

    // Its job ended, the harness wakes it: a turn of its own, that ends with an answer.
    context.adapter.busy("zeus");
    context.pass().unwrap();
    context
        .adapter
        .answer("zeus", "The recording finished: 3 files");
    context.pass().unwrap();
    context.pass().unwrap();

    assert_eq!(tiers.task(1).task.state, "done");
    assert_eq!(results(&context, &tiers), ["Started the recording run"]);
    assert_eq!(tiers.notes("chief"), told, "nothing more is said");
}

#[test]
fn marks_the_window_where_a_turn_begins_and_again_as_each_message_goes_in() {
    let context = Context::new();
    let tiers = working(&context);
    let pane = context.host.last("zeus").unwrap().pane;
    assert_eq!(
        context.background.marked(),
        vec![pane.clone()],
        "once: when the first message showed in the window's record"
    );

    // The agent is at work on its turn, and a word for it waits until it is
    // not: delivered, it begins a turn of its own.
    context.adapter.busy("zeus");
    context.tell(tiers.project.id, &tiers.assignee(1), 1, "Use grammar v2");
    context.pass().unwrap();
    context.adapter.with("zeus", |agent| agent.settled = true);
    context.pass().unwrap();
    context.pass().unwrap();

    assert_eq!(context.background.marked(), [pane.clone(), pane]);
    assert_eq!(given(&context, "zeus").len(), 2, "the tell was delivered");
}

#[test]
fn each_mark_says_how_the_harness_keeps_its_servers_and_which_harness_it_is() {
    // What the host tells the harness's own from the turn's by is the adapter's word
    // and the harness's name, which the engine says as the turn begins and no more.
    let context = Context::new();
    context.adapter.servers.set(Servers::InGroup);
    let _tiers = working(&context);

    assert_eq!(context.background.servers_said(), [Servers::InGroup]);
    assert_eq!(
        context.background.harness_said(),
        ["claude"],
        "the name its programs go by, not the roster's kind"
    );
}

#[test]
fn a_cancelled_task_closes_its_window_with_the_work_still_running_and_whoever_gave_the_task_is_told(
) {
    let context = Context::new();
    let tiers = working(&context);
    let pane = context.host.last("zeus").unwrap().pane;
    // The host ends the window's programs as it closes it, and lists them.
    context.host.exits_leaving(&[LOCK, LOG]);

    context
        .ledger
        .borrow_mut()
        .cancel_task(tiers.project.id, 1, "human")
        .unwrap();
    context.pass().unwrap();
    context.pass().unwrap();

    assert!(closed(&context, &pane), "cancelling closes the window");
    let told = tiers.notes("chief");
    let said = format!(
        "T-1's window closed with background work running, which its close ended: {LOCK}; {LOG}"
    );
    assert!(told.contains(&said), "{told:?}");
}

#[test]
fn a_window_closed_after_its_result_with_work_the_host_did_not_end_says_so_too() {
    // The ending finds nothing running; the host finds a job when it closes the
    // window, one that began in between (or one the ending could not end).
    let context = Context::new();
    let tiers = working(&context);
    context.host.exits_leaving(&[LOCK]);
    context.adapter.answer("zeus", "Parser done");
    context.pass().unwrap();

    assert_eq!(tiers.task(1).task.state, "done");
    assert!(
        tiers.notes("chief").contains(&format!(
            "T-1's window closed with background work running, which its close ended: {LOCK}"
        )),
        "{:?}",
        tiers.notes("chief")
    );
}

#[test]
fn a_window_that_goes_by_itself_with_the_work_running_says_so_in_the_pause_note() {
    let context = Context::new();
    let tiers = working(&context);
    context.host.exits_leaving(&[LOCK]);

    context.exit("zeus");

    assert_eq!(tiers.task(1).task.state, "paused");
    let session = tiers.assignee(1);
    assert_eq!(
        tiers.notes("chief").last().map(String::as_str),
        Some(
            format!(
                "T-1 is paused: @{session}'s window closed with background work running, which its close ended: {LOCK}. Resume it with: cf task resume T-1 \"…\"; its window comes back on its own conversation."
            )
            .as_str()
        )
    );
}

#[test]
fn a_window_closed_with_a_process_that_may_be_its_own_names_it_to_whoever_gave_the_task() {
    // The host names a process it did not end that may be the window's (a server a tool
    // shell detached): its number and its program, and nothing proved it the window's.
    let context = Context::new();
    let tiers = working(&context);
    context.host.exits_possibly_leaving(&["node"]);

    context
        .ledger
        .borrow_mut()
        .cancel_task(tiers.project.id, 1, "human")
        .unwrap();
    context.pass().unwrap();
    context.pass().unwrap();

    let told = tiers.notes("chief");
    assert!(
        told.contains(
            &"T-1's window closed; pid 4001 (node) may be this window's and was not ended"
                .to_owned()
        ),
        "{told:?}"
    );
}

#[test]
fn a_window_that_goes_by_itself_names_in_its_pause_note_what_may_be_its_own_after_its_work() {
    let context = Context::new();
    let tiers = working(&context);
    context.host.exits_leaving(&[LOCK]);
    context.host.exits_possibly_leaving(&["node", "Python"]);

    context.exit("zeus");

    let session = tiers.assignee(1);
    assert_eq!(
        tiers.notes("chief").last().map(String::as_str),
        Some(
            format!(
                "T-1 is paused: @{session}'s window closed with background work running, which its close ended: {LOCK}; pid 4001 (node), pid 4002 (Python) may be this window's and were not ended. Resume it with: cf task resume T-1 \"…\"; its window comes back on its own conversation."
            )
            .as_str()
        )
    );
}

#[test]
fn a_chiefs_window_that_closes_tells_the_human_what_it_left_and_what_may_be_its_own() {
    // The chief's window has no task whose giver a note could tell: the human is told, in the
    // words a worker's close says it in (Calliope's review T-421).
    let context = Context::new();
    let tiers = working(&context);
    context.host.exits_leaving(&[LOCK]);
    context.host.exits_possibly_leaving(&["node"]);

    context.exit("chief");

    let told = tiers.notes("human");
    assert!(
        told.contains(&format!(
            "@chief's window closed with background work running, which its close ended: {LOCK}; pid 4001 (node) may be this window's and was not ended"
        )),
        "{told:?}"
    );
}

#[test]
fn a_chiefs_window_that_closes_with_nothing_left_tells_the_human_nothing_of_it() {
    let context = Context::new();
    let tiers = working(&context);

    context.exit("chief");

    let told = tiers.notes("human");
    assert!(
        told.iter().all(|note| !note.contains("background work") && !note.contains("may be this window's")),
        "{told:?}"
    );
}

#[test]
fn a_window_that_closes_with_nothing_running_says_nothing_of_it() {
    let context = Context::new();
    let tiers = working(&context);

    context.exit("zeus");

    let told = tiers.notes("chief");
    assert!(
        told.iter().all(|note| !note.contains("background work")),
        "{told:?}"
    );
}

#[test]
fn what_a_close_note_quotes_of_the_work_has_its_secrets_hidden_and_is_held_to_a_line() {
    let context = Context::new();
    let tiers = working(&context);
    let secret = "curl -H 'Authorization: Bearer sk-live-0123456789abcdefABCDEF' https://example.test/upload";
    context.host.exits_leaving(&[secret]);

    context
        .ledger
        .borrow_mut()
        .cancel_task(tiers.project.id, 1, "human")
        .unwrap();
    context.pass().unwrap();
    context.pass().unwrap();

    let told = tiers.notes("chief");
    let note = told
        .iter()
        .find(|note| note.contains("background work running, which its close ended"))
        .unwrap_or_else(|| panic!("{told:?}"));
    assert!(!note.contains("sk-live-0123456789abcdefABCDEF"), "{note}");
    assert!(note.contains("[masked]"), "{note}");
    assert!(
        note.contains("(1 key- or token-like string masked)"),
        "{note}"
    );
}
