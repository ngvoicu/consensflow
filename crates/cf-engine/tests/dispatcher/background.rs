//! A worker's turn that ends while the background work it started still runs
//! is not its task's result (the report of 2026-10-09: a 30-minute recording
//! run left under a lock tool died with the window its worker's turn had
//! ended). The pane host says what each window's tree runs that began after
//! its turn did; while there is some, the task stays working, the board says
//! so, and the chief is given no result; when it has ended, the turn the
//! harness wakes its agent with is the one whose end is the result, or the last
//! turn's end is, once the agent has had a moment. A window that closes with
//! such work running says so to whoever gave its task. Node's dispatcher never
//! asked the host, so none of these is held to a Node trace: the engine's
//! asks of the host about it are not in the traces at all.

use cf_engine::testing::Context;
use cf_engine::{Activity, ActivityState};

use crate::fixtures::{closed, given, Tiers};

const LOCK: &str = "python3 lock.py record --run 7";
const LOG: &str = "tail -f run.log";

/// A program of the harness's own that it started in the turn and keeps for the
/// session, as Codex does its `codex-code-mode-host` the first time its agent
/// runs a command. The kit's workers run on Claude Code: this one is named for it.
const HELPER: &str = "/opt/claude/bin/claude-code-mode-host --serve";

/// How long the engine gives a woken agent to begin its turn, in the kit.
const SETTLE_MS: i64 = 10_000;

/// T-1 given to zeus, whose agent starts what `commands` say, answers that it
/// did and ends its turn: the pass that looks at it is made.
fn left_running<'a>(context: &'a Context, commands: &[&str]) -> Tiers<'a> {
    let tiers = Tiers::new(context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");
    context.background.run("zeus", commands);
    context.adapter.answer("zeus", "Started the recording run");
    context.pass().unwrap();
    tiers
}

/// What the window of T-1 is doing.
fn doing(context: &Context, tiers: &Tiers<'_>) -> Activity {
    context.dispatcher.activity(tiers.id(&tiers.assignee(1)))
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

#[test]
fn holds_the_end_of_a_turn_while_the_work_it_started_runs() {
    let context = Context::new();
    let tiers = left_running(&context, &[LOCK, LOG]);

    assert_eq!(tiers.task(1).task.state, "working", "no result yet");
    assert_eq!(results(&context, &tiers), Vec::<String>::new());
    assert_eq!(
        doing(&context, &tiers),
        Activity::because(
            ActivityState::Working,
            "background work running (2 processes)"
        )
    );
    let pane = context.host.last("zeus").unwrap().pane;
    assert!(!closed(&context, &pane), "the window is not closed");

    // Nothing changes as it runs, however many passes look at it.
    context.advance(60_000);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");
    assert!(!closed(&context, &pane));
}

#[test]
fn says_how_many_processes_run_as_their_number_changes() {
    let context = Context::new();
    let tiers = left_running(&context, &[LOCK]);
    assert_eq!(
        doing(&context, &tiers).reason.as_deref(),
        Some("background work running (1 process)")
    );
    context.background.run("zeus", &[LOCK, LOG, "sleep 600"]);
    context.pass().unwrap();
    assert_eq!(
        doing(&context, &tiers).reason.as_deref(),
        Some("background work running (3 processes)")
    );
}

#[test]
fn the_end_of_the_turn_the_work_wakes_is_the_result() {
    let context = Context::new();
    let tiers = left_running(&context, &[LOCK]);

    // The job ends, and the harness wakes its agent: a turn of its own.
    context.background.end("zeus");
    context.adapter.busy("zeus");
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");
    assert_eq!(
        doing(&context, &tiers),
        Activity::of(ActivityState::Working)
    );

    context
        .adapter
        .answer("zeus", "The recording finished: 3 files");
    context.pass().unwrap();

    assert_eq!(tiers.task(1).task.state, "done");
    assert_eq!(
        results(&context, &tiers),
        ["Started the recording run\n\nThe recording finished: 3 files"]
    );
}

#[test]
fn work_that_ends_with_no_turn_to_wake_leaves_the_last_turns_end_as_the_result_after_the_settle_time(
) {
    let context = Context::new();
    let tiers = left_running(&context, &[LOCK]);

    context.background.end("zeus");
    context.pass().unwrap();
    assert_eq!(
        tiers.task(1).task.state,
        "working",
        "the agent may be woken"
    );
    assert_eq!(
        doing(&context, &tiers).state,
        ActivityState::Working,
        "and the board still says it works"
    );

    context.advance(SETTLE_MS - 1_000);
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working", "not yet");

    context.advance(1_000);
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "done");
    assert_eq!(results(&context, &tiers), ["Started the recording run"]);
}

