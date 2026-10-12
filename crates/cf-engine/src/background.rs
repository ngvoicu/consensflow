//! Background work: what a worker's window started that outlives its turn. An
//! agent can leave a job running and end its turn (its harness lets it, and
//! wakes it when the job ends), so the end of a turn is a task's result only
//! once nothing the turn started is still running. The rules, which the owner
//! decided on 2026-10-09 after a 30-minute recording run died with the window
//! its worker's turn had ended:
//!
//! - The pane host keeps the processes a window's tree had when a turn began
//!   ([`BackgroundWork::mark`]: just before a message goes into the window,
//!   and for a launch's first message once its header shows in the window's
//!   record, which is the harness's own start of the turn) and says which others
//!   it has now ([`BackgroundWork::running`]). What the window had before the
//!   turn (the harness, its servers) is not the turn's.
//! - A worker's window at rest with its task working and such work running is
//!   not at rest: the task stays working, the board says why, and the chief is
//!   given no result. There is no limit to how long the work may run: a window
//!   is never stopped for how long it works, and pausing or cancelling the task
//!   still closes it.
//! - When the work has ended the harness may wake its agent, and the turn it
//!   wakes is the one whose end is the result, by the usual rules. If no turn
//!   begins within [`Limits::background_settle_ms`] of the work's end, the last
//!   turn's end is the result.
//! - The agents and workflows a turn started in its harness are work the turn
//!   left running too, which the pane host cannot see (they are no process of the
//!   window's tree): the harness says so on its record, on the reading of a turn
//!   that ended with some pending ([`Observed::agents_pending`], Claude Code's own
//!   counts, decided on 2026-10-11 after a chief's result waited twelve minutes
//!   for its agents). Such a turn's end is no result, however long they run: an
//!   agent's end wakes a turn of its own, and no settle time ends the wait. That
//!   is the whole of what agents change here: a window whose turn ended with them
//!   pending takes input like any at rest, a chief's and a worker's alike, and
//!   what the chief sends a worker in the meantime (an answer, a resume, a tell)
//!   goes in between its turns (the harness's adapter says it is at rest; no
//!   delivery looks at them).
//! - A window that closes with such work running says so to whoever gave the
//!   task: the host lists what it ended in `pane.exit`. On Unix the host asks
//!   the work to end first (TERM, then HUP) and gives it a grace to release what
//!   it holds, so a job that handles the request ends cleanly.
//!
//! One window's mark is not renewed while its result is held back: a message
//! that reaches it then (the chief asks how it goes) is no new start of the
//! work the task's turn began.
//!
//! What the harness itself starts in the turn is no work the agent left. Codex
//! starts `codex-code-mode-host` the first time its agent runs a command, and
//! keeps it for the session: a program of Codex's own, beside its binary, which
//! the host sees begin after the mark like any job. A process named for the
//! window's harness (`<harness>-…`) is the harness's, and is not counted.
//!
//! So are the servers a harness runs, and what they start, whenever each begins
//! (found on 3.0.1: Claude Code's MCP servers, which it starts as it takes its
//! first message, held a worker's task for ever; so did the children of a Codex
//! designer's plugin servers). The host cannot tell them by when they began, and
//! tells them by where the window's harness is known to keep them
//! (`Adapter::servers`, told to the host with each mark), in one of two shapes:
//!
//! - Claude Code and Pi keep their servers in the process group of the window's
//!   program, and run each command their agents start in a group of its own, under
//!   the program. A process in the window's group is the harness's, and so is
//!   whatever descends from one, in whatever group: the browser an MCP server
//!   launches and detaches.
//! - Codex opens its servers with the window, each in a group of its own, under
//!   its own process, which is in the window's group and starts what its agent
//!   runs. A process outside the window's group that the window had when its first
//!   turn began is a server, and so is whatever descends from one.
//!
//! The window's program is no ancestor that counts: it is in the window's group,
//! and would make everything the harness's. A harness not known to keep its
//! servers in either way (OpenCode, Devin, and all on Windows) is told by time
//! alone, as it was.
//!
//! The board says which processes the window is held for, the first two of them:
//! `background work running (2 processes): python3 lock.py record; tail -f run.log`,
//! with a key or a token in a command hidden, as a note hides it.
//!
//! Limits. **On Windows a job the agent left running is ended with its window at
//! once, with no chance to release what it holds**: the host asks nothing there
//! and its job object ends the tree. The Ctrl+Break that would ask it escapes the
//! pane's pseudoconsole when the window's program is behind a `.cmd` shim (as npm
//! installs every harness), reaches the host's own console and ends the host. The
//! hold, the board's line and the note to whoever gave the task are the same on
//! every platform: they do not depend on how the window closes.
//!
//! **On Windows a harness's servers that begin after the first mark still hold the
//! first task's result**, and so does what they start: a job object has no process
//! groups and no parents to look at, so no harness says how it keeps its servers
//! there and the host tells by time alone.
//!
//! On Unix the host looks at who started each process, and a process whose parent
//! has gone has no one to look at: it is told by its own place alone, so a browser
//! that outlives the server that launched it, or a daemon a server's child
//! double-forks, is the turn's work unless it stayed in the window's group.
//! A Codex server that begins after the window's first turn began (a plugin it
//! starts only when a tool is called) is no server of its launch, and what it
//! starts is the turn's. Nor can a program of Codex's own that begins with the
//! window, outside its group, be told from a server (`codex-code-mode-host` begins
//! with the first command): what it started would be the harness's.
//!
//! Limits of the agents' part. The reading says a turn left agents pending until
//! the window's next turn says otherwise, and nothing else says they ended:
//!
//! - Agents the human ended by hand, with no turn to follow, keep the task held
//!   until the human pauses or cancels it.
//! - A pause or a cancel is paid by a window at rest with no key, as for a job,
//!   and the agents go on; closing the window ends them, with no note of them (a
//!   note lists the processes the pane host saw).
//! - A turn the human interrupts says nothing of agents (its record is the
//!   interrupt's), so the window does not read at rest until they end.
//! - A switch of the chief after its turn waits for a turn that leaves none
//!   (`chief_switch`); a switch now closes the window and ends them.
//!
//! A launch's first message is marked when it shows in the window's record,
//! which the engine sees on a pass (a second apart): work an agent starts before
//! that is in the window's mark, and is not held for. A look that finds a
//! message arrived and its turn already over takes the result without asking the
//! host, as the turn took less than a pass. Agents take longer than that to
//! start anything. And a long-lived helper the harness starts in the turn itself
//! (a language server, a dev server) is work the turn started, and holds the
//! task until the human pauses or cancels it.

