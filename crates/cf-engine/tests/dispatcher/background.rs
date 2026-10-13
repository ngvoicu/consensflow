//! A worker's turn that ends with its answer is its task's result at once,
//! whatever it left running, and what it left running is ended (the owner's
//! decision of 2026-10-13, after T-7 and T-15 on Windows: a headless Edge check the
//! harness moved to the background, which never exits, kept a finished worker's
//! task working, with no result for the chief). The pane host says what each
//! window's tree runs that began after its turn did; as the result is recorded the
//! engine asks it to end what is not the harness's own, the window staying open,
//! and whoever gave the task is told, in a note beside the result, what was ended,
//! what could not be and still runs, and that the turn left agents. A window that
//! closes with work running says so to whoever gave its task. Node's dispatcher
//! never asked the host, so none of these is held to a Node trace: the engine's
//! asks of the host about it are not in the traces at all.

use std::rc::Rc;

use crate::fixtures::{closed, given, Tiers};
use cf_engine::testing::Context;

pub(crate) const LOCK: &str = "python3 lock.py record --run 7";
pub(crate) const LOG: &str = "tail -f run.log";

/// A program of the harness's own that it started in the turn and keeps for the
/// session, as Codex does its `codex-code-mode-host` the first time its agent
/// runs a command. The kit's workers run on Claude Code: this one is named for it.
const HELPER: &str = "/opt/claude/bin/claude-code-mode-host --serve";

/// What the requester is told when the turn left `LOCK` running and it was ended,
/// and when it left `LOG` too.
const LOCK_ENDED: &str =
    "T-1's turn left background work running. Ended: python3 lock.py record --run 7.";
const BOTH_ENDED: &str = "T-1's turn left background work running. Ended: python3 lock.py record --run 7; tail -f run.log.";

/// T-1 given to zeus, whose agent starts what `commands` say, answers that it
/// did and ends its turn: the pass that looks at it is made.
pub(crate) fn answered_leaving<'a>(context: &'a Context, commands: &[&str]) -> Tiers<'a> {
    let tiers = working(context);
    context.background.run("zeus", commands);
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
    let tiers = answered_leaving(&context, &[LOCK, LOG]);

    assert_eq!(
        tiers.task(1).task.state,
        "done",
        "no hold: its answer is it"
    );
    assert_eq!(results(&context, &tiers), ["Started the recording run"]);
}

#[test]
fn what_the_turn_left_running_is_ended_with_the_result_and_the_requester_is_told_what() {
    let context = Context::new();
    let tiers = answered_leaving(&context, &[LOCK, LOG]);

    assert_eq!(context.background.ended("zeus"), [LOCK, LOG]);
    assert_eq!(tiers.notes("chief"), [BOTH_ENDED]);
}

#[test]
fn the_result_does_not_wait_for_the_work_to_end() {
    // A host gives what it ends the time to release what it holds (ten seconds on
    // Unix): the result is the chief's by then.
    let context = Context::new();
    let tiers = working(&context);
    let ending = context.background.hold_endings();
    context.background.run("zeus", &[LOCK]);
    context.adapter.answer("zeus", "Started the recording run");
    // The pass waits for the host, which waits for the test.
    let dispatcher = Rc::clone(&context.dispatcher);
    let pass = context
        .executor
        .start(async move { dispatcher.pass().await });
    context.settle();

    assert!(!pass.ended(), "the pass waits for what is being ended");
    assert_eq!(tiers.task(1).task.state, "done");
    assert_eq!(results(&context, &tiers), ["Started the recording run"]);
    assert_eq!(context.background.endings_asked("zeus"), 1, "it was asked");
    assert_eq!(
        context.background.ended("zeus"),
        Vec::<String>::new(),
        "and has not ended it yet"
    );
    assert_eq!(tiers.notes("chief"), Vec::<String>::new());

    ending.open();
    context.settle();
    assert!(pass.ended());
    assert_eq!(context.background.ended("zeus"), [LOCK]);
    assert_eq!(tiers.notes("chief"), [LOCK_ENDED]);
}