#[test]
fn work_that_runs_for_ever_holds_the_result_for_ever() {
    let context = Context::new();
    let tiers = left_running(&context, &[LOCK]);

    // A day, and a week, of a job that runs on: a window is never stopped for
    // how long it works.
    for hours in [24, 24 * 6] {
        context.advance(hours * 3_600_000);
        context.pass().unwrap();
        assert_eq!(tiers.task(1).task.state, "working");
    }
    assert_eq!(
        doing(&context, &tiers).reason.as_deref(),
        Some("background work running (1 process)")
    );
    let pane = context.host.last("zeus").unwrap().pane;
    assert!(!closed(&context, &pane));
}

#[test]
fn a_turn_that_failed_is_the_tasks_failure_whatever_its_agent_left_running() {
    let context = Context::new();
    let tiers = left_running(&context, &[LOCK]);
    assert_eq!(tiers.task(1).task.state, "working", "held for the job");

    // The harness reports the turn failed, with the job still running.
    context.adapter.with("zeus", |agent| agent.failed = true);
    context.pass().unwrap();

    assert_eq!(tiers.task(1).task.state, "failed");
}

#[test]
fn work_the_window_had_before_the_turn_holds_nothing() {
    // The host lists only what began after the turn did: a server the harness
    // started with its window is not among it, and the turn's end is the result.
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    context.adapter.answer("zeus", "Parser done");
    context.pass().unwrap();

    assert_eq!(tiers.task(1).task.state, "done");
    assert_eq!(results(&context, &tiers), ["Parser done"]);
}

#[test]
fn a_helper_the_harness_started_in_the_turn_is_no_work_the_agent_left_and_holds_nothing() {
    // Found by the live test of the gate: a Codex designer's turn ended and its
    // result never came, held for `codex-code-mode-host` for as long as the window lived.
    let context = Context::new();
    let tiers = left_running(&context, &[HELPER]);

    assert_eq!(tiers.task(1).task.state, "done");
    assert_eq!(results(&context, &tiers), ["Started the recording run"]);
}

#[test]
fn beside_the_work_of_the_agent_the_harnesss_own_helper_is_not_counted() {
    let context = Context::new();
    let tiers = left_running(&context, &[HELPER, LOCK]);

    assert_eq!(tiers.task(1).task.state, "working", "held for the job");
    assert_eq!(
        doing(&context, &tiers).reason.as_deref(),
        Some("background work running (1 process)")
    );
}

#[test]
fn a_window_that_closes_with_only_the_harnesss_helper_left_says_nothing_of_work_running() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
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
fn marks_the_window_where_a_turn_begins_and_not_while_its_task_is_held() {
    let context = Context::new();
    let tiers = left_running(&context, &[LOCK]);
    let pane = context.host.last("zeus").unwrap().pane;
    assert_eq!(
        context.background.marked(),
        vec![pane.clone()],
        "once: when the first message showed in the window's record"
    );

    // A word from the chief reaches the window while its task is held: it is
    // no new start of the work the task's turn began.
    context.tell(
        tiers.project.id,
        &tiers.assignee(1),
        1,
        "How is the run going?",
    );
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(given(&context, "zeus").len(), 2, "the tell was delivered");
    assert_eq!(
        context.background.marked(),
        vec![pane],
        "and marked nothing"
    );
    // (A tell is a question, and the task waits for its answer, paused: the
    // work its turn started is still the task's when it goes on.)
    assert_eq!(tiers.task(1).task.state, "paused");
}

#[test]
fn marks_the_window_before_a_message_goes_into_it_when_nothing_is_held() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    let pane = context.host.last("zeus").unwrap().pane;
    assert_eq!(context.background.marked(), vec![pane.clone()]);

    // The agent is at work on its turn, and a word for it waits until it is
    // not: delivered, it begins a turn of its own.
    context.adapter.busy("zeus");
    context.tell(tiers.project.id, &tiers.assignee(1), 1, "Use grammar v2");
    context.pass().unwrap();
    context.adapter.with("zeus", |agent| agent.settled = true);
    context.pass().unwrap();
    context.pass().unwrap();

    assert_eq!(context.background.marked(), [pane.clone(), pane]);
}

#[test]
fn asks_only_a_worker_whose_turn_is_over_with_its_task_working() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();

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
}

#[test]
fn a_cancelled_task_closes_its_window_with_the_work_still_running_and_whoever_gave_the_task_is_told(
) {
    let context = Context::new();
    let tiers = left_running(&context, &[LOCK, LOG]);
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
fn a_window_closed_after_its_result_with_work_that_began_since_the_look_says_so_too() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    // The look before the result finds nothing running; the host finds a job
    // when it closes the window, one that began in between.
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
    let tiers = left_running(&context, &[LOCK]);
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
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();

    context.exit("zeus");

    let told = tiers.notes("chief");
    assert!(
        told.iter().all(|note| !note.contains("background work")),
        "{told:?}"
    );
}

#[test]
fn what_a_note_quotes_of_the_work_has_its_secrets_hidden_and_is_held_to_a_line() {
    let context = Context::new();
    let tiers = left_running(&context, &[LOCK]);
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