use cf_harness::contract::{Observed, Pane};
use cf_ledger::{NewNote, ParticipantView, ProjectView};
use cf_proto::agents::Harness;
use cf_proto::panes::{Background, BackgroundProcess};

use crate::dispatcher::Dispatcher;
use crate::record::Record;
use crate::seams::EngineError;
use crate::shown::mask::mask;

/// The most commands a note quotes of what a window left running.
const QUOTED: usize = 5;
/// The most characters of one command a note quotes.
const COMMAND_CHARS: usize = 160;
/// The most commands the board names of what a window is held for, and the most
/// characters of each: its line is short, and its count says how many there are.
const NAMED: usize = 2;
const NAMED_CHARS: usize = 60;

/// What a window holds its task's result back for, while it does.
#[derive(Debug, Default)]
pub(crate) struct BackgroundPart {
    held: Option<Held>,
}

/// The work a window's task is held for: how much of it ran when it was last
/// looked at, the first of it as the board names it, and since when it has
/// ended, if it has.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Held {
    running: usize,
    named: String,
    ended_at: Option<i64>,
}

impl Held {
    fn left(&self) -> Left {
        match self.ended_at {
            None => Left::Running {
                count: self.running,
                named: self.named.clone(),
            },
            Some(_) => Left::Ended,
        }
    }
}

/// What a settled worker window's turn left, as far as its task's result goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Left {
    /// None: the turn's end is the result, by the usual rules.
    Nothing,
    /// This many processes the turn started still run, `named` the first of
    /// them as the board says them (empty where the host listed none).
    Running { count: usize, named: String },
    /// They have ended, a moment ago: the agent may be about to be woken.
    Ended,
    /// The turn ended with background agents or workflows pending, which its
    /// harness says on the reading: the end of one wakes a turn of its own.
    Agents,
}

