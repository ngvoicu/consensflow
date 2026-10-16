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
//! - the host listed the command as running, or the command is nowhere on the machine
//!   once the task is done: Codex and Devin end what a tool started when the tool call
//!   ends (live, 2026-10-14), and their run says so; a command the host never listed
//!   that still runs on the machine escaped the ending, and fails: the agent detached it
//!   from the shell that started it (`nohup … &`, `setsid`, `disown`, `Start-Process`)
//!   and that shell ended, so no chain of processes that run leads from it to a shell the
//!   harness started, and the host ends only what such a chain proves the turn's. The run
//!   then looks again once the window has closed, and says whether the window's close
//!   found it (it ends what its tree finds) or it runs on, and ends what still runs, so
//!   that a run leaves nothing of its own on the machine;
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
    self, delivered_result, find, is_over, notes_of_work, runs_command, Account, Members, Plan,
};
use cf_e2e::live::tree::{self, Proc, Tree};
use cf_e2e::live::{cut, secs, text, Log, Who};
use cf_e2e::process::{is_alive, signal, Run, Signal};
use cf_e2e::Result as Fallible;

use crate::check::{
    agent, failure, progress_line, Finished, Progress, CLOSES, DELIVERS, LOOK, WORK,
};

/// How many seconds the command the worker starts runs: more than a run lasts, and a
/// number that is in no other command line of the machine.
pub const SECONDS: u64 = 4711;

/// How long the run waits, once the task is done, for the chief to be told what the turn
/// left, for the command to be gone and for the window to close: a harness's window may
/// take what the pane host gives it, and a run that finds none of it waits for all of it.
/// The rig's stand-in ends at once, so that a run of it that finds nothing has waited long
/// enough in a fraction.
#[derive(Debug, Clone, Copy)]
struct Waits {
    told: Duration,
    gone: Duration,
}

impl Waits {
    fn of(who: Who) -> Self {
        match who {
            Who::StandIn => Self {
                told: Duration::from_secs(15),
                gone: Duration::from_secs(10),
            },
            Who::Harness(_) => Self {
                told: DELIVERS,
                gone: CLOSES,
            },
        }
    }
}

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
/// same number of seconds (its command line holds it) in a shell that waits for it, as a
/// harness's shell tool runs a command it is waited for or keeps running in the
/// background, and answer.
pub fn brief(who: Who) -> String {
    match who {
        Who::StandIn => stand_in_brief(false),
        Who::Harness(_) => format!(
            "Start the command `{}` in the background, so that it is still running when you have answered: use your shell tool's option to run a command in the background if it has one, or detach it yourself ({}). Do not wait for it, do not look at it and do not stop it. When it has started, reply with one line that says so, and nothing more.",
            command(),
            detach()
        ),
    }
}

/// What the chief gives the rig's stand-in: to start a job of [`SECONDS`] seconds and
/// answer; where `detached`, detached from the shell that runs it, as the agents of four
/// harnesses detached theirs (`nohup … &`), which ends the shell and leaves the job with
/// no parent.
pub fn stand_in_brief(detached: bool) -> String {
    let how = if detached { "--detach " } else { "" };
    format!("BACKGROUND {how}{SECONDS} left a job running")
}

