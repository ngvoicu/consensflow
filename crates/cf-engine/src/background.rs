//! Background work: what a worker's window started that outlives its turn. An
//! agent can leave a job running and end its turn (its harness lets it, and wakes
//! it when the job ends). **A worker's turn ends nothing it left running** (turn-end's
//! smallest rule, the chief's decision of 2026-10-21; it reverses the owner's decision of
//! 2026-10-13, under which what a turn left was ended as its result was taken, which
//! ended poker-lab's T-442 test run while its worker waited on it with Monitor): a task
//! whose worker's harness says it waits on work it wakes the agent for stays working
//! (`turn_end`), and what else the turn left goes on until the window closes. A chief's
//! window is not held and is not touched.
//!
//! - The pane host keeps the processes a window's tree had when a turn began
//!   ([`BackgroundWork::mark`]: just before a message goes into the window, and for
//!   a launch's first message once its header shows in the window's record, which
//!   is the harness's own start of the turn), and is told which harness the window
//!   is and how that harness keeps its servers (`Adapter::servers`). What the window
//!   had before the turn (the harness, its servers) is not the turn's: the host lists
//!   what a close ends of the rest.
//! - A worker's window that is free closes after its result as any does, and the close
//!   ends what the host can tell is the window's; one the human keeps open keeps it, and
//!   no other task's message goes into it (`ownership`). Whoever gave the task the window
//!   was given is told what the close ended ([`Dispatcher::tell_left_running`]): the
//!   window's own task, never a task queued for the session since (Astra's review of
//!   round 7). What is the turn's is for the host to say, and the engine tells it no
//!   process (`cf-panes`' `background` says how).
//! - The agents and workflows a turn started in its harness are work it left running too,
//!   which the pane host cannot see (they are no process of the window's tree): the
//!   harness says so on its record (Claude Code's own counts), and they keep the task
//!   working as the jobs do (`turn_end`).
//!
//! Limits.
//!
//! **What escapes.** A command whose shell has ended is not named by the host,
//! whichever way the agent made it outlive the shell (`nohup … &`, `setsid`,
//! `disown`, a double fork, `Start-Process`): no chain of processes that run leads
//! from it to a shell the harness started, and it being a job without a parent proves
//! nothing of whose it is. The window's close ends it where it can be told to be the
//! window's, and not otherwise: the pane host signals only the pane's program, what runs
//! under it by the process that started each, what is in the session the program leads
//! while it runs, and what a look saw so, however the tree is selected. A job in the
//! window's session is ended with the window; one that left it (`setsid`, and the shells
//! of Claude Code and Pi, which lead sessions of their own) with its shell gone, which no
//! look saw under the shell, is left running, and so is one that only the marker in its
//! environment says is the window's. On Windows the pane's job object holds what a process
//! starts wherever it goes, and the close ends it, unless a service made it for it:
//! `schtasks`, a WMI call. Closing a window does not prove every process it started died.
//!
//! **What keeps a window working.** A worker's answer is its result when its window's
//! turn has settled, which each harness's reader says of its own record. A Devin
//! window is working while its wire log says its turn is on (the thoughts, the words
//! and the calls it begins since the last end of a turn), and what a command the turn
//! left running goes on saying (a ping says a line a second, a server its log) is no
//! part of that (`cf-harness`'s `devin::record`).
//!
//! A launch's first message is marked when it shows in the window's record, which
//! the engine sees on a pass (a second apart): work an agent starts before that is
//! in the window's mark, and is not the turn's, and the host does not name it at a close.

use cf_harness::contract::Pane;
use cf_ledger::{NewNote, ParticipantView, ProjectView, TaskThread};
use cf_proto::panes::{Background, BackgroundProcess, PossiblyLeft};

use crate::dispatcher::Dispatcher;
use crate::record::Record;
use crate::seams::EngineError;
use crate::shown::mask::mask;

/// The most commands a note quotes of what a window left running.
const QUOTED: usize = 5;
/// The most characters of one command a note quotes.
const COMMAND_CHARS: usize = 160;