impl Left {
    /// What the board says the window is doing, where it is held for work.
    pub(crate) fn reason(&self) -> Option<String> {
        match self {
            Left::Nothing => None,
            Left::Running { count, named } => {
                let processes = match count {
                    1 => "1 process".to_owned(),
                    count => format!("{count} processes"),
                };
                let which = if named.is_empty() {
                    String::new()
                } else {
                    format!(": {named}")
                };
                Some(format!("background work running ({processes}){which}"))
            }
            Left::Ended => Some("background work ended; the agent may go on".to_owned()),
            Left::Agents => Some("background agents running".to_owned()),
        }
    }
}

/// What the window was last found to be held for, or its agents where it was
/// held for no work: what a look keeps while the window shows a dialog or a
/// message is going in.
fn remembered(held: Option<&Held>, agents: bool) -> Left {
    match (held, agents) {
        (Some(held), _) => held.left(),
        (None, true) => Left::Agents,
        (None, false) => Left::Nothing,
    }
}

impl Dispatcher {
    /// A turn is about to begin in the window: what its tree runs now came
    /// before it, and, where its harness is known to keep its own processes (its
    /// servers), so do they and what they start later. Not while the window holds
    /// its task's result back for the work an earlier turn started, which this
    /// message does not begin again.
    pub(crate) async fn mark_turn(&self, record: &Record, pane: &Pane) {
        if record.background.borrow().held.is_some() {
            return;
        }
        let servers = record.window.borrow().servers;
        self.seams.background.mark(pane, servers).await;
    }

    /// What the settled window of a worker's working task left running, asked
    /// just before the task's result would be taken. A hold keeps what it was
    /// last found to be while the window shows a dialog or a message is going in.
    pub(crate) async fn left_running(
        &self,
        participant: &ParticipantView,
        record: &Record,
        observed: &Observed,
        starting: bool,
    ) -> Result<Left, EngineError> {
        // A turn under way holds nothing back, and one that failed is the
        // task's failure, whatever its agent left running.
        if !observed.settled || observed.failed {
            record.background.borrow_mut().held = None;
            return Ok(Left::Nothing);
        }
        let held = record.background.borrow().held.clone();
        // Agents are the reading's word, which the host cannot give: a turn
        // that ended with some pending left them running.
        let agents = observed.agents_pending();
        if starting || observed.waiting.is_some() || record.delivery.borrow().delivering.is_some() {
            return Ok(remembered(held.as_ref(), agents));
        }
        let state = self
            .seams
            .ledger
            .borrow()
            .task_in_hand(participant.id)?
            .map(|thread| thread.task.state);
        match state.as_deref() {
            Some("working") => {}
            // A task paused, or waiting for an answer, goes on in this window:
            // the work its turn started is still its own.
            Some("paused" | "waiting") => return Ok(Left::Nothing),
            _ => {
                record.background.borrow_mut().held = None;
                return Ok(Left::Nothing);
            }
        }
        let pane = record.window.borrow().pane.clone();
        let Some(pane) = pane else {
            return Ok(Left::Nothing);
        };
        let work = self.seams.background.running(&pane).await;
        let listed = agent_processes(participant.harness.as_deref(), &work.processes);
        let running = agent_work(&work, &listed);
        let mut part = record.background.borrow_mut();
        if running > 0 {
            let named = first_named(&listed);
            part.held = Some(Held {
                running,
                named: named.clone(),
                ended_at: None,
            });
            return Ok(Left::Running {
                count: running,
                named,
            });
        }
        // No settle time ends a wait for agents: the turn an agent's end wakes
        // is the one whose end is the result, however long they run.
        if agents {
            return Ok(Left::Agents);
        }
        let Some(mut held) = part.held.clone() else {
            return Ok(Left::Nothing);
        };
        let now = self.now();
        let ended_at = *held.ended_at.get_or_insert(now);
        if now - ended_at < self.seams.limits.background_settle_ms {
            part.held = Some(held);
            return Ok(Left::Ended);
        }
        part.held = None;
        Ok(Left::Nothing)
    }

    /// The window is gone: it holds nothing back.
    pub(crate) fn release_background(&self, record: &Record) {
        record.background.borrow_mut().held = None;
    }