/// What the command line of the command the worker started holds, to find it on the
/// machine: the command itself, or for the rig's stand-in, which starts a job of its own,
/// the job's words.
fn wanted(who: Who) -> String {
    match who {
        Who::StandIn => format!("--background-job {SECONDS}"),
        Who::Harness(_) => command(),
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
    /// The command lines of the machine's processes that run the command, looked at once
    /// the task was done and the chief told: what escaped the ending, where the host
    /// listed none, each with where it stands on a Unix machine ([`standing`]).
    pub escaped: Vec<String>,
    /// What of `escaped` still ran when the window had closed, which ends what its tree
    /// finds: none where the close ended it unsaid. None where nothing escaped, or the
    /// window had not closed when the run last looked.
    pub after_close: Option<Vec<String>>,
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
    let close = after_the_close(left);
    if left.listed.is_empty() {
        if !left.escaped.is_empty() {
            problems.push(format!(
                "the command escaped the ending: the pane host never listed it, and it still ran once the task was done and the chief told (the agent detached it from the shell that started it, which ended, so no chain of processes that run led from it to a shell the harness started): {}; {}{}",
                left.escaped.join("; "),
                close.unwrap_or_default(),
                without_a_terminal(&left.escaped)
            ));
        }
    } else {
        if !left.alive.is_empty() {
            let still: Vec<String> = left
                .listed
                .iter()
                .filter(|process| left.alive.contains(&process.pid))
                .map(|process| format!("{} {}", process.pid, process.command))
                .collect();
            let mut said = format!(
                "the command is still running after the task was done and the chief told: {}",
                still.join("; ")
            );
            if let Some(close) = close {
                said.push_str("; ");
                said.push_str(&close);
            }
            problems.push(said);
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

/// What to read of a command that escaped and has no terminal (`terminal ??`, [`standing`]):
/// that it is in a session of its own, and the window's close ends only what runs in the
/// session its program leads. Nothing where no line says it.
fn without_a_terminal(escaped: &[String]) -> &'static str {
    if escaped.iter().any(|line| line.ends_with(", terminal ??)")) {
        " (terminal ??: it has none, so it is in a session of its own, and the window's close ends only what runs in the session its program leads)"
    } else {
        ""
    }
}

/// What the window's close did to a command that ran on once the task was done, as a clause
/// of a problem: whether the close, which ends what its tree finds, found it. None where
/// nothing ran on.
fn after_the_close(left: &Left) -> Option<String> {
    if left.escaped.is_empty() {
        return None;
    }
    Some(match &left.after_close {
        None => "the window had not closed when the run last looked".to_owned(),
        Some(rest) if rest.is_empty() => "the window's close ended it, unsaid".to_owned(),
        Some(rest) => format!(
            "it still runs after the window closed, which found nothing of it: {}",
            rest.join("; ")
        ),
    })
}

/// The run in a line, for the table that ends it: that the command was ended and the
/// chief told, and else the first thing that was wrong with it.
pub fn summary(left: &Left, verdict: &Result<(), Vec<String>>) -> String {
    match verdict {
        Ok(()) if left.listed.is_empty() => format!(
            "T-1 done after {} s; the harness ended the command with its tool call: the host never listed it, and nothing of it runs on the machine",
            secs(left.took)
        ),
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
    run_asked(who, plan, stand_in, &brief(who), log)
}

/// [`run`], the worker given `brief` as its task: the rig's stand-in is asked to leave a
/// job as a harness's shell tool does, and as the agents that detach theirs do.
pub fn run_asked(
    who: Who,
    plan: &Plan,
    stand_in: &Path,
    brief: &str,
    log: &Log,
) -> Fallible<Finished> {
    let log = log.scoped(who.name());
    let mut members = Members::open(plan, stand_in, &log)?;
    let waits = Waits::of(who);
    let finished = observe(&mut members, &agent(who), brief, &wanted(who), waits, &log);
    let closed = members.close();
    let mut finished = finished?;
    finished.case.closed(closed);
    Ok(finished)
}

/// The worker given its task, and the board and the pane host looked at once a second until
/// the task ends; then the result and the chief's note waited for, the command waited to be
/// gone, and what was seen held to the brief. `wanted` is what the command line of the
/// command holds, to find it on the machine.
fn observe(
    members: &mut Members,
    agent: &str,
    brief: &str,
    wanted: &str,
    waits: Waits,
    log: &Log,
) -> Fallible<Finished> {
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
            waits.told,
            LOOK,
            || {
                left.notes = notes_of_work(&members.inbox(Some(members.chief()))?, number);
                Ok((!left.notes.is_empty()).then_some(()))
            },
        )?;
        find(log, "the command to be gone", waits.gone, LOOK, || {
            left.alive = still_there(&left.listed);
            Ok(left.alive.is_empty().then_some(()))
        })?;
        left.escaped = on_the_machine(wanted);
    }
    let mut exit: Option<Tree> = None;
    if let (true, Some(pane)) = (ended, pane.clone()) {
        find(
            log,
            "the worker's window to close",
            waits.gone,
            LOOK,
            || {
                exit = tree::left_at_exit(members.rig(), &pane);
                Ok(exit.is_some().then_some(()))
            },
        )?;
    }
    // A command that escaped the ending is the window's close's to find, or nobody's: what
    // still runs once the window has closed is said, and then ended, so that a run leaves
    // nothing of its own on the machine.
    if !left.escaped.is_empty() && exit.is_some() {
        left.after_close = Some(on_the_machine(wanted));
    }
    end_what_runs_on(wanted);

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

/// Ends what runs the command now, so that a run leaves nothing of its own on the machine.
/// What is ended is what the machine shows running it at this moment, by its command line:
/// a number that an earlier look found, and another process took, is not ended.
fn end_what_runs_on(wanted: &str) {
    for pid in on_the_machine(wanted)
        .iter()
        .filter_map(|line| pid_of(line))
    {
        let _ = signal(pid, Signal::Kill);
    }
}

/// The process a line of [`on_the_machine`] is of: the number it begins with.
fn pid_of(line: &str) -> Option<u32> {
    line.split_whitespace().next()?.parse().ok()
}

/// The command lines of the machine's processes that run `wanted` now ([`runs_command`]:
/// on Windows a `ping` PowerShell started has its path in its line), each after the
/// process's number: `ps` on Unix, which says where each stands too ([`standing`]), the
/// process table through PowerShell on Windows. None where neither answers.
fn on_the_machine(wanted: &str) -> Vec<String> {
    let ran = if cfg!(windows) {
        Run::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                "Get-CimInstance Win32_Process | ForEach-Object { \"$($_.ProcessId) $($_.CommandLine)\" }",
            ])
            .inheriting_env()
            .run()
    } else {
        Run::new("ps")
            .args(["-axo", "pid=,ppid=,pgid=,tty=,command="])
            .inheriting_env()
            .run()
    };
    ran.ok()
        .filter(|ran| ran.code == Some(0))
        .map(|ran| running(&ran.stdout, wanted, cfg!(windows)))
        .unwrap_or_default()
}

/// The lines of `listing`, the process table [`on_the_machine`] read (`windows`'s, or
/// `ps`'s, whose lines say where each stands), of the processes that run `wanted`.
fn running(listing: &str, wanted: &str, windows: bool) -> Vec<String> {
    listing
        .lines()
        .map(str::trim)
        .filter_map(|line| {
            if windows {
                Some(line.to_owned())
            } else {
                standing(line)
            }
        })
        .filter(|line| runs_command(line, wanted))
        .collect()
}

/// A line of `ps -axo pid=,ppid=,pgid=,tty=,command=` as the run says a process: its number
/// and command line, then where it stands: the process that started it (the system's init
/// once that has gone), its process group, and its terminal. A command the window's close
/// finds by the session its program leads has the window's terminal; one with none (`??`)
/// is in a session of its own, which no pane has, and the close does not find it. None for
/// a line that says no command.
fn standing(line: &str) -> Option<String> {
    let mut words = line.split_whitespace();
    let (pid, parent, group, terminal) =
        (words.next()?, words.next()?, words.next()?, words.next()?);
    let command: Vec<&str> = words.collect();
    if command.is_empty() {
        return None;
    }
    Some(format!(
        "{pid} {} (parent {parent}, group {group}, terminal {terminal})",
        command.join(" ")
    ))
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