impl Dispatcher {
    /// A turn is about to begin in the window: what its tree runs now came
    /// before it, and, where its harness is known to keep its own processes (its
    /// servers), so do they and what they start later, and the programs the harness is
    /// named for.
    pub(crate) async fn mark_turn(&self, record: &Record, pane: &Pane) {
        let (servers, harness) = {
            let window = record.window.borrow();
            (window.servers, window.harness.clone())
        };
        self.seams.background.mark(pane, servers, &harness).await;
    }

    /// What a window that closed with work its turn started still running says to
    /// whoever gave the task the window was given (`given`: the newest task message that
    /// began to reach it, as it was when the window closed), where no pause note says it
    /// (a stall's does): the task was done, cancelled, reassigned, finished or failed and
    /// its window closed after. Never a task queued for the session since, which the
    /// window never had (Astra's review of round 7). A window that closes with none says
    /// nothing, and one that was given no task has no one to tell.
    pub(crate) fn tell_left_running(
        &self,
        project: &ProjectView,
        given: Option<&TaskThread>,
        left: &[BackgroundProcess],
        possibly: &[PossiblyLeft],
    ) -> Result<(), EngineError> {
        if left.is_empty() && possibly.is_empty() {
            return Ok(());
        }
        let Some(thread) = given else {
            return Ok(());
        };
        let number = thread.task.number;
        self.seams.ledger.borrow_mut().note(
            project.id,
            &NewNote {
                from: None,
                to: thread.task.requester.clone(),
                task: Some(number),
                body: closed_leaving(&format!("T-{number}'s window closed"), left, possibly),
            },
        )?;
        Ok(())
    }

    /// A pause of task T-`number` paid in its window at rest, which stays open: what its turn
    /// left running runs on, and so does what may be the window's outside its tree (a loop a
    /// tool shell detached), which no pause, turn's end or Finish ends (the chief's decision of
    /// 2026-10-24, a detached job is reported, never ended; T-160's retry loop, paused for disk
    /// on 2026-10-22, ran on 37 minutes unnamed). Whoever gave the task is told, once, in the
    /// words of a close's note, where the host names any and the task is still paused. Nothing
    /// is signalled.
    pub(crate) async fn tell_paused_left_running(
        &self,
        project: &ProjectView,
        record: &Record,
        number: i64,
    ) -> Result<(), EngineError> {
        let Some(pane) = record.window.borrow().pane.clone() else {
            return Ok(());
        };
        let Some(left) = self.seams.background.left_running(&pane).await else {
            return Ok(());
        };
        if left.total == 0 && left.possibly_left.is_empty() {
            return Ok(());
        }
        let task = self.seams.ledger.borrow().task(project.id, number)?;
        let Some(thread) = task.filter(|thread| thread.task.state == "paused") else {
            return Ok(());
        };
        self.seams.ledger.borrow_mut().note(
            project.id,
            &NewNote {
                from: None,
                to: thread.task.requester.clone(),
                task: Some(number),
                body: paused_leaving(number, &left),
            },
        )?;
        self.changed();
        Ok(())
    }

    /// What a chief's window that closed with work still running, or with processes the host
    /// names as possibly its own, says to the human: no task's giver is there to tell, and the
    /// words are a worker's close's. A window that closes with none says nothing.
    pub(crate) fn tell_human_left_running(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
        left: &[BackgroundProcess],
        possibly: &[PossiblyLeft],
    ) -> Result<(), EngineError> {
        if left.is_empty() && possibly.is_empty() {
            return Ok(());
        }
        let closed = format!("@{}'s window closed", participant.handle);
        self.seams.ledger.borrow_mut().note(
            project.id,
            &NewNote {
                from: None,
                to: "human".to_owned(),
                task: None,
                body: closed_leaving(&closed, left, possibly),
            },
        )?;
        Ok(())
    }
}

