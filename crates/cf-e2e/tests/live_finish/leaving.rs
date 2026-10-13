//! A worker that leaves a long command running and answers (the owner's decision of
//! 2026-10-13, after T-7 and T-15 on Windows): its answer is its task's result at
//! once, what its turn left running is ended, and the chief is told what was ended in a
//! note beside the result. The worker is asked to start a long command in the
//! background, `sleep 4711` (`ping -n 4711 127.0.0.1` on Windows, which has no `sleep`
//! that every one of its shells knows), not to wait for it, and to answer. The run
//! looks, once a second, at the task, the worker's lane of the board and what the pane
//! host lists of its tree (`pane.background`), which is where it learns the command's
//! process (its command line holds `4711`, and no other process of the machine's does);
//! and holds what it saw to the brief ([`judge`]):
//!
//! - the task is done, and its result reached the chief;
//! - the host listed the command as running: a worker that left none (its harness may
//!   end what a tool started when the tool call ends, or the agent took it out of the
//!   window's session with `setsid` and every process that started it was gone before
//!   the host saw one of them start, which no tree the host has holds) proves nothing,
//!   and fails;
//! - the command is gone once the task is done;
//! - and the chief was told, in the note beside the result, that the turn left work
//!   and that it was ended: a note that says part of what the host proved the turn's still
//!   runs, and was not ended, fails.
//!
//! Whatever the verdict, the run says what the host listed at the turn's end, what it
//! said the window left when it closed, and what the chief was told.

use std::path::Path;
use std::time::Duration;

use cf_e2e::live::members::{
    self, delivered_result, find, is_over, notes_of_work, Account, Members, Plan,
};
use cf_e2e::live::tree::{self, Proc, Tree};
use cf_e2e::live::{cut, secs, text, Log, Who};
use cf_e2e::process::is_alive;
use cf_e2e::Result as Fallible;

use crate::check::{
    agent, failure, progress_line, Finished, Progress, CLOSES, DELIVERS, LOOK, WORK,
};

/// How many seconds the command the worker starts runs: more than a run lasts, and a
/// number that is in no other command line of the machine.
pub const SECONDS: u64 = 4711;

/// The command the worker starts, as this machine's shells know it: Windows has no
/// `sleep` in PowerShell or in `cmd`, and has `ping` in every one of them.
fn command() -> String {
    if cfg!(windows) {
        format!("ping -n {SECONDS} 127.0.0.1")
    } else {
        format!("sleep {SECONDS}")
    }
}

/// How the worker may detach the command when its shell tool has no option to run one in
/// the background.
fn detach() -> String {
    if cfg!(windows) {
        format!(
            "`Start-Process -WindowStyle Hidden ping -ArgumentList '-n','{SECONDS}','127.0.0.1'` in PowerShell, or `nohup {} > /dev/null 2>&1 &` in a shell that has `nohup`",
            command()
        )
    } else {
        format!("`nohup {} > /dev/null 2>&1 &`", command())
    }
}

/// What the chief gives the worker: for a harness, to start the command in the background
/// and answer; for the rig's stand-in, which has no shell tool, to start a job of the
/// same number of seconds (its command line holds it), detached from the shell that runs
/// it as the agents of four harnesses detached theirs, and answer.
pub fn brief(who: Who) -> String {
    match who {
        Who::StandIn => format!("BACKGROUND --detach {SECONDS} left a job running"),
        Who::Harness(_) => format!(
            "Start the command `{}` in the background, so that it is still running when you have answered: use your shell tool's option to run a command in the background if it has one, or detach it yourself ({}). Do not wait for it, do not look at it and do not stop it. When it has started, reply with one line that says so, and nothing more.",
            command(),
            detach()
        ),
    }
}

/// What the run saw of the worker, its command and what the chief was told.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Left {
    /// The state the task was last seen in, and how long it was watched.
    pub state: String,
    pub took: Duration,
    /// The processes of the command the host listed while the task was watched, each
    /// once, in the order it first listed them.
    pub listed: Vec<Proc>,
    /// The result the chief was given, if it was.
    pub result: Option<String>,
    /// What the chief was told of work the turn left, the notes that say so.
    pub notes: Vec<String>,
    /// The processes of `listed` that were still there when the run last looked, once
    /// the task was done and the chief told.
    pub alive: Vec<u64>,
}

