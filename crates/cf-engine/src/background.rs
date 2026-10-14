//! Background work: what a worker's window started that outlives its turn. An
//! agent can leave a job running and end its turn (its harness lets it, and wakes
//! it when the job ends). **A worker's turn that ends with its answer is its
//! task's result at once, whatever it left running, and what it left running is
//! ended as the result is taken.** The owner decided it on 2026-10-13, after two
//! field reports from a Windows machine (T-7 and T-15): a headless Edge check
//! (`msedge --headless --dump-dom …`), which the harness moved to the background
//! after the agent's wait of 22 s and which never exits, kept a finished worker's
//! task working for as long as its window lived, with no result for the chief, no
//! way to accept the task and only the worker's board line to say why. It reverses
//! two rules that held the result back: the owner's of 2026-10-09 (none while
//! processes the turn started still ran, with no limit, after a 30-minute
//! recording run died with the window its worker's turn had ended), and that of
//! 2026-10-11 (none while the turn's background agents were pending, after a
//! chief's result waited twelve minutes for its agents). A chief's window is not
//! held and is not touched: it is a worker's turn, and its task, that this is
//! about.
//!
//! - The pane host keeps the processes a window's tree had when a turn began
//!   ([`BackgroundWork::mark`]: just before a message goes into the window, and for
//!   a launch's first message once its header shows in the window's record, which
//!   is the harness's own start of the turn), and is told which harness the window
//!   is and how that harness keeps its servers (`Adapter::servers`). What the window
//!   had before the turn (the harness, its servers) is not the turn's.
//! - A worker's settled turn that did not fail, on a working task, is its result by
//!   the usual rules ([`Dispatcher::collect`]). Once it is recorded the engine asks
//!   the host to end what the turn left ([`BackgroundWork::end`]), the window staying
//!   open and its harness untouched, and tells whoever gave the task, and then closes
//!   the window as any finished worker's closes. All of that is work of the window's
//!   own, begun apart from the pass ([`Dispatcher::act`]) as a launch or a delivery
//!   is: it holds the window, so that nothing goes into it and no pass looks at it
//!   meanwhile, and what it waits for (on Unix the host asks the work to end first,
//!   TERM and then HUP, and gives it up to ten seconds to release what it holds, as
//!   a close does; on Windows each process is ended at once and waited for) keeps no
//!   other window's look waiting. The window is held until the host has said it is
//!   done: the daemon waits for its answer by the bound the host keeps to and a
//!   margin, and a host that gives none is asked again, which ends what is left of
//!   the turn's work and answers when that is done ([`Dispatcher::ending_of`]). A turn
//!   that failed is its task's failure whatever it left running, and a window with
//!   nothing left says nothing.
//! - What is the turn's is for the host to say, and the engine tells it no process:
//!   it takes its word, from all of what the host can prove (`cf-panes`' `background`
//!   says how). **When in doubt the host ends less.** Of what began after the mark
//!   the host calls the turn's only what a chain of processes that run leads to: a
//!   shell the harness started, and what that started. A job an agent detached with
//!   `nohup … &` from a shell that then ended has no chain, and is no work of the turn
//!   by having none; nor is anything the harness's own, by where its harness keeps its
//!   servers, by the programs it is named for, or by having been seen the harness's.
//!   What it can tell neither (a server a harness starts late: Claude Code's MCP
//!   servers begin as it takes its first message, Codex's when a tool first needs one)
//!   it leaves alone, and says nothing of. A short run of the harness's own that is in
//!   no group of the window's (Claude Code's status line: a `bash` that runs `git …
//!   branch --show-current`, in a group of its own as its agent's commands are, T-42 on
//!   3.0.3) is told from the agent's by its ending: the host looks again a moment after
//!   it first finds the turn's work, at a close, at a program's exit and at a turn's
//!   end, and names and ends only what both looks find (`cf-panes`' `background`). So
//!   the engine ends nothing the host did not choose, and a harness's own process is
//!   not at its mercy.
//! - Whoever gave the task is told in a note right after the result, one line in the
//!   voice of the notes beside it ([`left_note`]): what was ended (a command is
//!   quoted as a close's note quotes it, with its keys and tokens masked), what of
//!   the turn's work could not be ended and still runs (which is also what the host
//!   chose as the turn's and its bound would not let it signal, as nothing showed it
//!   was the window's: it may run on), and that the turn left background agents.
//!   Nothing is said of a turn that left nothing, nor of a process the host left
//!   alone.
//! - The agents and workflows a turn started in its harness are work the turn left
//!   running too, which the pane host cannot see (they are no process of the
//!   window's tree): the harness says so on its record, on the reading of a turn
//!   that ended with some pending ([`Observed::agents_pending`], Claude Code's own
//!   counts). The result is taken all the same, and the note says the turn left
//!   them. The host has no way to end them that touches nothing else: Claude Code
//!   stops them with `Ctrl+X Ctrl+K` pressed twice, or from its `/tasks` view, which
//!   are keys typed into a window that may be the human's, and the window that
//!   closes after the result ends them with it. One a human keeps open goes on
//!   with them; an agent's end then wakes a turn in a window whose task is done,
//!   and that turn is no result (the task is no longer working) and fails nothing:
//!   it is what the window shows, as any turn of a worker whose task is done.
//! - A window that closes with work still running says so to whoever gave the task:
//!   the host lists what the close ended in `pane.exit`, the turn's work as above. On
//!   Unix the host asks the work to end first and gives it a grace, as above. What an
//!   ending ended is not on that list, and what it could not end, and a close then
//!   ends, is.
//!
//! Limits.
//!
//! **What escapes.** A command whose shell has ended is left running, not ended and not
//! named, whichever way the agent made it outlive the shell (`nohup … &`, `setsid`,
//! `disown`, a double fork, `Start-Process`): no chain of processes that run leads
//! from it to a shell the harness started, and it being a job without a parent proves
//! nothing of whose it is. The window's close ends it where it can be told to be the
//! window's, and not otherwise: the pane host signals only the pane's program, what runs
//! under it by the process that started each, what is in the session the program leads
//! while it runs, and what a look saw so, however the tree is selected. A job in the
//! window's session is ended with the window; one that left it (`setsid`, and the shells
//! of Claude Code and Pi, which lead sessions of their own) with its shell gone, which no
//! look saw under the shell, is left running, and so is one that only the marker in its
//! environment says is the window's. Which of a harness's shells are in the window's
//! session is its own, and the live step shows it. On Windows the pane's job object holds
//! what a process starts wherever it goes, and the close ends it, unless a service made
//! it for it: `schtasks`, a WMI call. What the host ends
//! is what a shell that still runs started: a command its shell tool is still waiting
//! for when the turn ends, and one the harness runs in the background for its agent
//! and keeps the shell of. Whether a harness's own tool call ends what its tool
//! started when the call ends (Codex and Devin, seen live on 2026-10-14) is its own.
//! A process of the harness's own that is in no group of the window's and lasts longer
//! than the moment the host waits before it looks again (a status line that takes
//! seconds) is named, and ended at a turn's end, as the agent's: the host tells it from
//! a command by nothing but its ending.
//!
//! **What keeps a window working.** A worker's answer is its result when its window's
//! turn has settled, which each harness's reader says of its own record. A Devin
//! window is working while its wire log says its turn is on (the thoughts, the words
//! and the calls it begins since the last end of a turn), and what a command the turn
//! left running goes on saying (a ping says a line a second, a server its log) is no
//! part of that (`cf-harness`'s `devin::record`): else a worker that left a command
//! that reports would never have its task done, whatever the host ended of what it
//! left.
//!
//! What the host takes for a job it cannot tell from a server is ended: a server the
//! user wraps in `bash` or `powershell` and the harness starts after the mark, where
//! the harness is not known to keep its servers in a way (OpenCode, Devin, and
//! every harness on Windows), and has not been seen the harness's while it ran. The
//! servers the harnesses document are started as programs, and none of them was seen
//! started by a shell.
//!
//! A window whose program ends by itself (it went, or crashed) leaves its commands
//! without the process that started their shells: none is the turn's by a chain that
//! runs, so its exit names none, and its close ends what is the window's, as above,
//! unsaid.
//!
//! A launch's first message is marked when it shows in the window's record, which
//! the engine sees on a pass (a second apart): work an agent starts before that is
//! in the window's mark, and is not the turn's. A turn that began and ended between
//! two passes is marked after what it left began: the host lists none of it, so
//! none is ended and nothing is said, and the close that follows ends what is the
//! window's, as above, unsaid.

