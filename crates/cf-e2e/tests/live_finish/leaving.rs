//! A worker that leaves a long command running and answers (the owner's decision of
//! 2026-10-13, after T-7 and T-15 on Windows; turn-end's smallest rule of 2026-10-21): its
//! answer is its task's result at once, and its turn's end ends nothing; its window,
//! free, closes after the result as any does, the close ends what the window's tree runs,
//! and the chief is told what was still running. The worker is asked to start a long command in the
//! background, `sleep 4711.<the run's word>` (`ping -n 4711 -w 4711.<the run's word> 127.0.0.1` on
//! Windows, which has no `sleep` that every one of its shells knows), not to wait for it, and to
//! answer ([`word`]). The run looks, once a second, at the task, the worker's lane of the board and
//! what the pane host lists of its tree (`pane.background`), which is where it learns the command's
//! process (its command line holds the word);
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
//!   command in the window's own tree, the jobs the rig's stand-in wrote it started, and what
//!   holds the run's word as a whole argument and began after the run's window ([`choose`]):
//!   never a process by the command's other words, which anybody's `sleep 4711` holds;
//!   The one shape that does not fail is the known macOS limit, which the run expects of Pi
//!   and Devin on a Mac alone ([`Expect`]; the chief's decision of 2026-11-03, T-1100). Their
//!   tool shells lead sessions of their own, so a job detached with `nohup … &` once its
//!   shell has ended has no session, parent or readable marker that ties it to the window:
//!   the host never lists it and the close does not end it (`work.rs`). It passes when the task
//!   is done and its result delivered, every process of the job has parent 1 and no terminal,
//!   it still ran once the window had closed, and the chief was told nothing; the run then
//!   ends what it can prove its own, an orphan begun after its window that runs in the folder
//!   made for the run ([`Chosen::take_orphans`]), through the descent and by which process
//!   it is. Any other shape fails as above. What the run counts for the command that escaped
//!   is what its window held, an orphan of its own folder begun after the window, and the
//!   stand-in's jobs by the rig's own folder of them, and what holds the run's word
//!   ([`Chosen::of_the_run`]): never a process of the machine that holds the command's words
//!   alone, as other trees' runs and anybody's own `sleep` hold the same (T-592, T-1538); one the
//!   run cannot prove its own is said, and fails nothing;
//! - the command is gone once the window has closed;
//! - and the chief was told, in the note of the window's close, that it closed with the
//!   work still running.
//!
//! Whatever the verdict, the run says what the host listed at the turn's end, what it
//! said the window left when it closed, and what the chief was told.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use cf_e2e::live::members::{
    self, delivered_result, descending_from, end_chosen, find, holding, is_over, notes_of_work,
    orphans_in_folder, working_folder, Account, Ended, Identity, Machine, Members, Plan, RunFolder,
    Running, ThisMachine,
};
use cf_e2e::live::tree::{self, Proc, Tree};
use cf_e2e::live::{cut, secs, text, Harness, Log, Who};
use cf_e2e::process::{is_alive, Run};
use cf_e2e::Result as Fallible;

use crate::check::{
    agent, failure, progress_line, Finished, Progress, CLOSES, DELIVERS, LOOK, WORK,
};

/// Whether `command` has `word` as a whole argument: split at the blanks, one of them is it.
pub fn has_arg(command: &str, word: &str) -> bool {
    command.split_whitespace().any(|arg| arg == word)
}

/// Whether the program of `command` is the one the worker's command is: `sleep`, or on Windows
/// `ping`, which that table lists by the path of its image (`"C:\Windows\system32\PING.EXE" -n
/// 4711 127.0.0.1`), so that the command's own words are not in its line.
fn is_the_command(command: &str) -> bool {
    let first = command.split_whitespace().next().unwrap_or_default();
    let name = first
        .trim_matches('"')
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default();
    let name = name.to_ascii_lowercase();
    matches!(name.strip_suffix(".exe").unwrap_or(&name), "sleep" | "ping")
}

/// How many seconds the command the worker starts runs: more than a run lasts (an hour and
/// more). Anybody's `sleep` may count the same: it is no word of the run's ([`word`]).
pub const fn seconds() -> u64 {
    4711
}

