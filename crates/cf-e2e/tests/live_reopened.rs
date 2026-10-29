//! Live: a worker's window opened again with no task reads at rest, and reads at work
//! as soon as it has a turn of its own, on every harness. It is opt-in
//! (`cargo xtask live reopened`, `npm run live:reopened`): it spends the machine's quota
//! on a short task and a short command of each harness named, and needs each installed
//! and logged in, so it is ignored unless asked for and is no part of `check` or of CI.
//!
//! The report it answers (T-722, 2026-10-26, ConsensFlow 3.0.6-alpha.2): a Devin tester
//! had finished its task, which was accepted, and its window had closed. The board's view
//! button opened it again, with no task, and its row read "Working" for fifty minutes and
//! more with Devin at rest. A harness resumed on a conversation waits at its prompt and
//! works on nothing until it is given a message, whatever its record says of the turn
//! before; the board must say so. What the record says is each harness's own, and some
//! of it cannot end a turn that ended (Devin's GPT models store no last answer, Pi's
//! failed turn is no turn at rest), so the case is the board's reading of a real window,
//! not a reading of a record.
//!
//! The run is a real harness as the one member of a project whose chief is the stand-in,
//! twice, in two shapes of the conversation the window comes back on:
//! - **finished**: the check, as the chief, gives it a short task and waits for its answer
//!   and for its window to close;
//! - **cut**: it is given the long command of the live suites, and its window is ended
//!   with the command running, as the app's quit ends every window (`pane.kill`), so
//!   that its conversation ends in a call with no result.
//!
//! The human then opens the window (`session.open`, the board's view button) and the
//! board's reading of it is looked at every half second. It must read "idle" within
//! [`AT_REST`] and never "working" after. In the first shape the control is what a fix
//! must not take away: the human pastes the long command into the same window, and it
//! must read "working" for as long as the command runs, and "idle" once it has answered.
//!
//! ```text
//! cargo xtask live reopened                    every harness, one after the other
//! cargo xtask live reopened devin pi           the ones named
//! cargo xtask live reopened devin --shape cut  one shape (`finished` or `cut`) of it
//! cargo xtask live reopened -- --list          lists this test
//! ```

use std::path::Path;
use std::thread::sleep;
use std::time::{Duration, Instant};

use cf_e2e::live::members::{member, named, stand_in_chief, Account, LongCommand, Members, Plan};
use cf_e2e::live::report::{self, Check};
use cf_e2e::live::{self, text, Harness, Log};
use cf_e2e::Result;
use serde_json::{json, Value};

/// The stand-in for the chief's window.
const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");

/// The command the window is asked to run: a minute, long enough that a window that
/// reads at rest in the middle of it is caught at it (T-656 saw it at 60 to 100 s).
const LONG: LongCommand = LongCommand::lasting("cf-reopened-long-command", 60);

/// How long the first task has to be answered and its window to close, the reopened
/// window to be up, to read at rest, and to answer the control.
const ANSWERED: Duration = Duration::from_secs(240);
const OPENED: Duration = Duration::from_secs(90);
const AT_REST: Duration = Duration::from_secs(60);
const WORKS: Duration = Duration::from_secs(120);
const SETTLES: Duration = Duration::from_secs(240);

/// How long the window is watched after it first reads at rest.
const WATCH: Duration = Duration::from_secs(15);

/// How often the board is looked at.
const LOOK: Duration = Duration::from_millis(500);

/// What the worker is first asked: a word to answer with.
const FIRST: &str = "Reply with exactly one line: REOPENED_FIRST_DONE";

/// What the human pastes into the reopened window: the long command, then a word.
fn control() -> String {
    format!(
        "Run this exact command with your shell tool and wait for it to finish:\n{}\nThen reply with exactly one line: REOPENED_CONTROL_DONE",
        LONG.text()
    )
}

/// The board's reading of the worker's lane: its activity state.
fn state_of(members: &Members, handle: &str) -> Result<String> {
    Ok(text(&members.lane(handle)?["activity"]["state"]))
}

