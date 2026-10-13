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
//!   is the harness's own start of the turn) and says which others it has now
//!   ([`BackgroundWork::running`]). What the window had before the turn (the
//!   harness, its servers) is not the turn's.
//! - A worker's settled turn that did not fail, on a working task, is its result by
//!   the usual rules ([`Dispatcher::collect`]). Once it is recorded the engine asks
//!   the host what the turn left, leaves out the harness's own programs by their
//!   names (below), and asks the host to end the rest ([`BackgroundWork::end`]):
//!   the work, and what descends from it. The window stays open and its harness is
//!   untouched. On Unix the host asks the work to end first (TERM, then HUP) and
//!   gives it a grace to release what it holds, as a close does, so a job that
//!   handles the request ends cleanly; on Windows each process is ended at once.
//!   A turn that failed is its task's failure whatever it left running, and a
//!   window with no work left says nothing.
//! - What the host cannot tell for the turn's with no doubt it does not end, and
//!   says so. A process taken for the turn's work used to only delay a result; now
//!   it would be killed, and one that is the harness's own would break the worker's
//!   servers. Where the daemon said how the harness keeps its servers (below) the
//!   host ends all it lists; where it did not (OpenCode, Devin, and every harness
//!   on Windows) it ends only what a shell the harness started after the mark
//!   started (`cf-panes`' `background::shells` says why, and what that leaves).
//! - Whoever gave the task is told in a note right after the result, one line in the
//!   voice of the notes beside it ([`left_note`]): what was ended (a command is
//!   quoted as a close's note quotes it, with its keys and tokens masked), what
//!   could not be ended and still runs, and that the turn left background agents.
//!   Nothing is said of a turn that left nothing.
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
//!   the host lists what it ended in `pane.exit`. On Unix the host asks the work to
//!   end first and gives it a grace, as above. What an ending ended is not on that
//!   list, and what it could not end, and a close then ends, is.
//!
//! What the harness itself starts in the turn is no work the agent left. Codex
//! starts `codex-code-mode-host` the first time its agent runs a command, and
//! keeps it for the session: a program of Codex's own, beside its binary, which
//! the host sees begin after the mark like any job. A process named for the
//! window's harness (`<harness>-…`) is the harness's, and is not ended.
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
//! alone, as far as what its turn left is listed, and the host ends none of that
//! but what a shell the harness started after the mark started.
//!
//! Limits.
//!
//! **On Windows a job object has no process groups**, so no harness says how it
//! keeps its servers there, and the first mark of a window cannot tell a server
//! that begins after it from a job (Claude Code starts its MCP servers as it takes
//! its first message). The host ends there only what descends from a shell: a
//! `bash` or a `powershell` that the harness started, as it starts each command its
//! agent runs, after the mark, with the parent each process of the job names
//! (Toolhelp32, held for the process that started it only if that one began first).
//! What does not (a job whose shell has gone, a process the harness started itself,
//! whatever a `cmd.exe` runs: npm installs every server behind a `cmd /c npx`) is left
//! running, and the note says so; the close that follows a done task ends it, as a
//! job object ends its whole tree, and says so in the note of the close.
//!
//! On Unix the host looks at who started each process, and a process whose parent
//! has gone has no one to look at: it is told by its own place alone, so a browser
//! that outlives the server that launched it, or a daemon a server's child
//! double-forks, is the turn's work unless it stayed in the window's group, and is
//! ended. A Codex server that begins after the window's first turn began (a plugin
//! it starts only when a tool is called) is no server of its launch, and what it
//! starts is the turn's, and is ended. Nor can a program of Codex's own that
//! begins with the window, outside its group, be told from a server
//! (`codex-code-mode-host` begins with the first command): what it started would
//! be ended, though it is the harness's.
//!
//! A launch's first message is marked when it shows in the window's record, which
//! the engine sees on a pass (a second apart): work an agent starts before that is
//! in the window's mark, and is not the turn's. A turn that began and ended between
//! two passes is marked after what it left began: the host lists none of it, so
//! none is ended and nothing is said, and the close that follows ends it unsaid.
//! And a long-lived helper the harness starts in the turn itself (a language
//! server, a dev server) is work the turn started, and is ended with it.

use std::rc::Rc;

use cf_harness::contract::Pane;
use cf_ledger::{NewNote, ParticipantView, ProjectView, TaskView};
use cf_proto::agents::Harness;
use cf_proto::panes::{BackgroundProcess, Ending};

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
    /// servers), so do they and what they start later.
    pub(crate) async fn mark_turn(&self, record: &Record, pane: &Pane) {
        let servers = record.window.borrow().servers;
        self.seams.background.mark(pane, servers).await;
    }

    /// The result of a worker's turn was just recorded: what the turn left
    /// running is ended, the window staying open, and whoever gave the task is
    /// told what was ended, what could not be and still runs, and that its agents
    /// were left (`agents`, the reading's word). A window that left nothing says
    /// nothing, and one the host cannot be asked of has left nothing to say.
    pub(crate) async fn end_turn_work(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
        record: &Rc<Record>,
        task: &TaskView,
        agents: bool,
    ) -> Result<(), EngineError> {
        let pane = record.window.borrow().pane.clone();
        let ending = match pane {
            Some(pane) => self.end_left_running(participant, &pane).await,
            None => Ending::default(),
        };
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

    /// What the host did about what `pane`'s turn left, less the harness's own
    /// programs, which are never asked for.
    async fn end_left_running(&self, participant: &ParticipantView, pane: &Pane) -> Ending {
        let work = self.seams.background.running(pane).await;
        let left = agent_processes(participant.harness.as_deref(), &work.processes);
        if left.is_empty() {
            return Ending::default();
        }
        self.seams.background.end(pane, &left).await
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
/// result is taken: what was ended, what could not be and still runs, and that
/// agents were left. None for a turn that left nothing.
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
