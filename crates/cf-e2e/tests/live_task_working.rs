//! Live: a worker's task is working by the time its window runs its command. It is opt-in
//! (`cargo xtask live task-working`, `npm run live:task-working`): it spends the machine's
//! quota on one short task of each harness named (Codex, the default), and needs each
//! installed and logged in, so it is ignored unless asked for and is no part of `check` or
//! of CI.
//!
//! The report it answers (T-534 item 10, 2026-10-25, ConsensFlow 3.0.6-alpha.2): with a
//! Codex worker the board still read the task "queued" while the window was already running
//! its command. The task moves to working when the window's record shows the brief
//! (`delivery.confirmed`), which Codex writes some seconds after its window opens, so the
//! question is whether that is late against the command the model runs once it has read the
//! brief. It is not: the run measures it, and the task is working a second or two before the
//! command starts. What the report saw was the check's own look: a window that has the brief
//! on its command line (Codex's first message is its last argument, Claude's too) names the
//! long command's mark from the moment it is opened, so a look for the mark among what
//! descends from the window finds the harness itself, seconds before the command. The
//! command is found here by what it is, the `node -e` the brief asks for
//! ([`LongCommand::running`]), and never by the window's tree.
//!
//! The run is a real harness as the one member of a project whose chief is the stand-in. The
//! check, as the chief, gives it a task that runs the long command, and looks at the task
//! and at the machine's `node` every 40 ms until both are seen. The run fails when the
//! command ran while the task was not yet working, or when the worker never ran it.
//!
//! ```text
//! cargo xtask live task-working                    Codex, the default
//! cargo xtask live task-working codex claude       each, one after the other
//! cargo xtask live task-working -- --list          lists this test
//! ```

use std::path::Path;
use std::thread::sleep;
use std::time::{Duration, Instant};

use cf_e2e::live::members::{member, stand_in_chief, Account, LongCommand, Members, Plan};
use cf_e2e::live::report::{self, Check};
use cf_e2e::live::{self, cut, text, Harness, Log};
use cf_e2e::Result;

/// The stand-in for the chief's window.
const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");

/// The long command the worker is asked to run.
const LONG: LongCommand = LongCommand::new("cf-task-working-long-command");

/// How long the worker has to run its command.
const WORK: Duration = Duration::from_secs(120);

/// How often the board and the machine are looked at.
const LOOK: Duration = Duration::from_millis(40);

/// What the worker is asked: the command, and a reply once it has ended.
fn brief() -> String {
    format!(
        "Run this exact command with your shell tool and wait for it to finish:\n{}\nThen reply with exactly one line: DONE",
        LONG.text()
    )
}

/// When, from the task's being given, the board first read it working and the command first
/// ran: none for what did not happen within [`WORK`].
struct Moments {
    working: Option<Duration>,
    command: Option<Duration>,
    /// Each change of what the looks found, as a line.
    series: Vec<String>,
}

/// The task `number` and the machine looked at until both moments are found, or [`WORK`] is
/// up. `started` is when the task was given.
fn look(members: &Members, number: i64, started: Instant) -> Result<Moments> {
    let mut moments = Moments {
        working: None,
        command: None,
        series: Vec::new(),
    };
    let mut last = String::new();
    while started.elapsed() < WORK && (moments.working.is_none() || moments.command.is_none()) {
        let state = members
            .task(number)
            .map(|task| text(&task["state"]))
            .unwrap_or_else(|failed| format!("unread: {failed}"));
        let running = !LONG.running()?.is_empty();
        let now = started.elapsed();
        let line = format!(
            "T-{number} is {state}; the command {}",
            if running { "runs" } else { "is not running" }
        );
        if line != last {
            moments
                .series
                .push(format!("+{:.2} s {line}", now.as_secs_f64()));
            last = line;
        }
        if state == "working" && moments.working.is_none() {
            moments.working = Some(now);
        }
        if running && moments.command.is_none() {
            moments.command = Some(now);
        }
        sleep(LOOK);
    }
    Ok(moments)
}

/// The worker given its task and what the looks found of it, held to the brief.
fn observe(members: &Members, agent: &str, log: &Log) -> Result<Account> {
    let mut case = Account::new();
    let number = members.give(agent, &brief())?;
    let started = Instant::now();
    log.say(format!("T-{number} is given: {}", cut(&brief(), 100)));
    let moments = look(members, number, started)?;
    case.say(moments.series.join("\n"));
    let (working, command) = (moments.working, moments.command);
    case.check(
        "the worker ran its command",
        command.is_some(),
        format!("{command:?} after the task was given"),
    );
    case.check(
        "the task is working by the time the command runs",
        working
            .zip(command)
            .is_some_and(|(working, command)| working <= command),
        format!("working at {working:?}, the command at {command:?}"),
    );
    // What a run leaves on the machine is ended: the command is the window's, and the window's
    // close ends the rest of its tree.
    if let Ok(Some(window)) = members.window_of(agent) {
        if let Some(own) = members.window_process(&window) {
            let _ = LONG.end_in(own);
        }
    }
    Ok(case)
}

/// The run of one harness.
fn run(harness: Harness, log: &Log) -> Result<Account> {
    let plan = Plan::new("task-working", stand_in_chief()).with(member(harness));
    let agent = plan.roster[1]["id"].as_str().unwrap_or_default().to_owned();
    let mut members = Members::open(&plan, Path::new(FAKE_AGENT), log)?;
    members.add(&agent, &[])?;
    let seen = observe(&members, &agent, log);
    let closed = members.close();
    let mut case = seen?;
    case.closed(closed);
    Ok(case)
}

#[test]
#[ignore = "opt-in: spends the machine's quota on one short task of each harness named; cargo xtask live task-working (npm run live:task-working)"]
fn a_workers_task_is_working_by_the_time_its_window_runs_its_command() -> Result {
    let log = Log::begin("live task working")?;
    let mut harnesses: Vec<Harness> = live::list("harness")
        .iter()
        .filter_map(|word| Harness::from_name(word))
        .collect();
    if harnesses.is_empty() {
        harnesses.push(Harness::Codex);
    }
    let mut checks = Vec::new();
    for harness in harnesses {
        let scoped = log.scoped(harness.name());
        let version = live::version_of(harness);
        let case = run(harness, &scoped).unwrap_or_else(|failed| {
            let mut case = Account::new();
            case.fail(&failed);
            case
        });
        log.block(
            &format!(
                "== {} {version}: {}",
                harness.name(),
                if case.held() { "ok" } else { "FAILED" }
            ),
            &case.text(),
        );
        checks.push(Check::new(
            harness.name(),
            "the task is working by the time the command runs",
            case.held(),
            version,
        ));
    }
    report::conclude(&log, "what each worker came to", &checks)
}
