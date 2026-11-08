//! A worker that leaves a long command running and answers (the owner's decision of
//! 2026-10-13, after T-7 and T-15 on Windows; turn-end's smallest rule of 2026-10-21): its
//! answer is its task's result at once, and its turn's end ends nothing; its window,
//! free, closes after the result as any does, the close ends what the window's tree runs,
//! and the chief is told what was still running. The worker is asked to start a long command in the
//! background, `sleep 4711` (`ping -n 4711 127.0.0.1` on Windows, which has no `sleep`
//! that every one of its shells knows), not to wait for it, and to answer. The run
//! looks, once a second, at the task, the worker's lane of the board and what the pane
//! host lists of its tree (`pane.background`), which is where it learns the command's
//! process (its command line holds `4711`, and no other process of the machine's does);
//! and holds what it saw to the brief ([`judge`]):
//!
//! - the task is done, and its result reached the chief; where the worker's harness wakes
//!   it for the command (Claude Code runs it as a job of its own, and the reader counts it:
//!   turn-end's A2), the task is held for it and its chief told it goes on, and the run,
//!   as the chief, finishes it (`cf task finish`), as T-15's chief would: the command never
//!   ends within the run;
//! - the host listed the command as running, or the command is nowhere on the machine
//!   once the task is done: Codex and Devin end what a tool started when the tool call
//!   ends (live, 2026-10-14), and their run says so; a command the host never listed
//!   that still runs on the machine escaped the ending, and fails: the agent detached it
//!   from the shell that started it (`nohup … &`, `setsid`, `disown`, `Start-Process`)
//!   and that shell ended, so no chain of processes that run leads from it to a shell the
//!   harness started, and the host names only what such a chain proves the turn's. The run
//!   then looks again once the window has closed, and says whether the window's close
//!   found it (it ends what its tree finds) or it runs on, and ends what it saw of the
//!   command in the window's own tree and the jobs the rig's stand-in wrote it started
//!   ([`choose`]): never a process by what its command line says, so a command a harness
//!   detached before any look saw it runs on until its seconds are out;
//!   The one shape that does not fail is the known macOS limit, which the run expects of Pi
//!   and Devin on a Mac alone ([`Expect`]; the chief's decision of 2026-11-03, T-1100). Their
//!   tool shells lead sessions of their own, so a job detached with `nohup … &` once its
//!   shell has ended has no session, parent or readable marker that ties it to the window:
//!   the host never lists it and the close does not end it (`work.rs`). It passes when the task
//!   is done and its result delivered, every process of the job has parent 1 and no terminal,
//!   it still ran once the window had closed, and the chief was told nothing; the run then
//!   ends what it can prove its own, an orphan begun after its window that runs in the folder
//!   made for the run ([`Chosen::take_orphans`]), through the descent and by which process
//!   it is. Any other shape fails as above, and so does a job it cannot prove its own, by its
//!   number;
//! - the command is gone once the window has closed;
//! - and the chief was told, in the note of the window's close, that it closed with the
//!   work still running.
//!
//! Whatever the verdict, the run says what the host listed at the turn's end, what it
//! said the window left when it closed, and what the chief was told.

use std::path::{Path, PathBuf};
use std::time::Duration;

use cf_e2e::live::members::{
    self, delivered_result, end_chosen, find, holding, is_over, notes_of_work, orphans_in_folder,
    working_folder, Account, Ended, Identity, Machine, Members, Plan, Running, ThisMachine,
};
use cf_e2e::live::tree::{self, Proc, Tree};
use cf_e2e::live::{cut, secs, text, Harness, Log, Who};
use cf_e2e::process::{is_alive, Run};
use cf_e2e::Result as Fallible;
use looks::Looks;

use crate::check::{
    agent, failure, progress_line, Finished, Progress, CLOSES, DELIVERS, LOOK, WORK,
};

/// How many seconds the command the worker starts runs: more than a run lasts, and a
/// number that is in no other command line of the machine.
pub const SECONDS: u64 = 4711;