/// The command of `process` as a note says it: held to one line and to
/// [`COMMAND_CHARS`] characters, what looks like a key or a token in it hidden
/// (and counted in `masked`), a process whose command could not be read named by
/// its number.
fn line(process: &BackgroundProcess, masked: &mut usize) -> String {
    let hidden = mask(&process.command);
    *masked += hidden.count;
    let words = hidden.text.split_whitespace().collect::<Vec<_>>().join(" ");
    if words.chars().count() > COMMAND_CHARS {
        let kept: String = words.chars().take(COMMAND_CHARS - 1).collect();
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
fn quote(left: &[BackgroundProcess]) -> String {
    quote_of(left, left.len())
}

/// `closed` (a window closed), and what it left running and what may be its own, as a close
/// note says them.
fn closed_leaving(closed: &str, left: &[BackgroundProcess], possibly: &[PossiblyLeft]) -> String {
    format!("{closed}{}", leaving(left, possibly))
}

/// What every note of a window's close says after the window closed (the note to whoever gave
/// its task, the chief's to the human, the pause note): the work its close ended, which `pane.exit`
/// lists (`background`), and what may be its own and was not ended (`possiblyLeft`). Nothing for
/// none.
pub(crate) fn leaving(left: &[BackgroundProcess], possibly: &[PossiblyLeft]) -> String {
    let mut said = String::new();
    if !left.is_empty() {
        said.push_str(&format!(
            " with background work running, which its close ended: {}",
            quote(left)
        ));
    }
    if let Some(words) = possibly_left_words(possibly) {
        said.push_str(&format!("; {words}"));
    }
    said
}

/// What the note of a pause whose window stays says of what it left running (`left`, which
/// `pane.background` answered), in the words of a close's note but for the pause, which
/// ends none of it.
fn paused_leaving(number: i64, left: &Background) -> String {
    let mut said = format!("T-{number} is paused");
    if left.total > 0 {
        said.push_str(&format!(
            " with background work its window's turn left running, which the pause does not end: {}",
            quote_of(&left.processes, left.total)
        ));
    }
    if let Some(words) = possibly_left_words(&left.possibly_left) {
        said.push_str(&format!("; {words}"));
    }
    said
}

/// What a note says of the processes the host named as possibly a window's and did not end
/// (`PaneExit::possibly_left`): `pid 4242 (node) may be this window's and was not ended`,
/// [`QUOTED`] of them at most and the rest counted. None for none.
fn possibly_left_words(possibly: &[PossiblyLeft]) -> Option<String> {
    if possibly.is_empty() {
        return None;
    }
    let mut named: Vec<String> = possibly
        .iter()
        .take(QUOTED)
        .map(|process| format!("pid {} ({})", process.pid, program_of(process)))
        .collect();
    if possibly.len() > QUOTED {
        named.push(format!("{} more", possibly.len() - QUOTED));
    }
    let was = if possibly.len() == 1 { "was" } else { "were" };
    Some(format!(
        "{} may be this window's and {was} not ended",
        named.join(", ")
    ))
}

/// The name of the program of `process` as a note says it: on one line, held to
/// [`COMMAND_CHARS`] characters, what looks like a key or a token in it hidden.
fn program_of(process: &PossiblyLeft) -> String {
    let words = mask(&process.program)
        .text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if words.is_empty() {
        "unnamed".to_owned()
    } else {
        words.chars().take(COMMAND_CHARS).collect()
    }
}

/// [`quote`] of the first of `total` processes, which a host that lists no more than
/// a few says: the ones it left out are counted with the ones a note does.
fn quote_of(listed: &[BackgroundProcess], total: usize) -> String {
    if listed.is_empty() {
        return format!("{total} processes");
    }
    let mut masked = 0;
    let mut commands: Vec<String> = listed
        .iter()
        .take(QUOTED)
        .map(|process| line(process, &mut masked))
        .collect();
    if total > QUOTED {
        commands.push(format!("and {} more", total - QUOTED));
    }
    let mut quoted = commands.join("; ");
    if masked > 0 {
        let strings = if masked == 1 { "string" } else { "strings" };
        quoted.push_str(&format!(" ({masked} key- or token-like {strings} masked)"));
    }
    quoted
}

#[cfg(test)]
mod tests;