/// The task given, answered, and its window closed: the worker's handle.
fn first_task(members: &Members, agent: &str, log: &Log) -> Result<String> {
    let number = members.give(agent, FIRST)?;
    log.say(format!("T-{number} is given: {FIRST}"));
    let started = Instant::now();
    let lane = loop {
        let state = text(&members.task(number)?["state"]);
        let lane = members.lane(agent)?;
        if !matches!(state.as_str(), "open" | "queued" | "working") && lane["pane"].is_null() {
            break lane;
        }
        if started.elapsed() > ANSWERED {
            return Err(cf_e2e::Error::Daemon(format!(
                "T-{number} is {state} and the worker's window is {} after {ANSWERED:?}",
                if lane["pane"].is_null() {
                    "closed"
                } else {
                    "open"
                }
            )));
        }
        sleep(LOOK);
    };
    Ok(text(&lane["participant"]["handle"]))
}

/// The window of `handle` once the human has opened it: its pane.
fn opened(members: &Members, handle: &str) -> Result<Value> {
    let said = members.page("session.open", json!({ "handle": handle }))?;
    if said["ok"] != true {
        return Err(cf_e2e::Error::Daemon(format!(
            "session.open {handle}: {said}"
        )));
    }
    let started = Instant::now();
    loop {
        let lane = members.lane(handle)?;
        if lane["pane"].is_object() {
            return Ok(lane["pane"].clone());
        }
        if started.elapsed() > OPENED {
            return Err(cf_e2e::Error::Daemon(format!(
                "the window of {handle} did not open within {OPENED:?}"
            )));
        }
        sleep(LOOK);
    }
}

/// What the board read of the reopened window: each change of its reading, and whether it
/// read at rest in time and went on so.
fn at_rest(members: &Members, handle: &str, case: &mut Account) -> Result<()> {
    let started = Instant::now();
    let mut series: Vec<String> = Vec::new();
    let mut last = String::new();
    let mut rested: Option<Duration> = None;
    let mut worked_after = None;
    while started.elapsed() < AT_REST + WATCH {
        let state = state_of(members, handle)?;
        let now = started.elapsed();
        if state != last {
            series.push(format!("+{:.1} s {state}", now.as_secs_f64()));
            last = state.clone();
        }
        match (&rested, state.as_str()) {
            (None, "idle") => rested = Some(now),
            (Some(_), "working") if worked_after.is_none() => worked_after = Some(now),
            _ => {}
        }
        if rested.is_some_and(|at| now >= at + WATCH) {
            break;
        }
        sleep(LOOK);
    }
    case.say(series.join("\n"));
    case.check(
        "the window opened with no task reads at rest",
        rested.is_some_and(|at| at <= AT_REST),
        format!("idle at {rested:?}, within {AT_REST:?}"),
    );
    case.check(
        "and does not read at work after",
        worked_after.is_none(),
        format!("working again at {worked_after:?}"),
    );
    Ok(())
}

/// The control: the long command pasted into the window. It reads at work while the
/// command runs, and at rest after.
fn works(members: &Members, handle: &str, pane: &Value, case: &mut Account) -> Result<()> {
    members.paste(pane, &control())?;
    let started = Instant::now();
    let mut running_at_rest = 0;
    let mut worked = None;
    let mut ended = None;
    while started.elapsed() < WORKS + SETTLES && ended.is_none() {
        let state = state_of(members, handle)?;
        let running = !LONG.running()?.is_empty();
        let now = started.elapsed();
        if state == "working" && worked.is_none() {
            worked = Some(now);
        }
        // A window reading at rest in the middle of its long command is a false rest.
        if running && worked.is_some() && state == "idle" {
            running_at_rest += 1;
        }
        if worked.is_some() && !running && state == "idle" {
            ended = Some(now);
        }
        if worked.is_none() && now > WORKS {
            break;
        }
        sleep(LOOK);
    }
    // What a run leaves on the machine is ended: the command is the window's.
    if let Ok(Some(window)) = members.window_of(handle) {
        if let Some(own) = members.window_process(&window) {
            let _ = LONG.end_in(own);
        }
    }
    case.check(
        "a turn the human gives it reads at work",
        worked.is_some(),
        format!("working at {worked:?}"),
    );
    case.check(
        "and does not read at rest in the middle of its command",
        running_at_rest < 2,
        format!("{running_at_rest} looks at rest while the command ran"),
    );
    case.check(
        "and reads at rest once it has answered",
        ended.is_some(),
        format!("at rest at {ended:?}"),
    );
    Ok(())
}

