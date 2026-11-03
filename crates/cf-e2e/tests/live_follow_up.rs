//! Live: a follow-up added `--after` a task goes to a window opened again on its session's
//! conversation, and that window begins its brief, on every harness. It is opt-in
//! (`cargo xtask live follow-up`, `npm run live:follow-up`): it spends the machine's quota on
//! two or three short turns of each harness named, and needs each installed and logged in, so it
//! is ignored unless asked for and is no part of `check` or of CI.
//!
//! The report it answers (T-1130, 2026-11-02, ConsensFlow 3.0.7): T-9, added `--after T-8` for a
//! session whose window ConsensFlow had closed and opened again, went from queued to working once
//! T-8 was accepted, and an hour later its transcript held its brief alone: the window had the
//! brief and never began. The board said "working", and nobody was told. A task is working once
//! its window's record shows its brief; whether a turn began is the record's next words.
//!
//! The run is a real harness as the one member of a project whose chief is the stand-in, in two
//! shapes of the conversation its follow-up's window comes back on:
//! - **finished**: the check, as the chief, gives it a short task and waits for its answer and for
//!   its window to close;
//! - **cut**: it is given the long command of the live suites, its window is ended with the command
//!   running, as the app's quit ends every window (`pane.kill`), and its task paused for it; the
//!   chief resumes it with a short answer, which a window opened again on the conversation gives,
//!   and that window closes (the field's window, closed and opened again for its pause).
//!
//! The chief then adds T-2 `--after T-1` and accepts T-1, and T-2 is watched for [`BEGINS`]: it
//! must be done with its answer (its window began it). One that is not must not be left working:
//! a window that never began its brief has its task paused, and the chief told (the fix of
//! T-1130). The check says which, with T-2's last transcript items and the window's last screen.
//!
//! ```text
//! cargo xtask live follow-up                     every harness, one after the other
//! cargo xtask live follow-up devin claude        the ones named
//! cargo xtask live follow-up pi --shape cut      one shape (`finished` or `cut`) of it
//! cargo xtask live follow-up -- --list           lists this test
//! ```

use std::path::Path;
use std::thread::sleep;
use std::time::{Duration, Instant};

use cf_e2e::live::members::{member, named, stand_in_chief, Account, LongCommand, Members, Plan};
use cf_e2e::live::report::{self, Check};
use cf_e2e::live::{self, text, Harness, Log};
use cf_e2e::Result;

/// The stand-in for the chief's window.
const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");

/// The command the cut shape's first task runs: long enough to be cut in.
const LONG: LongCommand = LongCommand::lasting("cf-follow-up-long-command", 90);

/// How long a short task has to be answered and its window to close.
const ANSWERED: Duration = Duration::from_secs(240);
/// How long the long command has to start.
const WORKS: Duration = Duration::from_secs(120);
/// How long a cut window has to be gone from the board.
const GONE: Duration = Duration::from_secs(90);
/// How long the follow-up has to be done, or to leave `working` for its window never began it:
/// two of the daemon's arrival waits, and its window's open, with room.
const BEGINS: Duration = Duration::from_secs(300);

/// How often the board is looked at.
const LOOK: Duration = Duration::from_millis(500);

/// The words of the first task's answer, the resume's and the follow-up's.
const FIRST: &str = "FOLLOW_FIRST_DONE";
const FOLLOW: &str = "FOLLOW_UP_DONE";

/// A brief that asks for one line, `word`.
fn reply(word: &str) -> String {
    format!("Reply with exactly one line: {word}")
}

/// What the cut shape's first task asks: the long command, then a word.
fn long_task() -> String {
    format!(
        "Run this exact command with your shell tool and wait for it to finish:\n{}\nThen reply with exactly one line: {FIRST}",
        LONG.text()
    )
}

/// `cf` as the chief runs it, which must do what it is asked.
fn chief(members: &Members, words: &[&str]) -> Result<()> {
    let ran = members.as_chief(words)?;
    if ran.code == Some(0) {
        Ok(())
    } else {
        Err(cf_e2e::Error::Daemon(format!(
            "cf {}: {ran}",
            words.join(" ")
        )))
    }
}