/// What the run expects of a worker that detaches the command: that the window's close ends
/// it where the tree holds it, or the known macOS limit of two harnesses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Expect {
    /// The command is gone with the window, or the harness ended it, and the chief was told:
    /// a command that escaped fails the run.
    Ended,
    /// A tool shell that leads a session of its own detached it (`nohup … &`), as Pi's and
    /// Devin's do on macOS: nothing ties it to the window, the host never lists it and the
    /// close does not end it (the limit `process_tree` and `work.rs` write). The run holds
    /// that to be so, and ends what it can prove its own.
    OwnSession,
}

impl Expect {
    /// What a worker of `who` is expected to come to: the limit for Pi and Devin on a Mac,
    /// where Windows' job holds what they detach, and the ending for the rest.
    pub fn of(who: Who) -> Self {
        match who {
            Who::Harness(Harness::Pi | Harness::Devin) if cfg!(target_os = "macos") => {
                Self::OwnSession
            }
            _ => Self::Ended,
        }
    }
}

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
    /// The task was held for the command, its chief told it goes on, and the run finished
    /// it as the chief.
    pub finished: bool,
    /// What the chief was told of work the turn left, the notes that say so.
    pub notes: Vec<String>,
    /// The processes of `listed` that were still there when the run last looked, once
    /// the task was done, the window closed and the chief told.
    pub alive: Vec<u64>,
    /// The processes of `listed` that were gone when the run looked at the machine once it saw
    /// the task over: a process the turn itself ended (Codex's PowerShell wrapper of a tool
    /// call lives under a second, and the host's look, once a second, may fall on it), which
    /// the close has nothing of to end and the chief is owed no note of. The task may be seen
    /// over after its window has closed, so this alone does not tell a process the turn ended
    /// from one the close ended: `at_exit` does (what the pane host found in the window at the
    /// close). Empty where the run did not look, which holds the run to the note.
    pub gone_in_the_turn: Vec<u64>,
    /// The processes of the command the pane host said the window left running when it
    /// closed (its `pane.exit`): what a close ended, whatever came of the listed ones.
    pub at_exit: Vec<Proc>,
    /// The command lines of the machine's processes that run the command, looked at once
    /// the task was done and the chief told: what escaped the ending, where the host
    /// listed none, each with where it stands on a Unix machine ([`standing`]).
    pub escaped: Vec<String>,
    /// What of `escaped` still ran when the window had closed, which ends what its tree
    /// finds: none where the close ended it unsaid. None where nothing escaped, or the
    /// window had not closed when the run last looked.
    pub after_close: Option<Vec<String>>,
}