#[test]
fn a_turn_that_left_nothing_says_nothing_and_asks_nothing_of_the_host() {
    // The host lists only what began after the turn did: a server the harness
    // started with its window is not among it, and the turn's end is the result.
    let context = Context::new();
    let tiers = working(&context);
    context.adapter.answer("zeus", "Parser done");
    context.pass().unwrap();

    assert_eq!(tiers.task(1).task.state, "done");
    assert_eq!(results(&context, &tiers), ["Parser done"]);
    assert_eq!(context.background.asked("zeus"), 1, "it looked");
    assert_eq!(context.background.endings_asked("zeus"), 0);
    assert_eq!(tiers.notes("chief"), Vec::<String>::new());
}

#[test]
fn a_turn_that_failed_is_the_tasks_failure_and_nothing_is_ended_for_it() {
    let context = Context::new();
    let tiers = working(&context);
    context.background.run("zeus", &[LOCK]);
    // The harness reports the turn failed, with the job still running.
    context.adapter.with("zeus", |agent| agent.failed = true);
    context.adapter.answer("zeus", "Started the recording run");
    context.pass().unwrap();

    assert_eq!(tiers.task(1).task.state, "failed");
    assert_eq!(results(&context, &tiers), Vec::<String>::new());
    assert_eq!(context.background.endings_asked("zeus"), 0);
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
    context.background.run("chief", &[LOCK]);
    context.adapter.answer("chief", "Started the recording run");
    context.pass().unwrap();
    context.pass().unwrap();

    assert_eq!(context.background.asked("chief"), 0, "never looked at");
    assert_eq!(context.background.endings_asked("chief"), 0);
    assert_eq!(context.background.ended("chief"), Vec::<String>::new());
    assert_eq!(tiers.task(1).task.state, "working");
}

#[test]
fn a_program_of_the_harnesss_own_is_never_asked_to_end_beside_the_agents_work() {
    // Found by the live test of the gate: a Codex designer's turn ended and its
    // result never came, held for `codex-code-mode-host` for as long as the window
    // lived. It is not the agent's work, and ending it would break the harness.
    let context = Context::new();
    let tiers = answered_leaving(&context, &[HELPER, LOCK]);

    assert_eq!(tiers.task(1).task.state, "done");
    assert_eq!(context.background.ended("zeus"), [LOCK]);
    assert_eq!(tiers.notes("chief"), [LOCK_ENDED]);
}

#[test]
fn where_the_harnesss_own_program_is_all_the_turn_left_nothing_is_asked_or_said() {
    let context = Context::new();
    let tiers = answered_leaving(&context, &[HELPER]);

    assert_eq!(tiers.task(1).task.state, "done");
    assert_eq!(results(&context, &tiers), ["Started the recording run"]);
    assert_eq!(context.background.endings_asked("zeus"), 0);
    assert_eq!(tiers.notes("chief"), Vec::<String>::new());
}

#[test]
fn what_the_host_could_not_end_is_said_to_still_run() {
    // A host that does not know the operation, or that did not answer: nothing
    // was ended, and the requester is not told it was.
    let context = Context::new();
    context.background.end_nothing();
    let tiers = answered_leaving(&context, &[LOCK]);

    assert_eq!(
        tiers.task(1).task.state,
        "done",
        "the result goes all the same"
    );
    assert_eq!(
        tiers.notes("chief"),
        ["T-1's turn left background work running. Still running, not ended: python3 lock.py record --run 7."]
    );
}

#[test]
fn what_a_note_quotes_of_the_work_has_its_secrets_hidden_and_is_held_to_a_line() {
    let context = Context::new();
    let secret = "curl -H 'Authorization: Bearer sk-live-0123456789abcdefABCDEF' https://example.test/upload";
    let tiers = answered_leaving(&context, &[secret]);

    let told = tiers.notes("chief");
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(
        !told[0].contains("sk-live-0123456789abcdefABCDEF"),
        "{}",
        told[0]
    );
    assert!(told[0].contains("[masked]"), "{}", told[0]);
    assert!(
        told[0].ends_with("(1 key- or token-like string masked)."),
        "{}",
        told[0]
    );
}