/// Waits until task `number` is no longer queued or at work and `agent`'s window is closed: its
/// state then, and the window's handle.
fn over_and_closed(members: &Members, agent: &str, number: i64) -> Result<(String, String)> {
    let started = Instant::now();
    loop {
        let state = text(&members.task(number)?["state"]);
        let lane = members.lane(agent)?;
        if !matches!(state.as_str(), "open" | "queued" | "working") && lane["pane"].is_null() {
            return Ok((state, text(&lane["participant"]["handle"])));
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
    }
}

/// The finished shape's T-1: given, answered, and its window closed.
fn finished(members: &Members, agent: &str, log: &Log) -> Result<()> {
    let number = members.give(agent, &reply(FIRST))?;
    log.say(format!("T-{number} is given: a word"));
    let (state, _) = over_and_closed(members, agent, number)?;
    log.say(format!("T-{number} is {state}, its window closed"));
    Ok(())
}

/// The cut shape's T-1: the long command, its window cut while it runs and gone from the board,
/// then resumed for a word, answered in a window opened again, and that window closed.
fn cut(members: &Members, agent: &str, log: &Log) -> Result<()> {
    let number = members.give(agent, &long_task())?;
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
    if let Some(own) = own {
        let _ = LONG.end_in(own);
    }
    let gone = Instant::now();
    while !members.lane(&window.handle)?["pane"].is_null() {
        if gone.elapsed() > GONE {
            return Err(cf_e2e::Error::Daemon(format!(
                "the daemon still has the window of {} {GONE:?} after it was cut",
                window.handle
            )));
        }
        sleep(LOOK);
    }
    let state = text(&members.task(number)?["state"]);
    log.say(format!("T-{number} is {state} after its window was cut"));
    let task = format!("T-{number}");
    chief(
        members,
        &[
            "task",
            "resume",
            &task,
            &format!("Do not run the command again. {}", reply(FIRST)),
        ],
    )?;
    let (state, _) = over_and_closed(members, agent, number)?;
    log.say(format!(
        "T-{number} is {state} after its resume, its window closed"
    ));
    Ok(())
}

/// The notes to the chief that say T-`number` is paused.
fn paused_notes(members: &Members, number: i64) -> Result<Vec<String>> {
    let chief = members.chief().to_owned();
    let opening = format!("T-{number} is paused");
    Ok(members
        .inbox(Some(&chief))?
        .into_iter()
        .map(|message| text(&message["body"]))
        .filter(|body| body.starts_with(&opening))
        .collect())
}

/// T-2, added `--after T-1` once T-1 is over, and T-1 accepted: what came of it in [`BEGINS`].
fn follow_up(members: &Members, agent: &str, log: &Log, case: &mut Account) -> Result<()> {
    chief(members, &["task", "add", "--after", "T-1", &reply(FOLLOW)])?;
    chief(members, &["task", "accept", "T-1"])?;
    log.say("T-2 is added --after T-1, and T-1 accepted");
    let started = Instant::now();
    let mut working = None;
    let mut last = String::new();
    let mut series = Vec::new();
    let mut tail = String::new();
    let state = loop {
        let state = text(&members.task(2)?["state"]);
        let now = started.elapsed();
        if state != last {
            series.push(format!("+{:.1} s {state}", now.as_secs_f64()));
            last = state.clone();
        }
        if state == "working" && working.is_none() {
            working = Some(now);
        }
        if let Ok(Some(window)) = members.window_of(agent) {
            tail = members.tail(&text(&window.pane["id"]));
        }
        if !matches!(state.as_str(), "open" | "queued" | "working") || now > BEGINS {
            break state;
        }
        sleep(LOOK);
    };
    case.say(series.join("\n"));
    let transcript = members
        .as_chief(["task", "get", "T-2", "--transcript", "--last", "3"])
        .map(|ran| ran.stdout)
        .unwrap_or_default();
    case.say(format!("T-2 {state}; its transcript:\n{transcript}"));
    case.say(format!("its window's last screen:\n{tail}"));
    let notes = paused_notes(members, 2)?;
    case.check(
        "the follow-up's window began its brief",
        state == "done",
        format!("T-2 {state}, working at {working:?}"),
    );
    case.check(
        "a follow-up its window never began is not left working",
        state != "working" && (state != "paused" || notes.len() == 1),
        format!("T-2 {state}; the chief's notes of its pause: {notes:?}"),
    );
    Ok(())
}

/// A shape of the conversation the follow-up's window comes back on.
type Shape = fn(&Members, &str, &Log) -> Result<()>;

/// The run of one harness in one shape.
fn run(harness: Harness, shape: Shape, log: &Log) -> Result<Account> {
    let plan = Plan::new("follow-up", stand_in_chief()).with(member(harness));
    let agent = plan.roster[1]["id"].as_str().unwrap_or_default().to_owned();
    let mut members = Members::open(&plan, Path::new(FAKE_AGENT), log)?;
    members.add(&agent, &[])?;
    let mut case = Account::new();
    let seen =
        shape(&members, &agent, log).and_then(|()| follow_up(&members, &agent, log, &mut case));
    let closed = members.close();
    if let Err(failed) = &seen {
        case.fail(failed);
    }
    case.closed(closed);
    Ok(case)
}

/// The shapes, by the word that asks for one (`--shape cut`), and what each holds.
const SHAPES: [(&str, &str, Shape); 2] = [
    ("finished", "after a finished task", finished),
    ("cut", "after a task whose window was cut and resumed", cut),
];

#[test]
#[ignore = "opt-in: spends the machine's quota on two or three short turns of each harness named; cargo xtask live follow-up (npm run live:follow-up)"]
fn a_follow_up_goes_to_a_window_opened_again_on_its_conversation_that_begins_its_brief() -> Result {
    let log = Log::begin("live follow-up")?;
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
                &format!("{what}, its follow-up's window began its brief"),
                case.held(),
                version.clone(),
            ));
        }
    }
    report::conclude(&log, "what each follow-up came to", &checks)
}
