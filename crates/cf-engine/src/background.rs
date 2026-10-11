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
//! Limits. **On Windows a job the agent left running is ended with its window at
//! once, with no chance to release what it holds**: the host asks nothing there
//! and its job object ends the tree. The Ctrl+Break that would ask it escapes the
//! pane's pseudoconsole when the window's program is behind a `.cmd` shim (as npm
//! installs every harness), reaches the host's own console and ends the host. The
//! hold, the board's line and the note to whoever gave the task are the same on
//! every platform: they do not depend on how the window closes.
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
use cf_proto::panes::BackgroundProcess;

use crate::dispatcher::Dispatcher;
use crate::record::Record;
use crate::seams::EngineError;
use crate::shown::mask::mask;

/// The most commands a note quotes of what a window left running.
const QUOTED: usize = 5;
/// The most characters of one command a note quotes.
const COMMAND_CHARS: usize = 160;

/// What a window holds its task's result back for, while it does.
#[derive(Debug, Default)]
pub(crate) struct BackgroundPart {
    held: Option<Held>,
}

/// The work a window's task is held for: how much of it ran when it was last
/// looked at, and since when it has ended, if it has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Held {
    running: usize,
    ended_at: Option<i64>,
}

impl Held {
    fn left(self) -> Left {
        match self.ended_at {
            None => Left::Running(self.running),
            Some(_) => Left::Ended,
        }
    }
}

/// What a settled worker window's turn left, as far as its task's result goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Left {
    /// None: the turn's end is the result, by the usual rules.
    Nothing,
    /// This many processes the turn started still run.
    Running(usize),
    /// They have ended, a moment ago: the agent may be about to be woken.
    Ended,
    /// The turn ended with background agents or workflows pending, which its
    /// harness says on the reading: the end of one wakes a turn of its own.
    Agents,
}

impl Left {
    /// What the board says the window is doing, where it is held for work.
    pub(crate) fn reason(self) -> Option<String> {
        match self {
            Left::Nothing => None,
            Left::Running(1) => Some("background work running (1 process)".to_owned()),
            Left::Running(count) => Some(format!("background work running ({count} processes)")),
            Left::Ended => Some("background work ended; the agent may go on".to_owned()),
            Left::Agents => Some("background agents running".to_owned()),
        }
    }
}

/// What the window was last found to be held for, or its agents where it was
/// held for no work: what a look keeps while the window shows a dialog or a
/// message is going in.
fn remembered(held: Option<Held>, agents: bool) -> Left {
    match (held, agents) {
        (Some(held), _) => held.left(),
        (None, true) => Left::Agents,
        (None, false) => Left::Nothing,
    }
}

impl Dispatcher {
    /// A turn is about to begin in the window: what its tree runs now came
    /// before it. Not while the window holds its task's result back for the
    /// work an earlier turn started, which this message does not begin again.
    pub(crate) async fn mark_turn(&self, record: &Record, pane: &Pane) {
        if record.background.borrow().held.is_some() {
            return;
        }
        self.seams.background.mark(pane).await;
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
        let held = record.background.borrow().held;
        // Agents are the reading's word, which the host cannot give: a turn
        // that ended with some pending left them running.
        let agents = observed.agents_pending();
        if starting || observed.waiting.is_some() || record.delivery.borrow().delivering.is_some() {
            return Ok(remembered(held, agents));
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
        let mut part = record.background.borrow_mut();
        if work.total > 0 {
            part.held = Some(Held {
                running: work.total,
                ended_at: None,
            });
            return Ok(Left::Running(work.total));
        }
        // No settle time ends a wait for agents: the turn an agent's end wakes
        // is the one whose end is the result, however long they run.
        if agents {
            return Ok(Left::Agents);
        }
        let Some(mut held) = part.held else {
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

/// The commands of what a window left running, as a note quotes them: the first
/// few, each held to one line and a few characters, what looks like a key or a
/// token in them hidden, and how many were left out or hidden said after them.
pub(crate) fn quote(left: &[BackgroundProcess]) -> String {
    let mut masked = 0;
    let mut commands: Vec<String> = left
        .iter()
        .take(QUOTED)
        .map(|process| {
            let hidden = mask(&process.command);
            masked += hidden.count;
            let words = hidden.text.split_whitespace().collect::<Vec<_>>().join(" ");
            if words.chars().count() > COMMAND_CHARS {
                let kept: String = words.chars().take(COMMAND_CHARS - 1).collect();
                format!("{kept}…")
            } else if words.is_empty() {
                format!("process {}", process.pid)
            } else {
                words
            }
        })
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

#[cfg(test)]
mod tests {
    use super::*;

    fn process(pid: u32, command: &str) -> BackgroundProcess {
        BackgroundProcess {
            pid,
            command: command.to_owned(),
        }
    }

    #[test]
    fn the_board_says_how_many_processes_the_window_waits_for() {
        assert_eq!(Left::Nothing.reason(), None);
        assert_eq!(
            Left::Running(1).reason().as_deref(),
            Some("background work running (1 process)")
        );
        assert_eq!(
            Left::Running(2).reason().as_deref(),
            Some("background work running (2 processes)")
        );
        assert!(Left::Ended.reason().is_some());
        assert_eq!(
            Left::Agents.reason().as_deref(),
            Some("background agents running")
        );
    }

    #[test]
    fn a_look_that_keeps_what_it_found_keeps_the_work_it_was_held_for_before_the_agents() {
        let running = Held {
            running: 2,
            ended_at: None,
        };
        let ended = Held {
            running: 2,
            ended_at: Some(5),
        };
        assert_eq!(remembered(Some(running), false), Left::Running(2));
        assert_eq!(remembered(Some(running), true), Left::Running(2));
        assert_eq!(remembered(Some(ended), true), Left::Ended);
        // Held for no work, a window whose turn left agents is held for them.
        assert_eq!(remembered(None, true), Left::Agents);
        assert_eq!(remembered(None, false), Left::Nothing);
    }

    #[test]
    fn a_note_quotes_the_first_commands_and_says_how_many_it_leaves_out() {
        let left: Vec<_> = (1..=7)
            .map(|n| process(n, &format!("python3 job.py {n}")))
            .collect();
        assert_eq!(
            quote(&left),
            "python3 job.py 1; python3 job.py 2; python3 job.py 3; python3 job.py 4; python3 job.py 5; and 2 more"
        );
    }

    #[test]
    fn a_command_is_one_line_and_bounded_and_a_secret_in_it_is_hidden() {
        let long = format!("tool {}", "x".repeat(400));
        let quoted = quote(&[process(1, &long)]);
        assert_eq!(quoted.chars().count(), COMMAND_CHARS);
        assert!(quoted.ends_with('…'));

        let quoted = quote(&[process(
            2,
            "curl -H 'Authorization: Bearer abcdef0123456789abcdef' --data  a\nb",
        )]);
        assert!(!quoted.contains("abcdef0123456789abcdef"), "{quoted}");
        assert!(quoted.contains("[masked]"), "{quoted}");
        assert!(
            quoted.ends_with("(1 key- or token-like string masked)"),
            "{quoted}"
        );
        assert!(!quoted.contains('\n'));
    }

    #[test]
    fn a_process_whose_command_could_not_be_read_is_named_by_its_number() {
        assert_eq!(quote(&[process(42, "")]), "process 42");
    }
}