/// The run's own word, made once for the run: [`seconds`], a point, and 64 random bits as twenty
/// digits (`4711.01234567890123456789`). It is a whole argument of the command's own process in
/// every form the worker is given ([`command`], [`detach`]): what `sleep` waits for on a Mac (a
/// fraction it waits for too, less than a second more), and on Windows what `ping` waits for a
/// reply to come back from `127.0.0.1` (`-w`, read up to the point: 4711 ms, where a reply comes in
/// less than one). No process that is not the run's holds it ([`Chosen::by_word`]).
pub fn word() -> &'static str {
    static WORD: OnceLock<String> = OnceLock::new();
    WORD.get_or_init(|| {
        // A version 4 uuid's fixed bits are in a different place in each half: the two halves,
        // one over the other, are 64 random bits.
        let (high, low) = uuid::Uuid::new_v4().as_u64_pair();
        format!("{}.{:020}", seconds(), high ^ low)
    })
}

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
        format!("ping -n {} -w {} 127.0.0.1", seconds(), word())
    } else {
        format!("sleep {}", word())
    }
}

/// How the worker may detach the command when its shell tool has no option to run one in
/// the background.
fn detach() -> String {
    if cfg!(windows) {
        format!(
            "`Start-Process -WindowStyle Hidden ping -ArgumentList '-n','{}','-w','{}','127.0.0.1'` in PowerShell, or `nohup {} > /dev/null 2>&1 &` in a shell that has `nohup`",
            seconds(),
            word(),
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

/// What the chief gives the rig's stand-in: to start a job of [`seconds`] seconds and
/// answer; where `detached`, detached from the shell that runs it, as the agents of four
/// harnesses detached theirs (`nohup … &`), which ends the shell and leaves the job with
/// no parent.
pub fn stand_in_brief(detached: bool) -> String {
    let how = if detached { "--detach " } else { "" };
    format!("BACKGROUND {how}{} left a job running", seconds())
}

/// What the command line of the command the worker started holds, to find it on the
/// machine: the command itself, or for the rig's stand-in, which starts a job of its own,
/// the job's words.
fn wanted(who: Who) -> String {
    match who {
        Who::StandIn => format!("--background-job {}", seconds()),
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
            problems
                .push("the chief was told nothing of the work the window closed with".to_owned());
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
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
/// a session of its own, the known limit on a Mac, or that the window's close ended it and the chief
/// was told. The one [`summary`] tells, by the same shapes: a process of the machine that holds the
/// command's words and is none of this run's (another tree's run of the stand-in, which the run
/// cannot tell from its own) changes neither.
fn passed_as(left: &Left) -> &'static str {
    if (left.listed.is_empty() && !left.escaped.is_empty())
        || listed_then_left_for_a_session_of_its_own(left)
    {
        "the command ran on in a session of its own, as the known limit on a Mac says it may, and the chief was told nothing"
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
    // The rig's stand-in tells its own jobs by their files (the rig's folder of them): its job
    // counts whole seconds and carries no word. A harness's command carries the run's word.
    let mut chosen = Chosen {
        word: matches!(who, Who::Harness(_)).then(|| word().to_owned()),
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

    // What the host's listing of the command holds: the run's word, or the stand-in job's seconds.
    let mark = chosen.word.clone().unwrap_or_else(|| seconds().to_string());
    let mut left = Left::default();
    let mut began: Option<Duration> = None;
    let mut pane: Option<String> = None;
    let mut progress = Progress::default();
    let ended = find(log, "the worker's task to end", WORK, LOOK, || {
        let thread = members.task(number)?;
        let lane = members.lane(agent)?;
        let own = members
            .window_in(lane.clone())
            .and_then(|window| members.window_process(&window));
        // The jobs first: a listing made after a job wrote its process has the job, if it runs.
        let jobs = running_jobs(members);
        match ThisMachine.processes() {
            Ok(all) => chosen.choose(&all, own, &jobs, wanted),
            Err(failed) => chosen.failed.push(failed.to_string()),
        }
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
        Ok((is_over(&thread) || goes_on(members, number)?).then_some(()))
    })?
    .is_some();
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
            left.alive = still_there(&left.listed);
            Ok(left.alive.is_empty().then_some(()))
        })?;
    }
    // What of the machine the run counts for its own is what its window held or an orphan of
    // its own folder since the window began, and for the stand-in's jobs what holds the rig's
    // own folder of them: never what holds the command's words alone (`of_the_run`). A folder is
    // the run's only under the rig's own temp root: a live rig's project folder is in the person's
    // home, and no ground of the run's.
    let word = jobs_folder(members);
    let own_folder = members.rig().run_folder(members.folder());
    look_for_leftovers(&mut left, still_there, || {
        on_the_machine(|all| {
            chosen.of_the_run(
                all,
                (own_folder.as_ref(), wanted, word.as_deref()),
                working_folder,
            )
        })
    });
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
    // still runs once the window has closed is said, and then what the run saw of it in the
    // window is ended, so that a run leaves nothing it can tell its own on the machine. A
    // command a harness detached before any look saw it in the window runs on, and ends by
    // itself when its seconds are out: nothing is ended by what its command line says.
    if !left.escaped.is_empty() && exit.is_some() {
        left.after_close = Some(on_the_machine(|all| {
            chosen.of_the_run(
                all,
                (own_folder.as_ref(), wanted, word.as_deref()),
                working_folder,
            )
        }));
    }
    // What holds this run's own word is the run's wherever it went, and is ended by which process
    // it is, whichever way the case comes out (a command that escaped fails it, [`judge`]).
    if let Ok(all) = ThisMachine.processes() {
        for one in chosen.by_word(&all, wanted) {
            if !chosen
                .processes
                .iter()
                .any(|known| known.identity == one.identity)
            {
                chosen.processes.push(one);
            }
        }
    }
    if looks_for_its_own_orphans(expect, &left) {
        match ThisMachine.processes() {
            Ok(all) => {
                let found = chosen.take_orphans(&all, own_folder.as_ref(), wanted, working_folder);
                if !chosen.unproven.is_empty() {
                    case.say(format!(
                        "an orphan that holds the command's words and is none of this run's by its window, its start and its folder is left as it is, for it may be another run's: {}",
                        chosen.unproven.join("; ")
                    ));
                }
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
    /// The worker's window's process, as the first look at it said: what descends from it is
    /// the window's own, as long as it runs.
    window: Option<Identity>,
    /// The run's own word ([`word`]), which its command's process holds as a whole argument:
    /// what holds it and began after the run's window is the run's wherever it went. None for
    /// the rig's stand-in, whose jobs are told by their files.
    word: Option<String>,
    /// An orphan that holds the command's words and cannot be proved the run's own, each as the
    /// machine lists it: not ended, and not counted for the run either, as it may be any other
    /// run's ([`Chosen::take_orphans`]).
    unproven: Vec<String>,
}

impl Chosen {
    /// Adds what of the command the run ends at its end, as the machine lists its processes
    /// now (`all`): what descends from the worker's window's own process `own` and holds
    /// `wanted`, with what it started ([`holding`]); and the rig's stand-in jobs (`jobs`:
    /// each one's process, as the job wrote it, and its file, which no process but the job
    /// has in its command line, begun no earlier than the window; one the stand-in detached has
    /// no window to descend from).
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
            self.window.get_or_insert(own);
        }
        let in_window = own.map(|own| holding(all, own, wanted)).unwrap_or_default();
        let mut of_jobs = Vec::new();
        for (pid, file) in jobs {
            if self.jobs.contains(file) {
                continue;
            }
            let found = all.iter().find(|process| {
                process.identity.pid == *pid
                    && process.command.contains(file.as_str())
                    && self
                        .window_began
                        .is_some_and(|since| process.identity.started >= since)
            });
            if let Some(process) = found {
                self.jobs.push(file.clone());
                of_jobs.push(process.clone());
            }
        }
        let of_the_word = self.by_word(all, wanted);
        for one in in_window.into_iter().chain(of_jobs).chain(of_the_word) {
            if !self
                .processes
                .iter()
                .any(|known| known.identity == one.identity)
            {
                self.processes.push(one);
            }
        }
    }

    /// What holds the run's own word as a whole argument, and `wanted` or the command's program,
    /// and began no earlier than the run's window, wherever it runs and whoever started it, with
    /// what it started: the command of this run, or the shell that holds its words. The word is
    /// 64 random bits made for the run ([`word`]), which no other process holds; a process that
    /// began before the window is none of the run's whatever it holds. Nothing where the run has
    /// no word (the stand-in's) or has not seen its window (the host names it as it opens). Not
    /// the worker's own window, what it descends from, or another process of its program: the
    /// brief that names the command is in the window's command line too (T-548: a check that
    /// ended what held the words ended the window's TUI).
    fn by_word(&self, all: &[Running], wanted: &str) -> Vec<Running> {
        let (Some(word), Some(since)) = (&self.word, self.window_began) else {
            return Vec::new();
        };
        let program = |process: &Running| {
            process
                .command
                .split_whitespace()
                .next()
                .map(|first| first.rsplit(['/', '\\']).next().unwrap_or(first).to_owned())
        };
        let window = self
            .window
            .and_then(|own| all.iter().find(|process| process.identity == own));
        let mut ancestors: Vec<Identity> = Vec::new();
        if let Some(window) = window {
            let mut at = window.clone();
            while let Some(parent) = all.iter().find(|process| {
                process.identity.pid == at.ppid && process.identity.started <= at.identity.started
            }) {
                if ancestors.contains(&parent.identity) {
                    break;
                }
                ancestors.push(parent.identity);
                at = parent.clone();
            }
        }
        let own_program = window.and_then(program);
        let mut found: Vec<Running> = Vec::new();
        for process in all.iter().filter(|process| {
            has_arg(&process.command, word)
                && process.identity.started >= since
                && (process.command.contains(wanted) || is_the_command(&process.command))
                && Some(process.identity) != self.window
                && !ancestors.contains(&process.identity)
                && !(own_program.is_some() && program(process) == own_program)
        }) {
            for one in
                std::iter::once(process.clone()).chain(descending_from(all, process.identity))
            {
                if !found.iter().any(|known| known.identity == one.identity) {
                    found.push(one);
                }
            }
        }
        found
    }

    /// What of `all` the run counts for the command that escaped, and for nothing else that
    /// holds its words: what descends from the worker's window's process and holds `wanted`
    /// ([`holding`]); an orphan that holds it, began no earlier than the window, and runs in the
    /// run's own `folder`, where it has one under its temp root ([`orphans_in_folder`]); and,
    /// where the run has a word made for it (`word`: the folder of the rig's own jobs, in which no
    /// process of another run holds a file), what holds both `wanted` and `word` and began no
    /// earlier than the window. A process of another tree's run that holds
    /// `wanted` is none of them, whatever else is the same (T-592: another tree's
    /// `--background-job 4711`). Each once.
    fn of_the_run(
        &self,
        all: &[Running],
        (folder, wanted, word): (Option<&RunFolder>, &str, Option<&str>),
        folder_of: impl Fn(u32) -> Option<PathBuf>,
    ) -> Vec<Running> {
        let mut found: Vec<Running> = self
            .window
            .map(|own| holding(all, own, wanted))
            .unwrap_or_default();
        if let (Some(folder), Some(since)) = (folder, self.window_began) {
            found.extend(orphans_in_folder(all, wanted, folder, since, &folder_of));
        }
        found.extend(self.by_word(all, wanted));
        if let (Some(word), Some(since)) = (word, self.window_began) {
            found.extend(
                all.iter()
                    .filter(|process| {
                        process.command.contains(wanted)
                            && process.command.contains(word)
                            && process.identity.started >= since
                    })
                    .cloned(),
            );
        }
        let mut once: Vec<Running> = Vec::new();
        for process in found {
            if !once.iter().any(|known| known.identity == process.identity) {
                once.push(process);
            }
        }
        once
    }

    /// Adds what escaped every window and is provably the run's own, as the machine lists its
    /// processes now (`all`): an orphan that holds `wanted`, began no earlier than the worker's
    /// window did, and runs in the run's own `folder` as `folder_of` says it, with what it
    /// started ([`orphans_in_folder`]): a folder under the run's temp root alone ([`RunFolder`]),
    /// and none where the run has none (a live rig's project folder is in the person's home). A job a tool shell that leads a session of its own
    /// detached, once the shell has ended, has nothing else that ties it to the window; the
    /// folder is the worker's alone, made for the run, and the start tells the job from a
    /// process of an earlier run in it. A command line that says the same is not enough, as it
    /// never was ([`Chosen::choose`]). An orphan that holds `wanted` and is none of these is not
    /// ended, and is none of the run's either: the run says it ([`Chosen::unproven`]) and fails
    /// on none, as it may be any other run's (another tree's, which holds the same words).
    /// Answers what the folder found, each time, whether or not an earlier look had chosen it.
    fn take_orphans(
        &mut self,
        all: &[Running],
        folder: Option<&RunFolder>,
        wanted: &str,
        folder_of: impl Fn(u32) -> Option<PathBuf>,
    ) -> Vec<Running> {
        let found = match (folder, self.window_began) {
            (Some(folder), Some(since)) => {
                orphans_in_folder(all, wanted, folder, since, &folder_of)
            }
            _ => Vec::new(),
        };
        for orphan in found.iter().cloned().chain(self.by_word(all, wanted)) {
            if !self
                .processes
                .iter()
                .any(|chosen| chosen.identity == orphan.identity)
            {
                self.processes.push(orphan);
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
        problems
    }
}

/// The jobs the rig's stand-in agents started that run, each as its process and its file:
/// those that have written their process and neither that they ran to their end nor that
/// they were asked to end, whose files are under the rig's own temp root. None where the windows
/// are a harness's.
fn running_jobs(members: &Members) -> Vec<(u32, String)> {
    members
        .rig()
        .jobs("job")
        .iter()
        .filter(|job| !job.finished() && !job.released())
        .filter(|job| {
            job.file()
                .parent()
                .and_then(|folder| members.rig().run_folder(folder))
                .is_some()
        })
        .map(|job| (job.pid(), job.file().display().to_string()))
        .collect()
}

/// The command lines of the processes of the machine that are the run's own now (what
/// `of_the_run` says of the machine's processes, [`Chosen::of_the_run`]), each after the process's number:
/// `ps` on Unix, which says where each stands too ([`standing`]), the process table through
/// PowerShell on Windows. None where neither answers. A process is not the run's for its
/// command line alone: other trees' runs on this machine hold the same words.
fn on_the_machine(of_the_run: impl Fn(&[Running]) -> Vec<Running>) -> Vec<String> {
    let Ok(all) = ThisMachine.processes() else {
        return Vec::new();
    };
    let own: Vec<Running> = of_the_run(&all);
    if own.is_empty() {
        return Vec::new();
    }
    if cfg!(windows) {
        return own
            .iter()
            .map(|process| format!("{} {}", process.identity.pid, process.command))
            .collect();
    }
    Run::new("ps")
        .args(["-axo", "pid=,ppid=,pgid=,tty=,command="])
        .inheriting_env()
        .run()
        .ok()
        .filter(|ran| ran.code == Some(0))
        .map(|ran| {
            ran.stdout
                .lines()
                .map(str::trim)
                .filter(|line| {
                    let pid = line
                        .split_whitespace()
                        .next()
                        .and_then(|pid| pid.parse::<u32>().ok());
                    pid.is_some_and(|pid| own.iter().any(|process| process.identity.pid == pid))
                })
                .filter_map(standing)
                .collect()
        })
        .unwrap_or_default()
}

/// The folder of the rig's own jobs, as the rig's stand-in names it in the command line of a
/// job it started (`--background-job <seconds> <file>`, the file in this folder alone): a word
/// that is this run's and no other's, under the rig's own temp root. None where the rig has
/// written no job.
fn jobs_folder(members: &Members) -> Option<String> {
    let jobs = members.rig().jobs("job");
    let file = jobs.first()?.file();
    let folder = members.rig().run_folder(file.parent()?)?;
    Some(format!(
        "{}{}",
        folder.given().display(),
        std::path::MAIN_SEPARATOR
    ))
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