    /// What a window that closed with work its turn started still running says
    /// to whoever gave the task it last worked on, where no pause note says it
    /// (a stall's does): the task was cancelled, reassigned or failed and its
    /// window closed after, or it was done and the work began between the look
    /// that let its result be taken and the close. A window that closes with
    /// none says nothing.
    pub(crate) fn tell_left_running(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
        left: &[BackgroundProcess],
    ) -> Result<(), EngineError> {
        if left.is_empty() {
            return Ok(());
        }
        let Some(thread) = self.seams.ledger.borrow().last_task(participant.id)? else {
            return Ok(());
        };
        let number = thread.task.number;
        self.seams.ledger.borrow_mut().note(
            project.id,
            &NewNote {
                from: None,
                to: thread.task.requester,
                task: Some(number),
                body: format!(
                    "T-{number}'s window closed with background work still running: {}",
                    quote(left)
                ),
            },
        )?;
        Ok(())
    }
}

/// The processes of `listed`, which a window's tree started in the turn, that
/// are the agent's work: all but the harness's own (`kind` is the harness of
/// the window, as the roster says it).
pub(crate) fn agent_processes(
    kind: Option<&str>,
    listed: &[BackgroundProcess],
) -> Vec<BackgroundProcess> {
    let harness = kind.and_then(Harness::from_kind);
    listed
        .iter()
        .filter(|process| harness.is_none_or(|harness| !harness_own(harness, &process.command)))
        .cloned()
        .collect()
}

/// How many of the processes a window's tree started in the turn (`work`) are
/// the agent's work, `agents` being those of its list that are
/// ([`agent_processes`]). Of a list the host cut short, the ones it left out are
/// taken for the agent's.
fn agent_work(work: &Background, agents: &[BackgroundProcess]) -> usize {
    let own = work.processes.len() - agents.len();
    work.total.saturating_sub(own)
}

/// Whether the process a command line starts is a program of the harness's
/// own: the program is named for the harness, then a dash (`codex-code-mode-host`
/// for Codex), on Windows with its extension. The program is the line's first
/// word.
fn harness_own(harness: Harness, command: &str) -> bool {
    let program = command.split_whitespace().next().unwrap_or_default();
    let name = program.rsplit(['/', '\\']).next().unwrap_or_default();
    name.strip_prefix(harness.as_str())
        .is_some_and(|rest| rest.starts_with('-'))
}

/// The command of `process` as a note or the board says it: held to one line and
/// to `most` characters, what looks like a key or a token in it hidden (and
/// counted in `masked`), a process whose command could not be read named by its
/// number.
fn line(process: &BackgroundProcess, most: usize, masked: &mut usize) -> String {
    let hidden = mask(&process.command);
    *masked += hidden.count;
    let words = hidden.text.split_whitespace().collect::<Vec<_>>().join(" ");
    if words.chars().count() > most {
        let kept: String = words.chars().take(most - 1).collect();
        format!("{kept}…")
    } else if words.is_empty() {
        format!("process {}", process.pid)
    } else {
        words
    }
}

/// The commands of what a window left running, as a note quotes them: the first
/// few, each held to one line and a few characters, what looks like a key or a
/// token in them hidden, and how many were left out or hidden said after them.
pub(crate) fn quote(left: &[BackgroundProcess]) -> String {
    let mut masked = 0;
    let mut commands: Vec<String> = left
        .iter()
        .take(QUOTED)
        .map(|process| line(process, COMMAND_CHARS, &mut masked))
        .collect();
    if left.len() > QUOTED {
        commands.push(format!("and {} more", left.len() - QUOTED));
    }
    let mut quoted = commands.join("; ");
    if masked > 0 {
        let strings = if masked == 1 { "string" } else { "strings" };
        quoted.push_str(&format!(" ({masked} key- or token-like {strings} masked)"));
    }
    quoted
}

/// The first commands of what a window is held for, as the board names them: a
/// shorter line than a note's, the same hiding of a key or a token, and no count
/// after them, which the board says before.
fn first_named(left: &[BackgroundProcess]) -> String {
    let mut masked = 0;
    left.iter()
        .take(NAMED)
        .map(|process| line(process, NAMED_CHARS, &mut masked))
        .collect::<Vec<_>>()
        .join("; ")
}

#[cfg(test)]
mod tests;