use std::rc::Rc;
use std::time::Duration;

use cf_harness::contract::Pane;
use cf_ledger::{NewNote, ParticipantView, ProjectView, TaskView};
use cf_proto::panes::{BackgroundProcess, Ending};

use crate::dispatcher::Dispatcher;
use crate::record::Record;
use crate::seams::EngineError;
use crate::shown::mask::mask;

/// How many times the host is asked to end what a turn left while it gives no
/// answer, and how long the engine waits before it asks again.
const ENDING_ASKS: usize = 3;
const ENDING_ASKED_AGAIN: Duration = Duration::from_secs(1);

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

    /// The result of a worker's turn was just recorded: what follows it is the
    /// window's own work, begun apart from the pass and holding the window
    /// ([`Dispatcher::end_turn_work`], then the close of a window that is free).
    /// `agents` is the reading's word that the turn left background agents.
    pub(crate) async fn end_work_apart(
        self: &Rc<Self>,
        project: &ProjectView,
        record: &Rc<Record>,
        task: TaskView,
        agents: bool,
    ) {
        let (this, held, project) = (Rc::clone(self), Rc::clone(record), project.clone());
        self.act(record, async move {
            this.end_turn_work(&project, &held, &task, agents).await?;
            // The participant may have been forgotten, or the window closed, while the
            // host answered.
            if this.forgotten(&held) || held.window.borrow().pane.is_none() {
                return Ok(());
            }
            this.close_if_free(&held).await?;
            Ok(())
        })
        .await;
    }

    /// What the turn left is ended, the window staying open, and whoever gave the task
    /// is told what was ended, what of the turn's work could not be and still runs,
    /// and that its agents were left. A window that left nothing says nothing, and one
    /// the host cannot be asked of has left nothing to say.
    async fn end_turn_work(
        &self,
        project: &ProjectView,
        record: &Rc<Record>,
        task: &TaskView,
        agents: bool,
    ) -> Result<(), EngineError> {
        let ending = self.ending_of(record).await;
        // The participant may have been forgotten while the host answered.
        if self.forgotten(record) {
            return Ok(());
        }
        let Some(body) = left_note(task.number, &ending, agents) else {
            return Ok(());
        };
        self.seams.ledger.borrow_mut().note(
            project.id,
            &NewNote {
                from: None,
                to: task.requester.clone(),
                task: Some(task.number),
                body,
            },
        )?;
        self.changed();
        Ok(())
    }

    /// What the host says it ended of what the window left, waited for to the end: its
    /// answer comes when it is done, which is up to its grace and what it waits after,
    /// and the window stays held until it does. A host that gives none (it went, or it
    /// is ending still) is asked again, a moment later, which ends what is left of the
    /// turn's work as it did, and answers when that is done; after [`ENDING_ASKS`] asks
    /// with no answer it is taken to have gone, and nothing is known of what it did:
    /// the window closes as a free worker's does, which ends what its tree runs.
    async fn ending_of(&self, record: &Rc<Record>) -> Ending {
        for asked in 1..=ENDING_ASKS {
            let Some(pane) = record.window.borrow().pane.clone() else {
                break;
            };
            if let Some(ending) = self.seams.background.end(&pane).await {
                return ending;
            }
            if asked < ENDING_ASKS {
                self.seams.time.sleep(ENDING_ASKED_AGAIN).await;
            }
        }
        Ending::default()
    }

    /// What a window that closed with work its turn started still running says
    /// to whoever gave the task it last worked on, where no pause note says it
    /// (a stall's does): the task was cancelled, reassigned or failed and its
    /// window closed after, or it was done and the work began between the end of
    /// its turn and the close, or was one the host would not end. A window that
    /// closes with none says nothing.
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

/// What whoever gave T-`number` is told of what its worker's turn left, as the
/// result is taken: what was ended, what of the turn's work could not be and still
/// runs, and that agents were left. None for a turn that left nothing.
pub(crate) fn left_note(number: i64, ending: &Ending, agents: bool) -> Option<String> {
    let (ended, left) = (&ending.ended, &ending.left);
    let work = ended.total > 0 || left.total > 0;
    let mut said = match (work, agents) {
        (true, _) => format!("T-{number}'s turn left background work running."),
        (false, true) => return Some(format!("T-{number}'s turn left background agents running.")),
        (false, false) => return None,
    };
    if ended.total > 0 {
        said.push_str(&format!(
            " Ended: {}.",
            quote_of(&ended.processes, ended.total)
        ));
    }
    if left.total > 0 {
        said.push_str(&format!(
            " Still running, not ended: {}.",
            quote_of(&left.processes, left.total)
        ));
    }
    if agents {
        said.push_str(" Its background agents were left running too.");
    }
    Some(said)
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
pub(crate) fn quote(left: &[BackgroundProcess]) -> String {
    quote_of(left, left.len())
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