#[test]
fn a_window_the_human_keeps_open_stays_open_with_its_work_ended() {
    let context = Context::new();
    let tiers = working(&context);
    let session = tiers.assignee(1);
    context.open_window(tiers.project.id, &session).unwrap();
    let pane = context.host.last("zeus").unwrap().pane;
    context.background.run("zeus", &[LOCK]);
    context.adapter.answer("zeus", "Started the recording run");
    context.pass().unwrap();

    assert_eq!(tiers.task(1).task.state, "done");
    assert_eq!(context.background.ended("zeus"), [LOCK]);
    assert!(!closed(&context, &pane), "the human holds it open");
}

#[test]
fn a_window_that_is_not_kept_has_its_work_ended_before_it_closes() {
    let context = Context::new();
    let tiers = working(&context);
    let pane = context.host.last("zeus").unwrap().pane;
    // The close lists what is left when it comes: nothing, for the ending was first.
    context.background.run("zeus", &[LOCK]);
    context.adapter.answer("zeus", "Started the recording run");
    context.pass().unwrap();

    assert!(closed(&context, &pane), "its task is done and it closes");
    assert_eq!(context.background.ended("zeus"), [LOCK]);
    assert_eq!(tiers.notes("chief"), [LOCK_ENDED]);
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
    context.background.run("zeus", &[LOCK]);
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
fn asks_only_a_worker_whose_turn_is_over_with_its_task_working() {
    let context = Context::new();
    let tiers = working(&context);

    // At work on its turn: nothing to ask yet.
    context.adapter.busy("zeus");
    context.pass().unwrap();
    assert_eq!(context.background.asked("zeus"), 0);
    assert_eq!(
        context.background.asked("chief"),
        0,
        "never a chief's window"
    );

    context.adapter.answer("zeus", "Parser done");
    context.pass().unwrap();
    assert_eq!(
        context.background.asked("zeus"),
        1,
        "asked as the result is taken"
    );
    assert_eq!(context.background.asked("chief"), 0);
    assert_eq!(tiers.task(1).task.state, "done");
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
    let said = format!("T-1's window closed with background work still running: {LOCK}; {LOG}");
    assert!(told.contains(&said), "{told:?}");
}

#[test]
fn a_window_closed_after_its_result_with_work_the_host_did_not_end_says_so_too() {
    // The look before the result finds nothing running; the host finds a job when
    // it closes the window, one that began in between (or one the ending could not
    // end).
    let context = Context::new();
    let tiers = working(&context);
    context.host.exits_leaving(&[LOCK]);
    context.adapter.answer("zeus", "Parser done");
    context.pass().unwrap();

    assert_eq!(tiers.task(1).task.state, "done");
    assert!(
        tiers.notes("chief").contains(&format!(
            "T-1's window closed with background work still running: {LOCK}"
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
                "T-1 is paused: @{session}'s window closed with background work still running: {LOCK}. Resume it with: cf task resume T-1 \"…\"; its window comes back on its own conversation."
            )
            .as_str()
        )
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
fn a_window_that_closes_with_only_the_harnesss_helper_left_says_nothing_of_work_running() {
    let context = Context::new();
    let tiers = working(&context);
    // The host finds the harness's helper when it closes the window.
    context.host.exits_leaving(&[HELPER]);
    context.adapter.answer("zeus", "Parser done");
    context.pass().unwrap();

    assert_eq!(tiers.task(1).task.state, "done");
    let told = tiers.notes("chief");
    assert!(
        !told.iter().any(|note| note.contains("background work")),
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
        .find(|note| note.contains("background work still running"))
        .unwrap_or_else(|| panic!("{told:?}"));
    assert!(!note.contains("sk-live-0123456789abcdefABCDEF"), "{note}");
    assert!(note.contains("[masked]"), "{note}");
    assert!(
        note.contains("(1 key- or token-like string masked)"),
        "{note}"
    );
}
