//! A worker's turn that ends with its answer is its task's result at once,
//! whatever it left running, and what it left running is ended (the owner's
//! decision of 2026-10-13, after T-7 and T-15 on Windows: a headless Edge check the
//! harness moved to the background, which never exits, kept a finished worker's
//! task working, with no result for the chief). As the result is recorded the
//! engine asks the pane host to end what the turn left, the window staying open: the
//! host chooses what, from all it can prove the turn's, and the engine takes its
//! word. That is the window's own work, begun apart from the pass and holding the
//! window: whoever gave the task is told, in a note beside the result, what was ended,
//! what of the turn's work could not be and still runs, and that the turn left agents,
//! and the window closes if it is free. A window that closes with work running says
//! so to whoever gave its task. Node's dispatcher never asked the host, so none of
//! these is held to a Node trace: the engine's asks of the host about it are not in
//! the traces at all.

use std::rc::Rc;

use crate::fixtures::{closed, given, Tiers};
use cf_engine::testing::Context;
use cf_proto::panes::Servers;

pub(crate) const LOCK: &str = "python3 lock.py record --run 7";
pub(crate) const LOG: &str = "tail -f run.log";

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
fn the_ending_is_the_windows_own_work_apart_from_the_pass_and_holds_the_window_meanwhile() {
    // A host gives what it ends the time to release what it holds (ten seconds on
    // Unix): the result is the chief's by then, the pass is over, no other window is
    // kept waiting, and nothing goes into the window being ended.
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus", "diana"]);
    tiers.open_body("Write the parser");
    tiers.open_body("Write the lexer");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");
    assert_eq!(tiers.task(2).task.state, "working");
    let pane = context.host.last("zeus").unwrap().pane;
    let ending = context.background.hold_endings();
    context.background.run("zeus", &[LOCK]);
    context.adapter.answer("zeus", "Started the recording run");

    // The pass that takes the result begins the ending and is over: it waits for none.
    let dispatcher = Rc::clone(&context.dispatcher);
    let pass = context
        .executor
        .start(async move { dispatcher.pass().await });
    context.settle();
    assert!(
        pass.ended(),
        "the pass does not wait for what is being ended"
    );
    assert_eq!(tiers.task(1).task.state, "done");
    assert_eq!(results(&context, &tiers), ["Started the recording run"]);
    assert_eq!(context.background.endings_asked("zeus"), 1, "it was asked");
    assert_eq!(
        context.background.ended("zeus"),
        Vec::<String>::new(),
        "and has not ended it yet"
    );
    assert_eq!(tiers.notes("chief"), Vec::<String>::new());
    assert!(
        !closed(&context, &pane),
        "the window is not closed while its work is being ended"
    );

    // The next pass looks at the other window while the first is being ended, and
    // finds the first held: it is not looked at again, nor asked a second time.
    context.adapter.answer("diana", "Lexer done");
    context.pass().unwrap();
    assert_eq!(tiers.task(2).task.state, "done");
    let mut given_to_the_chief = results(&context, &tiers);
    given_to_the_chief.sort();
    assert_eq!(
        given_to_the_chief,
        ["Lexer done", "Started the recording run"]
    );
    assert_eq!(context.background.endings_asked("zeus"), 1);

    ending.open();
    context.settle();
    assert_eq!(context.background.ended("zeus"), [LOCK]);
    assert_eq!(tiers.notes("chief"), [LOCK_ENDED]);
    assert!(closed(&context, &pane), "and then it closes");
}

#[test]
fn a_turn_that_left_nothing_says_nothing_and_the_host_is_asked_all_the_same() {
    // The host says what the turn left: a server the harness started with its
    // window is none of it, and the turn's end is the result.
    let context = Context::new();
    let tiers = working(&context);
    context.adapter.answer("zeus", "Parser done");
    context.pass().unwrap();

    assert_eq!(tiers.task(1).task.state, "done");
    assert_eq!(results(&context, &tiers), ["Parser done"]);
    assert_eq!(context.background.endings_asked("zeus"), 1, "it was asked");
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

    assert_eq!(context.background.endings_asked("chief"), 0);
    assert_eq!(context.background.ended("chief"), Vec::<String>::new());
    assert_eq!(tiers.task(1).task.state, "working");
}

#[test]
fn what_of_the_turns_work_the_host_could_not_end_is_said_to_still_run_beside_what_it_ended() {
    let context = Context::new();
    context.background.stubborn("zeus", &[LOG]);
    let tiers = answered_leaving(&context, &[LOCK]);

    assert_eq!(tiers.task(1).task.state, "done");
    assert_eq!(
        tiers.notes("chief"),
        ["T-1's turn left background work running. Ended: python3 lock.py record --run 7. Still running, not ended: tail -f run.log."]
    );
}

#[test]
fn a_host_that_does_not_answer_an_ending_has_said_nothing_and_none_is_made_up() {
    // A host that does not know the operation, or that did not answer: nothing was
    // ended that anyone knows of, and nothing is said was.
    let context = Context::new();
    context.background.end_nothing();
    let tiers = answered_leaving(&context, &[LOCK]);

    assert_eq!(
        tiers.task(1).task.state,
        "done",
        "the result goes all the same"
    );
    assert_eq!(context.background.endings_asked("zeus"), 1);
    assert_eq!(tiers.notes("chief"), Vec::<String>::new());
}

#[test]
fn what_the_note_names_is_a_few_commands_and_what_is_ended_is_all_of_them() {
    // The host names the first sixteen of what it ended and says how many: it chose
    // from all of it, and the note counts what it does not name.
    let context = Context::new();
    let jobs: Vec<String> = (1..=20).map(|n| format!("python3 job.py {n}")).collect();
    let commands: Vec<&str> = jobs.iter().map(String::as_str).collect();
    let tiers = answered_leaving(&context, &commands);

    assert_eq!(
        context.background.ended("zeus").len(),
        20,
        "all of them were ended"
    );
    assert_eq!(
        tiers.notes("chief"),
        ["T-1's turn left background work running. Ended: python3 job.py 1; python3 job.py 2; python3 job.py 3; python3 job.py 4; python3 job.py 5; and 15 more."]
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
fn asks_only_a_worker_whose_turn_is_over_with_its_task_working() {
    let context = Context::new();
    let tiers = working(&context);

    // At work on its turn: nothing to end yet.
    context.adapter.busy("zeus");
    context.pass().unwrap();
    assert_eq!(context.background.endings_asked("zeus"), 0);
    assert_eq!(
        context.background.endings_asked("chief"),
        0,
        "never a chief's window"
    );

    context.adapter.answer("zeus", "Parser done");
    context.pass().unwrap();
    assert_eq!(
        context.background.endings_asked("zeus"),
        1,
        "asked as the result is taken"
    );
    assert_eq!(context.background.endings_asked("chief"), 0);
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