impl Left {
    /// What the chief's notes say of the turn: that it was told work was ended, and that
    /// it was told some was not.
    fn told(&self) -> (bool, bool) {
        let said = |words: &str| self.notes.iter().any(|note| note.contains(words));
        (said(" Ended: "), said(" Still running, not ended: "))
    }
}

/// The task is done, its result reached the chief, the host listed the command as
/// running, it is gone, and the chief was told that it was ended: or every way it was
/// not.
pub fn judge(left: &Left) -> Result<(), Vec<String>> {
    let mut problems = Vec::new();
    if left.state != "done" {
        problems.push(format!(
            "T-1 is {}, not done after {} s",
            left.state,
            secs(left.took)
        ));
    }
    match &left.result {
        None => problems.push("the chief was given no result".to_owned()),
        Some(body) if body.trim().is_empty() => {
            problems.push("the result the chief was given is empty".to_owned());
        }
        Some(_) => {}
    }
    if left.listed.is_empty() {
        problems.push(format!(
            "the pane host never listed the command ({SECONDS}) as running: the worker left none, so there is nothing to end (its harness may end what a tool started when the tool call ends), or the host never found it (the agent started it in a session of its own, and the shell that started it was gone before the host saw it start)"
        ));
    } else {
        if !left.alive.is_empty() {
            let still: Vec<String> = left
                .listed
                .iter()
                .filter(|process| left.alive.contains(&process.pid))
                .map(|process| format!("{} {}", process.pid, process.command))
                .collect();
            problems.push(format!(
                "the command is still running after the task was done and the chief told: {}",
                still.join("; ")
            ));
        }
        let (ended, not_ended) = left.told();
        if left.notes.is_empty() {
            problems.push("the chief was told nothing of the work the turn left".to_owned());
        } else if !ended {
            problems.push(format!(
                "the chief was told the turn left work, but not that any of it was ended: {}",
                left.notes.join(" | ")
            ));
        }
        if not_ended {
            problems.push(format!(
                "the chief was told part of what the turn left was not ended, and still runs: {}",
                left.notes.join(" | ")
            ));
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

/// The run in a line, for the table that ends it: that the command was ended and the
/// chief told, and else the first thing that was wrong with it.
pub fn summary(left: &Left, verdict: &Result<(), Vec<String>>) -> String {
    match verdict {
        Ok(()) => format!(
            "T-1 done after {} s; the command ({}) was ended and the chief told: {}",
            secs(left.took),
            left.listed
                .iter()
                .map(|process| process.pid.to_string())
                .collect::<Vec<_>>()
                .join(", "),
            cut(&left.notes.join(" | "), 160)
        ),
        Err(problems) => problems.first().cloned().unwrap_or_default(),
    }
}

/// What the chief was told, whichever way the run came out: the notes that say the turn left
/// work, a line to each, or that it was told none.
fn at_the_chief(left: &Left) -> String {
    if left.notes.is_empty() {
        return "what the chief was told of work the turn left: nothing".to_owned();
    }
    let lines: Vec<String> = left
        .notes
        .iter()
        .map(|note| format!("    {note}"))
        .collect();
    format!(
        "what the chief was told of work the turn left:\n{}",
        lines.join("\n")
    )
}

/// Runs the worker `who` on the project `plan` opens, with the task [`brief`] gives:
/// what it came to, which holds when the task was done, the command left running was
/// ended and the chief told so.
pub fn run(who: Who, plan: &Plan, stand_in: &Path, log: &Log) -> Fallible<Finished> {
    let log = log.scoped(who.name());
    let mut members = Members::open(plan, stand_in, &log)?;
    let finished = observe(&mut members, &agent(who), &brief(who), &log);
    let closed = members.close();
    let mut finished = finished?;
    finished.case.closed(closed);
    Ok(finished)
}

/// The worker given its task, and the board and the pane host looked at once a second until
/// the task ends; then the result and the chief's note waited for, the command waited to be
/// gone, and what was seen held to the brief.
fn observe(members: &mut Members, agent: &str, brief: &str, log: &Log) -> Fallible<Finished> {
    let mut case = Account::new();
    if let Some(trust) = members.trust() {
        case.say(format!("trust: {trust}"));
    }
    members.add(agent, &[])?;
    let number = members.give(agent, brief)?;
    log.say(format!("T-{number} is given: {}", cut(brief, 140)));

    let mark = SECONDS.to_string();
    let mut left = Left::default();
    let mut began: Option<Duration> = None;
    let mut pane: Option<String> = None;
    let mut progress = Progress::default();
    let ended = find(log, "the worker's task to end", WORK, LOOK, || {
        let thread = members.task(number)?;
        let lane = members.lane(agent)?;
        let listing = (!lane["pane"].is_null()).then(|| tree::read(members.rig(), &lane["pane"]));
        progress.say(log, progress_line(number, &thread, &lane, listing.as_ref()));
        left.state = text(&thread["state"]);
        let now = log.elapsed();
        left.took = now - *began.get_or_insert(now);
        if let Some(id) = lane["pane"]["id"].as_str() {
            pane = Some(id.to_owned());
        }
        if let Some(Ok(listed)) = &listing {
            for process in listed.holding(&mark) {
                if !left.listed.iter().any(|known| known.pid == process.pid) {
                    left.listed.push(process.clone());
                }
            }
        }
        Ok(is_over(&thread).then_some(()))
    })?
    .is_some();
    if left.state == "done" {
        find(
            log,
            "the result to be delivered to the chief",
            DELIVERS,
            LOOK,
            || {
                left.result = delivered_result(&members.inbox(Some(members.chief()))?);
                Ok(left.result.is_some().then_some(()))
            },
        )?;
        // The chief is told what was ended beside the result, once it has ended it.
        find(
            log,
            "the chief to be told what the turn left",
            DELIVERS,
            LOOK,
            || {
                left.notes = notes_of_work(&members.inbox(Some(members.chief()))?, number);
                Ok((!left.notes.is_empty()).then_some(()))
            },
        )?;
        find(log, "the command to be gone", CLOSES, LOOK, || {
            left.alive = still_there(&left.listed);
            Ok(left.alive.is_empty().then_some(()))
        })?;
    }
    let mut exit: Option<Tree> = None;
    if let (true, Some(pane)) = (ended, pane.clone()) {
        find(log, "the worker's window to close", CLOSES, LOOK, || {
            exit = tree::left_at_exit(members.rig(), &pane);
            Ok(exit.is_some().then_some(()))
        })?;
    }

    let verdict = judge(&left);
    case.say(format!(
        "T-{number} is {} after {} s; the host listed the command's processes: {}",
        left.state,
        secs(left.took),
        listed_line(&left.listed)
    ));
    case.say(at_the_chief(&left));
    case.say(match &exit {
        Some(tree) => format!(
            "what the pane host said the window left running when it closed (its pane.exit): {}",
            tree.line()
        ),
        None => "the pane host said nothing of what the window left when it closed".to_owned(),
    });
    match &verdict {
        Ok(()) => case.check(
            "the command was ended with the result, and the chief told",
            true,
            summary(&left, &verdict),
        ),
        Err(problems) => {
            for problem in problems {
                case.fail(problem);
            }
            for line in failure(members, agent, number) {
                case.say(line);
            }
        }
    }
    Ok(Finished {
        case,
        summary: summary(&left, &verdict),
    })
}

/// The processes of `listed` that are still running.
fn still_there(listed: &[Proc]) -> Vec<u64> {
    listed
        .iter()
        .filter(|process| u32::try_from(process.pid).is_ok_and(is_alive))
        .map(|process| process.pid)
        .collect()
}

/// The processes of the command the host listed, each by its number and command line.
fn listed_line(listed: &[Proc]) -> String {
    if listed.is_empty() {
        return "none".to_owned();
    }
    let found: Vec<String> = listed
        .iter()
        .map(|process| format!("{} {}", process.pid, cut(&process.command, 90)))
        .collect();
    found.join("; ")
}

/// The project of `who`'s run: a chief that is the stand-in and the worker alone, in a folder of its
/// own, apart from the one a worker's plain run uses.
pub fn plan(who: Who) -> Plan {
    let worker = match who {
        Who::StandIn => members::stand_in_worker(),
        Who::Harness(harness) => members::member(harness),
    };
    Plan::new(
        &format!("leaving-{}", who.name()),
        members::stand_in_chief(),
    )
    .with(worker)
}

// The tests are in `leaving/`: a module that a path names looks for its own parts
// beside it, so these are named as well.
#[cfg(test)]
#[path = "leaving/tests.rs"]
mod tests;