/// The task is done, its result reached the chief, the host listed the command as
/// running, it is gone, and the chief was told the window closed with it running: or
/// every way it was not.
pub fn judge(left: &Left, expect: Expect) -> Result<(), Vec<String>> {
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
    // When what the turn left was looked for: once the task was done and the chief told, or
    // when the run last looked at a task that came to anything else.
    let done = left.state == "done";
    let when = |once: &str| {
        if done {
            format!("{once} the task was done and the chief told")
        } else {
            format!("when the run last looked, T-1 being {}", left.state)
        }
    };
    if left.listed.is_empty() {
        if left.escaped.is_empty() {
            // The harness ended the command with its tool call: nothing of it runs.
        } else if expect == Expect::OwnSession && in_a_session_of_its_own(left) {
            // The known limit, expected of this harness on this machine: the job is in a
            // session of its own, its shell has gone, and nothing ties it to the window. The
            // chief is told nothing of it, and a note that says otherwise says what the close
            // did not do.
            if !left.notes.is_empty() {
                problems.push(format!(
                    "the chief was told of work the window closed with ({}), while the command the agent detached into a session of its own went on running after the window closed, which found nothing of it: {}; if the host names such a job now, the limit is out of date, and so is the expectation that lets it pass (`Expect::OwnSession`)",
                    left.notes.join(" | "),
                    left.escaped.join("; ")
                ));
            }
        } else {
            problems.push(format!(
                "the command escaped the ending: the pane host never listed it, and it still ran {} (the agent detached it from the shell that started it, which ended, so no chain of processes that run led from it to a shell the harness started): {}; {}{}",
                when("once"),
                left.escaped.join("; "),
                close.unwrap_or_default(),
                without_a_terminal(&left.escaped)
            ));
        }
    } else if expect == Expect::OwnSession && listed_then_left_for_a_session_of_its_own(left) {
        // The same limit, where a look of the host's fell while the job's shell still tied it to
        // the window: the host listed it, then the shell ended and left it in a session of its
        // own, which the close did not find. Where it is after the close decides, not whether
        // a look saw it on its way out (GitHub's macOS runner, CI runs 41772376690 and
        // 41781145403).
        if !left.notes.is_empty() {
            problems.push(format!(
                "the chief was told of work the window closed with ({}), while the command the agent detached, which the host listed before its shell ended, went on running after the window closed in a session of its own, which the close found nothing of: {}",
                left.notes.join(" | "),
                left.escaped.join("; ")
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
                "the command is still running {}: {}",
                when("after"),
                still.join("; ")
            );
            if let Some(close) = &close {
                said.push_str("; ");
                said.push_str(close);
            }
            problems.push(said);
        }
        if left.notes.is_empty() {
            // The host looks once a second, and a look may have fallen on the shell that
            // started the command, which lists with the command's words in its own and ends a
            // moment later: the command it started and detached ran on, and the host never
            // listed it. Nothing listed is there now, and the close found nothing: that is the
            // escape too, said as it is where no look fell in the shell (CI run 42425306008).
            let unlisted = unlisted_but_running(left);
            if !unlisted.is_empty() {
                problems.push(format!(
                    "the command escaped the ending: the pane host listed only what ended before the run last looked ({}), and the command that started still ran {} without ever being listed (the agent detached it from the shell that started it, which ended, so no chain of processes that run led from it to a shell the harness started): {}; {}{}",
                    listed_pids(&left.listed),
                    when("once"),
                    unlisted.join("; "),
                    close.unwrap_or_default(),
                    without_a_terminal(&unlisted)
                ));
            }
            // A process the turn itself ended, before the task did, leaves the window's close
            // nothing to end and the chief nothing to be told (Codex's PowerShell wrapper of a
            // tool call, which the host's look may fall on: T-404, 4 runs of 12). What was still
            // there when the run saw the task over, what the pane host found in the window at
            // its close, and a command that ran on are held to the note as they always were.
            if !ended_in_the_turn(left) {
                problems.push(
                    "the chief was told nothing of the work the window closed with".to_owned(),
                );
            }
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

/// Whether an orphan of the command that the run cannot prove its own fails the run: one of a real
/// harness's run does, as a stray that may be an earlier run's or an agent's that left its folder.
/// The rig's stand-in tells its own jobs by their files, and other trees' runs of it on the same
/// machine have jobs of the same number, which are theirs: they are not strays of this run.
fn strays_matter(who: Who) -> bool {
    matches!(who, Who::Harness(_))
}

/// Whether the run looks for what it can prove its own among the processes that escaped every
/// window: where it expects the known macOS limit, and something ran on.
fn looks_for_its_own_orphans(expect: Expect, left: &Left) -> bool {
    expect == Expect::OwnSession && !left.escaped.is_empty()
}

/// Whether the host listed something of the command and what still runs of it is, after the
/// window's close, in a session of its own ([`in_a_session_of_its_own`]): a job the host saw
/// while its shell tied it to the window, which then left with the shell; or, where nothing
/// listed was still there and the chief was told nothing, the shell alone, which a look fell on
/// before it started the job (the job it started then is the one that runs on, and the host never
/// listed it). A listed process still tied to the window (one with a terminal, or a living
/// parent) is no such job, and nor is a close that told the chief of work it ended.
fn listed_then_left_for_a_session_of_its_own(left: &Left) -> bool {
    in_a_session_of_its_own(left)
        && if left.alive.is_empty() {
            left.notes.is_empty() && unlisted_but_running(left).len() == left.escaped.len()
        } else {
            left.alive.iter().all(|pid| {
                left.escaped
                    .iter()
                    .any(|line| line.starts_with(&format!("{pid} ")))
            })
        }
}

/// What of `left.escaped` the host never listed: the processes of the command that ran on
/// whose numbers are none of those it listed.
fn unlisted_but_running(left: &Left) -> Vec<String> {
    left.escaped
        .iter()
        .filter(|line| {
            !left
                .listed
                .iter()
                .any(|process| line.starts_with(&format!("{} ", process.pid)))
        })
        .cloned()
        .collect()
}

/// Whether everything the host listed of the command ended in the turn, before the task did,
/// and the chief was told nothing, as it should be: nothing escaped, the pane host found none of
/// the command in the window when it closed, and each process listed was gone when the run looked
/// once it saw the task over ([`gone_of`]). A run that did not look has none gone, so it is held
/// to the note.
fn ended_in_the_turn(left: &Left) -> bool {
    !left.listed.is_empty()
        && left.notes.is_empty()
        && left.escaped.is_empty()
        && left.at_exit.is_empty()
        && left
            .listed
            .iter()
            .all(|process| left.gone_in_the_turn.contains(&process.pid))
}

/// The numbers of the processes the host listed, for a line of the verdict.
fn listed_pids(listed: &[Proc]) -> String {
    listed
        .iter()
        .map(|process| process.pid.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Whether what the host never listed (`judge` asks only of a command it did not list and that
/// ran on) is the known macOS limit and nothing else: each process of it has no terminal (a
/// session of its own, which no pane has) and its parent is init's (the shell that started it has
/// gone), and it still ran once the window had closed, which found nothing of it. One that has a
/// terminal is in the window's session, and one that has a parent is a shell's that still runs:
/// the window's close finds those, so they are no limit. In the limit the shell, the session and
/// the marker that would tie the job to the window are all gone, and a system binary shows no
/// environment (`process_tree`, `work.rs`).
fn in_a_session_of_its_own(left: &Left) -> bool {
    left.escaped
        .iter()
        .all(|line| line.contains("(parent 1, ") && line.ends_with(", terminal ??)"))
        && left
            .after_close
            .as_ref()
            .is_some_and(|rest| !rest.is_empty())
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

/// The run in a line, for the table that ends it: that the command was ended by the
/// window's close and the chief told, and else the first thing that was wrong with it.
pub fn summary(left: &Left, verdict: &Result<(), Vec<String>>) -> String {
    match verdict {
        Ok(()) if left.listed.is_empty() && !left.escaped.is_empty() => format!(
            "T-1 done after {} s; the agent detached the command into a session of its own, the known limit on a Mac: the host never listed it, the window's close left it running and the chief was told nothing; still running: {}; the run ends what it can prove its own",
            secs(left.took),
            left.escaped.join("; ")
        ),
        Ok(()) if left.listed.is_empty() => format!(
            "T-1 done after {} s; the harness ended the command with its tool call: the host never listed it, and nothing of it runs on the machine",
            secs(left.took)
        ),
        Ok(()) if listed_then_left_for_a_session_of_its_own(left) => format!(
            "T-1 done after {} s; the agent detached the command into a session of its own, the known limit on a Mac: the host listed it before its shell ended, the window's close left it running and the chief was told nothing; still running: {}; the run ends what it can prove its own",
            secs(left.took),
            left.escaped.join("; ")
        ),
        Ok(()) if ended_in_the_turn(left) => format!(
            "T-1 done after {} s; the host listed the command's process ({}), which ended in the turn, before the task did: nothing of it was left for the window's close, and the chief was owed no note",
            secs(left.took),
            listed_pids(&left.listed)
        ),
        Ok(()) => format!(
            "T-1 done after {} s{}; the command ({}) was ended by the window's close and the chief told: {}",
            secs(left.took),
            if left.finished {
                ", held for it and finished by the chief"
            } else {
                ""
            },
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

/// What a run that passed is said to have shown, as the name of its check: that the command ran on in
/// a session of its own, the known limit on a Mac, that it ended in the turn and the chief was owed no
/// note, or that the window's close ended it and the chief was told. The one [`summary`] tells, by the
/// same shapes: a process of the machine that holds the command's words and is none of this run's
/// (another tree's run of the stand-in, which the run cannot tell from its own) changes neither.
fn passed_as(left: &Left) -> &'static str {
    if (left.listed.is_empty() && !left.escaped.is_empty())
        || listed_then_left_for_a_session_of_its_own(left)
    {
        "the command ran on in a session of its own, as the known limit on a Mac says it may, and the chief was told nothing"
    } else if ended_in_the_turn(left) {
        "the command ended in the turn, before the task did, and the chief was owed no note"
    } else {
        "the command was ended by the window's close after the result, and the chief told"
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
/// ended by the window's close and the chief told so.
pub fn run(who: Who, plan: &Plan, stand_in: &Path, log: &Log) -> Fallible<Finished> {
    run_asked(who, plan, stand_in, &brief(who), Expect::of(who), log)
}

/// [`run`], the worker given `brief` as its task: the rig's stand-in is asked to leave a
/// job as a harness's shell tool does, and as the agents that detach theirs do.
pub fn run_asked(
    who: Who,
    plan: &Plan,
    stand_in: &Path,
    brief: &str,
    expect: Expect,
    log: &Log,
) -> Fallible<Finished> {
    let log = log.scoped(who.name());
    let mut members = Members::open(plan, stand_in, &log)?;
    let waits = Waits::of(who);
    // What the run chose to end is ended whatever came of the run, and what could not be
    // ended, or looked for, fails it (Astra and Sol on round 6).
    let mut chosen = Chosen {
        strays_matter: strays_matter(who),
        ..Chosen::default()
    };
    let finished = observe(
        &mut members,
        (&agent(who), brief, &wanted(who)),
        waits,
        expect,
        &log,
        &mut chosen,
    );
    let ended = end_chosen(&chosen.processes);
    let closed = members.close();
    let mut finished = finished.unwrap_or_else(Finished::failed);
    for problem in chosen.problems(&ended) {
        finished.case.fail(problem);
    }
    finished.case.closed(closed);
    Ok(finished)
}

/// The worker `agent` given its task (`brief`), and the board and the pane host looked at
/// once a second until the task ends; then the result and the chief's note waited for, the
/// command waited to be gone, and what was seen held to the brief. `wanted` is what the
/// command line of the command holds, to find it on the machine. What the run is to end of
/// it is in `chosen`.
fn observe(
    members: &mut Members,
    (agent, brief, wanted): (&str, &str, &str),
    waits: Waits,
    expect: Expect,
    log: &Log,
    chosen: &mut Chosen,
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
    let mut looks = Looks::default();
    let mut began: Option<Duration> = None;
    let mut pane: Option<String> = None;
    let mut progress = Progress::default();
    let ended = find(log, "the worker's task to end", WORK, LOOK, || {
        let lane = members.lane(agent)?;
        let own = members
            .window_in(lane.clone())
            .and_then(|window| members.window_process(&window));
        // The jobs first: a listing made after a job wrote its process has the job, if it runs.
        let jobs = running_jobs(members);
        // The machine twice, the pane host's look between the two, and the task last: a task
        // read as not over after them says the turn still ran when they were taken, which
        // `Looks` holds what it takes from them to (the window closes after the task is over).
        let before = ThisMachine.processes();
        match &before {
            Ok(all) => chosen.choose(all, own, &jobs, wanted),
            Err(failed) => chosen.failed.push(failed.to_string()),
        }
        let listing = (!lane["pane"].is_null()).then(|| tree::read(members.rig(), &lane["pane"]));
        let after = ThisMachine.processes();
        let thread = members.task(number)?;
        let over = is_over(&thread) || goes_on(members, number)?;
        looks.look(before.as_deref().ok(), after.as_deref().ok(), !over, &mark);
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
        Ok(over.then_some(()))
    })?
    .is_some();
    // Which of the listed processes the looks showed ending while the turn ran: what the turn
    // ended needs no note. What the window's close ended is told by `at_exit` (below), and what
    // neither shows is held to the note.
    left.gone_in_the_turn = looks.gone_of(&left.listed);
    // Held for the command, which never ends within the run: the chief finishes the task.
    if ended && left.state == "working" {
        let finish = format!("T-{number}");
        let ran = members.as_chief(["task", "finish", finish.as_str()])?;
        case.say(format!(
            "T-{number} was held for the command and its chief told it goes on; the run finished it as the chief: {}",
            cut(&ran.stdout, 160)
        ));
        left.finished = ran.code == Some(0);
        find(log, "the finished task to be done", DELIVERS, LOOK, || {
            left.state = text(&members.task(number)?["state"]);
            Ok((left.state == "done").then_some(()))
        })?;
    }
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
        // The chief is told what the window still ran when its close, after the result,
        // ended it.
        find(
            log,
            "the chief to be told what the window closed with",
            waits.told,
            LOOK,
            || {
                left.notes = notes_of_work(&members.inbox(Some(members.chief()))?, number);
                Ok((!left.notes.is_empty()).then_some(()))
            },
        )?;
        find(log, "the command to be gone", waits.gone, LOOK, || {
            left.alive = alive_listed(&looks, &left.listed, &mark);
            Ok(left.alive.is_empty().then_some(()))
        })?;
    }
    look_for_leftovers(
        &mut left,
        |listed| alive_listed(&looks, listed, &mark),
        || on_the_machine(wanted),
    );
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
    left.at_exit = held_at_exit(exit.as_ref(), &mark);
    // A command that escaped the ending is the window's close's to find, or nobody's: what
    // still runs once the window has closed is said, and then what the run saw of it in the
    // window is ended, so that a run leaves nothing it can tell its own on the machine. A
    // command a harness detached before any look saw it in the window runs on, and ends by
    // itself when its seconds are out: nothing is ended by what its command line says.
    if !left.escaped.is_empty() && exit.is_some() {
        left.after_close = Some(on_the_machine(wanted));
    }
    if looks_for_its_own_orphans(expect, &left) {
        match ThisMachine.processes() {
            Ok(all) => {
                let found = chosen.take_orphans(&all, members.folder(), wanted, working_folder);
                if !found.is_empty() {
                    case.say(format!(
                        "the run found what the agent detached in its own folder, an orphan that began after the worker's window: {}; it ends it by which process it is",
                        found
                            .iter()
                            .map(|process| format!(
                                "{} {}",
                                process.identity.pid,
                                cut(&process.command, 60)
                            ))
                            .collect::<Vec<_>>()
                            .join("; ")
                    ));
                }
            }
            Err(failed) => chosen.failed.push(failed.to_string()),
        }
    }

    let verdict = judge(&left, expect);
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
        Ok(()) => case.check(passed_as(&left), true, summary(&left, &verdict)),
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

/// The chief of `number` was told its worker goes on in the background: the task is
/// held for what its turn left running.
fn goes_on(members: &Members, number: i64) -> Fallible<bool> {
    let opening = format!("T-{number} is still working in the background");
    Ok(members.inbox(Some(members.chief()))?.iter().any(|message| {
        message["kind"] == "note"
            && message["body"]
                .as_str()
                .is_some_and(|body| body.starts_with(&opening))
    }))
}

/// What the turn left, looked for whatever came of the task: one that failed or ran out of
/// its time leaves its command as one that was done does (Astra on round 6). Of the
/// processes the host listed, those `still` there, where the task was not done (one that
/// was has waited for them already); and the machine's processes that run the command
/// (`on_the_machine`).
fn look_for_leftovers(
    left: &mut Left,
    still: impl Fn(&[Proc]) -> Vec<u64>,
    on_the_machine: impl Fn() -> Vec<String>,
) {
    if left.state != "done" {
        left.alive = still(&left.listed);
    }
    left.escaped = on_the_machine();
}

/// What the run ends at its end, as it saw it ([`Chosen::choose`]), and the looks at the
/// machine's processes that failed.
#[derive(Debug, Default)]
struct Chosen {
    /// The processes, each by which process it is ([`end_chosen`]).
    processes: Vec<Running>,
    /// The rig's stand-in jobs looked at already, by their files.
    jobs: Vec<String>,
    /// Why a look at the machine's processes failed, each time one did.
    failed: Vec<String>,
    /// When the worker's window began, as the first look at its process said.
    window_began: Option<u64>,
    /// What escaped the window and cannot be proved the run's own, each as the machine lists it.
    unproven: Vec<String>,
    /// Whether `unproven` is a problem of the run ([`strays_matter`]).
    strays_matter: bool,
}

impl Chosen {
    /// Adds what of the command the run ends at its end, as the machine lists its processes
    /// now (`all`): what descends from the worker's window's own process `own` and holds
    /// `wanted`, with what it started ([`holding`]); and the rig's stand-in jobs (`jobs`:
    /// each one's process, as the job wrote it, and its file, which no process but the job
    /// has in its command line; one the stand-in detached has no window to descend from).
    /// A job is taken, as the process it is then, at the first look that finds the process
    /// of its number holding its file, which is when the rig knows it is its own, and never
    /// looked at again: a job that has gone leaves its number written, and another process
    /// that has the number since holds no file of this rig's, and is never taken for it
    /// (Astra and Sol on round 6). One a look does not find (its parent went between two
    /// reads of the machine) is looked for again. A process outside the window that no job
    /// of the rig's is, is none of it, whatever its command line says.
    fn choose(
        &mut self,
        all: &[Running],
        own: Option<Identity>,
        jobs: &[(u32, String)],
        wanted: &str,
    ) {
        if let Some(own) = own {
            // The window's process is read once as the host answered the open: its start is kept.
            self.window_began.get_or_insert(own.started);
        }
        let in_window = own.map(|own| holding(all, own, wanted)).unwrap_or_default();
        let mut of_jobs = Vec::new();
        for (pid, file) in jobs {
            if self.jobs.contains(file) {
                continue;
            }
            let found = all.iter().find(|process| {
                process.identity.pid == *pid && process.command.contains(file.as_str())
            });
            if let Some(process) = found {
                self.jobs.push(file.clone());
                of_jobs.push(process.clone());
            }
        }
        for one in in_window.into_iter().chain(of_jobs) {
            if !self
                .processes
                .iter()
                .any(|known| known.identity == one.identity)
            {
                self.processes.push(one);
            }
        }
    }

    /// Adds what escaped every window and is provably the run's own, as the machine lists its
    /// processes now (`all`): an orphan that holds `wanted`, began no earlier than the worker's
    /// window did, and runs in the run's own `folder` as `folder_of` says it, with what it
    /// started ([`orphans_in_folder`]). A job a tool shell that leads a session of its own
    /// detached, once the shell has ended, has nothing else that ties it to the window; the
    /// folder is the worker's alone, made for the run, and the start tells the job from a
    /// process of an earlier run in it. A command line that says the same is not enough, as it
    /// never was ([`Chosen::choose`]). An orphan that holds `wanted` and is none of these is not
    /// ended, and the run says so ([`Chosen::problems`]): it may be another run's. Answers
    /// what the folder found, each time, whether or not an earlier look had chosen it.
    fn take_orphans(
        &mut self,
        all: &[Running],
        folder: &Path,
        wanted: &str,
        folder_of: impl Fn(u32) -> Option<PathBuf>,
    ) -> Vec<Running> {
        let found = match self.window_began {
            Some(since) => orphans_in_folder(all, wanted, folder, since, &folder_of),
            None => Vec::new(),
        };
        for orphan in &found {
            if !self
                .processes
                .iter()
                .any(|chosen| chosen.identity == orphan.identity)
            {
                self.processes.push(orphan.clone());
            }
        }
        self.unproven = all
            .iter()
            .filter(|process| process.ppid == 1 && process.command.contains(wanted))
            .filter(|process| {
                !self
                    .processes
                    .iter()
                    .any(|chosen| chosen.identity == process.identity)
            })
            .map(|process| format!("{} {}", process.identity.pid, process.command))
            .collect();
        found
    }

    /// What may still run of what the run chose, a problem of the run each: what could not
    /// be ended (`ended`), and the looks that could not read the machine's processes, which
    /// may have missed what the window ran then.
    fn problems(&self, ended: &Ended) -> Vec<String> {
        let mut problems: Vec<String> = ended
            .problems()
            .into_iter()
            .map(|problem| format!("the command: {problem}"))
            .collect();
        if let Some(first) = self.failed.first() {
            problems.push(format!(
                "the machine's processes could not be read at {} look(s) while the task was watched, and what the window ran then may still run: {first}",
                self.failed.len()
            ));
        }
        if self.strays_matter && !self.unproven.is_empty() {
            problems.push(format!(
                "the command escaped the window and cannot be proved the run's own (only an orphan that began after the worker's window and runs in the run's folder is), so it was not ended and may still run: {}",
                self.unproven.join("; ")
            ));
        }
        problems
    }
}

/// The jobs the rig's stand-in agents started that run, each as its process and its file:
/// those that have written their process and neither that they ran to their end nor that
/// they were asked to end. None where the windows are a harness's.
fn running_jobs(members: &Members) -> Vec<(u32, String)> {
    members
        .rig()
        .jobs("job")
        .iter()
        .filter(|job| !job.finished() && !job.released())
        .map(|job| (job.pid(), job.file().display().to_string()))
        .collect()
}

/// The command lines of the machine's processes whose command holds `wanted` now, each
/// after the process's number: `ps` on Unix, which says where each stands too
/// ([`standing`]), the process table through PowerShell on Windows. None where neither
/// answers.
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
        .map(|ran| {
            ran.stdout
                .lines()
                .map(str::trim)
                .filter_map(|line| {
                    if cfg!(windows) {
                        Some(line.to_owned())
                    } else {
                        standing(line)
                    }
                })
                .filter(|line| line.contains(wanted))
                .collect()
        })
        .unwrap_or_default()
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

/// What of the command the pane host said the window left running when it closed: the processes
/// of `exit` whose command line holds `mark`, none where the host said nothing.
fn held_at_exit(exit: Option<&Tree>, mark: &str) -> Vec<Proc> {
    exit.map(|tree| tree.holding(mark).into_iter().cloned().collect())
        .unwrap_or_default()
}

/// The processes of `listed` that are still on the machine, each told by when it began where the
/// run saw it ([`Looks::alive_of`]); by its number alone where the machine does not answer.
fn alive_listed(looks: &Looks, listed: &[Proc], mark: &str) -> Vec<u64> {
    match ThisMachine.processes() {
        Ok(all) => looks.alive_of(listed, &all, mark),
        Err(_) => still_there(listed),
    }
}

/// The processes of `listed` that are still running, by their numbers.
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

// What the looks show of the command's processes, and the tests, are in `leaving/`: a module
// that a path names looks for its own parts beside it, so these are named as well.
#[path = "leaving/looks.rs"]
mod looks;
#[cfg(test)]
#[path = "leaving/tests.rs"]
mod tests;