/// The cut: the long command given as a task, the window ended while it runs, and the
/// daemon having seen the window go. The worker's handle, and the window's own process.
fn cut_in_its_command(members: &Members, agent: &str, log: &Log) -> Result<String> {
    let number = members.give(agent, &control())?;
    log.say(format!("T-{number} is given: the long command"));
    let started = Instant::now();
    let window = loop {
        let lane = members.lane(agent)?;
        if text(&lane["activity"]["state"]) == "working" && !LONG.running()?.is_empty() {
            if let Some(window) = members.window_in(lane) {
                break window;
            }
        }
        if started.elapsed() > WORKS {
            return Err(cf_e2e::Error::Daemon(format!(
                "T-{number} did not reach its command within {WORKS:?}"
            )));
        }
        sleep(LOOK);
    };
    let own = members.window_process(&window);
    members.cut(&window.pane)?;
    // What the cut leaves on the machine is ended: the command was the window's.
    if let Some(own) = own {
        let _ = LONG.end_in(own);
    }
    let gone = Instant::now();
    while !members.lane(&window.handle)?["pane"].is_null() {
        if gone.elapsed() > OPENED {
            return Err(cf_e2e::Error::Daemon(format!(
                "the daemon still has the window of {} {OPENED:?} after it was cut",
                window.handle
            )));
        }
        sleep(LOOK);
    }
    Ok(window.handle)
}

/// A window of the shapes a conversation ends in, and how each is run.
type Shape = fn(&mut Members, &str, &Log, &mut Account) -> Result<()>;

/// A finished conversation: its window opened with no task reads at rest, and a turn of
/// its own reads at work.
fn finished(members: &mut Members, agent: &str, log: &Log, case: &mut Account) -> Result<()> {
    let handle = first_task(members, agent, log)?;
    let pane = opened(members, &handle)?;
    at_rest(members, &handle, case)?;
    works(members, &handle, &pane, case)
}

/// A conversation cut in a command: its window opened with no task reads at rest.
fn cut(members: &mut Members, agent: &str, log: &Log, case: &mut Account) -> Result<()> {
    let handle = cut_in_its_command(members, agent, log)?;
    opened(members, &handle)?;
    at_rest(members, &handle, case)
}

/// The run of one harness in one shape.
fn run(harness: Harness, shape: Shape, log: &Log) -> Result<Account> {
    let plan = Plan::new("reopened", stand_in_chief()).with(member(harness));
    let agent = plan.roster[1]["id"].as_str().unwrap_or_default().to_owned();
    let mut members = Members::open(&plan, Path::new(FAKE_AGENT), log)?;
    members.add(&agent, &[])?;
    let mut case = Account::new();
    let seen = shape(&mut members, &agent, log, &mut case);
    let closed = members.close();
    // What failed, and how the close went, are both the run's.
    if let Err(failed) = &seen {
        case.fail(failed);
    }
    case.closed(closed);
    Ok(case)
}

/// The shapes, by the word that asks for one (`--shape cut`), and what each holds of a
/// window.
const SHAPES: [(&str, &str, Shape); 2] = [
    ("finished", "a finished conversation", finished),
    ("cut", "a conversation cut in a command", cut),
];

#[test]
#[ignore = "opt-in: spends the machine's quota on a short task and a short command of each harness named; cargo xtask live reopened (npm run live:reopened)"]
fn a_workers_window_opened_again_with_no_task_reads_at_rest_and_at_work_on_a_turn_of_its_own(
) -> Result {
    let log = Log::begin("live reopened")?;
    let harnesses = named(
        &live::list("harness"),
        &Harness::ALL,
        Harness::name,
        "harness",
        &Harness::ALL,
    )?;
    let mut checks = Vec::new();
    for harness in harnesses {
        let scoped = log.scoped(harness.name());
        let version = live::version_of(harness);
        let asked = live::list("shape");
        for (word, what, shape) in SHAPES {
            if !asked.is_empty() && !asked.iter().any(|asked| asked == word) {
                continue;
            }
            let case = run(harness, shape, &scoped).unwrap_or_else(|failed| {
                let mut case = Account::new();
                case.fail(&failed);
                case
            });
            log.block(
                &format!(
                    "== {} {version}, {what}: {}",
                    harness.name(),
                    if case.held() { "ok" } else { "FAILED" }
                ),
                &case.text(),
            );
            checks.push(Check::new(
                harness.name(),
                &format!("{what}, its window opened again with no task reads at rest"),
                case.held(),
                version.clone(),
            ));
        }
    }
    report::conclude(&log, "what each worker came to", &checks)
}
