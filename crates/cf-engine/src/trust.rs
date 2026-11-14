//! A window whose harness asks whether to trust its folder before it starts.
//! Claude Code asks it in a git repository it was never told to trust, and waits on
//! it with nothing else on the screen; a launch that nobody answered failed after
//! its time, its first message never shown, and a chief's waited for good
//! (dip-agentos's T-11, T-13, T-14 and T-15 each stopped there, 2026-10-19).
//!
//! The human chose the project's folder, and choosing it counts as trusting it
//! (the owner's decision, 2026-10-20): where what a yes trusts
//! ([`cf_harness::contract::Window::trusts`]) is the project's folder or a folder
//! below it, ConsensFlow answers yes on the window's screen as a person does, a
//! key at a time ([`cf_harness::contract::Window::toward_trust`]), and the notes
//! say so once. A yes that would trust a folder outside it (the root of a
//! repository the project's folder is below) is the human's: the notes say so
//! once the question has stayed [`TELL_LOOKS`], and the window waits for their answer
//! in it; once they said yes in one window, the project's other windows that ask of
//! that folder are answered as they answered, or, where no key of ConsensFlow's can
//! answer them (a cursor the reader cannot place), told of again; a window that ended
//! before the screen was looked at again, its task done and its window closed at once,
//! is read off the screen the host kept of it as the watch reads a prompt
//! ([`yes_at_exit`]). Either way nothing goes into the window, and nothing gives its
//! launch up, while the question is on its screen ([`Trusting`]); once it has gone
//! the launch goes on as any other, its first message given its whole time from
//! then. ConsensFlow writes none of the harness's own files: Claude Code rewrites
//! `~/.claude.json` from every running window, and a second writer could lose what
//! one of them wrote.
//!
//! The screen is watched apart from the launch, from the moment the window opens
//! (Claude Code shows the question after its launch has come up), a look every
//! half second. A key is pressed only into a screen that moved since the last key,
//! so that each press meets the question as the last look read it and a slow redraw
//! is never given a second; a Down that did not move it is pressed again after 5 s, and
//! an Enter or an Up never is (T-415): the window may have taken it, its screen lagging,
//! and a second one would answer what comes next. The question is then the human's.
//! The watch ends once the window is past the question: its harness's prompt on its
//! screen, or its record grown since it opened (a window
//! opened again on its conversation has that conversation in its record from the
//! first look); a window that never asked is let go after [`ASKS_WITHIN_MS`], but for one
//! still owed its launch's first message, which only its prompt lets go, within its
//! launch's time. A look
//! the host shows no screen for says nothing either way: a window that asked is held
//! until one does, and the human is told once that lasts [`UNREAD_MS`]. A question
//! whose choice the reader cannot place is pressed nothing, and the human is told of
//! it as of one outside the project's folder. The question asked again of a folder
//! already trusted by a folder above it, whose own settings grant more than its trust
//! ([`Question::Grants`]: a yes lets them apply without asking), is answered as the
//! first is (the owner's decision of 2026-10-23): yes, where the folder its yes records
//! is the project's folder or below it; elsewhere, or where its choice cannot be
//! placed, nothing is pressed and the human is told of it in words of its own, once
//! for the windows that ask it together. Claude Code's dialog of a server of the
//! project's `.mcp.json` it was never told to use, asked once the folder is trusted
//! ([`Question::Servers`]), is answered as the owner decided on 2026-10-23: the server
//! is used, where the folder a yes to the first question records is the project's and
//! no folder above the project's has a `.mcp.json` that names a server the project's
//! settings never used or left out (Claude Code asks of those servers in the same
//! dialog); elsewhere, or where its choice cannot be placed, it is held and told of as
//! the grants are. Each server used is told of once its window's folder says so, by the
//! words the dialog named it in; one the folder left out while only ConsensFlow pressed
//! is told of too, as no later window asks of it. A launch that is to
//! say its role first never meets the question: only a harness that keeps the
//! instructions its conversation began with is told its role so, and that is Codex,
//! which never asks as ConsensFlow opens it.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;
use std::time::Duration;

use cf_harness::contract::{Pane, Question, Trusts, Window};
use cf_harness::records::Reading;
use cf_ledger::{ParticipantView, ProjectView};
use cf_proto::panes::{SnapshotRequest, TAIL_LINES};
use serde_json::{json, Value};

use crate::dispatcher::Dispatcher;
use crate::record::Record;
use crate::runtime::returning;
use crate::seams::EngineError;
use crate::shown::Shown;

mod notes;
mod settle;
mod watch;

use watch::Watch;

/// How often the screen is looked at.
const LOOK: Duration = Duration::from_millis(500);

/// How long a window that never held still, its record empty, is watched for the
/// question: a harness asks it as it starts, before anything else is drawn.
pub(crate) const ASKS_WITHIN_MS: i64 = 30_000;

/// How long a chief's window owed its first message may show neither the question nor
/// its prompt before the human is told, once (T-629).
const NO_PROMPT_MS: i64 = 120_000;

/// How many looks a screen the last key went into may stay as it was before the key
/// is pressed again, where it may be ([`presses_again`]): the window did not take it.
/// Where it may not, the question is the human's from then (T-415): a screen that lags
/// may hide a key it took.
const PRESS_AGAIN_LOOKS: u32 = 10;

/// How many looks in a row a window that asked may show neither the question nor its
/// prompt before it counts as past the question: 12 s, since a resumed window draws
/// its conversation before its prompt, and a yes is banked only at the prompt.
const GONE_LOOKS: u32 = 24;

/// How many looks in a row a question ConsensFlow does not answer stays before the
/// human is told of it: 5 s. The question stays until it is answered; a conversation
/// drawn before its prompt may quote it, its cursor and all, for a look or three (1.5 s
/// on a chief's, `cargo xtask live folder-trust`), and a redraw caught halfway shows
/// its choice where the reader cannot place it for a look.
const TELL_LOOKS: u32 = 10;

/// How long the host may show no screen of a window that asked before the human is
/// told: 30 s, the time a window that never asked is watched for the question, longer
/// than a host's passing trouble. The window is held still: nothing goes into a
/// question no look has read.
const UNREAD_MS: i64 = 30_000;

/// How many keys a question is given again into a screen they did not move before it
/// goes to the human: a screen that is not moved by them is one the engine does not
/// read right. The keys that moved it are not counted: a dialog of new servers takes
/// one a server (T-415).
const MOST_KEYS: usize = 6;

/// How many keys a question is given in all before it goes to the human: more than
/// any dialog's choices, so that a reading that only ever moves never presses for good.
const MOST_MOVES: usize = 64;

/// Why the board reads a window starting while it asks: nothing goes into it then,
/// nor while it may still ask; and while it asks of new servers of `.mcp.json`.
pub(crate) const HELD: &str = "the window asks whether to trust its folder";
pub(crate) const HELD_SERVERS: &str = "the window asks whether to use the project's MCP servers";

/// Where a window stands with its harness's question about trusting its folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Trusting {
    /// No question is coming: its harness never asks one, or it was answered.
    #[default]
    Past,
    /// The window may still ask it: nothing goes in yet.
    Looking,
    /// The window asks it: it is being answered, or the human is to.
    Asked,
    /// The window asks whether to use new servers of `.mcp.json`, as it does once its
    /// folder is trusted: as [`Trusting::Asked`].
    AskedServers,
}

impl Trusting {
    /// Whether the window asks one of its harness's questions.
    pub(crate) fn asks(self) -> bool {
        matches!(self, Trusting::Asked | Trusting::AskedServers)
    }
}

/// What the engine keeps across windows: the folders each project's notes
/// spoke of already, so that the windows that ask of one folder at once (every
/// member of a project opened in it) are spoken of once, and the folders the
/// human said yes to in a window, which the project's other windows that ask of
/// them are answered for. The question of a folder's grants is spoken of apart
/// (`told_grants`), until the window it was told of is past it; so is the dialog of
/// new servers (`told_servers`), that the human is to answer it; and each server
/// ConsensFlow used, once, by the words the dialog named it in (`used`).
#[derive(Debug, Default)]
pub(crate) struct TrustState {
    told: RefCell<HashSet<(i64, String, bool)>>,
    told_grants: RefCell<HashSet<(i64, String)>>,
    told_servers: RefCell<HashSet<(i64, String)>>,
    used: RefCell<HashSet<(i64, String, String)>>,
    accepted: RefCell<HashSet<(i64, String)>>,
}

impl TrustState {
    /// The human said yes to `folder` in a window of `project`, and no key of
    /// ConsensFlow's answered it. Where the question was read with its choice placed
    /// (`sure`) the project's other windows that ask of the folder are answered as they
    /// answered; where it was not, no key of ConsensFlow's answers them, and each is told
    /// of again.
    pub(crate) fn said_yes(&self, project: i64, folder: &str, sure: bool) {
        if sure {
            self.accepted
                .borrow_mut()
                .insert((project, folder.to_owned()));
        } else {
            self.told
                .borrow_mut()
                .remove(&(project, folder.to_owned(), false));
        }
    }
}

/// Which of its harness's questions a window asks: they come one after another in a
/// window (the first question, then the dialog of new servers), each given its own
/// keys and its own time before the human is told.
fn dialog(question: Question) -> u8 {
    match question {
        Question::Toward(_) | Question::Unplaced => 0,
        Question::Grants(_) => 1,
        Question::Servers(_) => 2,
    }
}

/// Whether `record`'s launch still owes its window its first message, handed over
/// once the window is past the question ([`Dispatcher::hand_first`]): its Enter would
/// answer a question drawn late, under load, with the choice the question opens on,
/// "No, exit" (Calliope's review of T-432, T-520). Such a window is watched until its
/// prompt shows, within its launch's time (`arrival`), never let go for having asked
/// nothing yet.
fn owes_first(record: &Record) -> bool {
    record
        .delivery
        .borrow()
        .delivering
        .as_ref()
        .is_some_and(|delivering| delivering.owed.is_some())
}

/// What one look at the window found.
enum Seen {
    /// The question, the key toward a yes where its choice could be placed, and the
    /// lines it was read in.
    Asks(Question, Vec<String>),
    /// The harness's prompt: the window is past any question. Whether the question's
    /// choices are gone from the screen: a conversation that quoted them before its
    /// prompt still shows them, and a question answered does not.
    Prompt { cleared: bool },
    /// A screen still drawing, or one that is neither.
    Other,
    /// No screen: the host did not show it, which says nothing of the question.
    Unread,
}

/// Where a watch ended: whether its window's prompt was on its screen, and whether the
/// question's choices were gone from it.
struct Ended {
    at_prompt: bool,
    cleared: bool,
}

impl Ended {
    /// Past the question without a prompt to read, or no longer the record's.
    const PAST: Ended = Ended {
        at_prompt: false,
        cleared: false,
    };
}

/// The folder a yes was said to in a window that ended, and whether its question was
/// read with its choice placed, where one was said: the window was asking when it ended,
/// none of ConsensFlow's keys had answered it, and the screen the host kept of it is its
/// harness's prompt with the question's choices gone from it, as the watch asks of a
/// prompt (`cleared`): a conversation that quoted the question may be drawn above one.
/// The screen is looked at every half second, and a window can be gone between two looks,
/// its task done and its window closed at once, or ended of itself: the human's yes is
/// then read off the screen it ended with, and banked ([`TrustState::said_yes`]) as the
/// watch banks one it saw. A No leaves the question, or nothing, on the screen it ends
/// with. Only the bank moves here: the watch of a window that ended breaks out of its
/// loop all the same, and its cleanup after the loop (the grants and servers notes
/// forgotten, the servers told of) runs as for any window, so none of it is done again.
pub(crate) fn yes_at_exit(record: &Record, shown: &Shown) -> Option<(String, bool)> {
    let part = record.window.borrow();
    if !part.trust.asks() || part.trust_keys > 0 {
        return None;
    }
    let window = part.window.as_ref()?;
    let Trusts { folder, .. } = window.trusts()?;
    let lines = shown.lines();
    (window.shows_prompt(lines) && !window.names_trust_choices(lines))
        .then(|| (folder.clone(), part.trust_sure))
}

impl Dispatcher {
    /// A window the engine is about to close itself (a Close, a switch, a chief's
    /// give-up), where it was asking and none of ConsensFlow's keys answered it: what
    /// its screen shows is asked before the kill, for the engine settles such a close
    /// with an exit of its own that says nothing of the screen, and a yes the human
    /// said just before it is banked from what is read ([`yes_at_exit`]). The close
    /// is no worse for a look that fails.
    pub(crate) async fn bank_yes_before_close(&self, record: &Rc<Record>, pane: &Pane) {
        let asking = {
            let part = record.window.borrow();
            part.trust.asks() && part.trust_keys == 0
        };
        if !asking {
            return;
        }
        let shown = self.look_at(pane).await;
        // The window may have gone, or been opened again, while the host answered.
        if record.window.borrow().pane.as_ref() != Some(pane) {
            return;
        }
        let Some((folder, sure)) = yes_at_exit(record, &shown) else {
            return;
        };
        if let Ok(Some(project)) = self.project_of(record.id) {
            self.trust.said_yes(project.id, &folder, sure);
        }
    }

    /// Whether `record`'s window, about to be handed its first message, shows one of
    /// its harness's questions on a look at its screen now: the question about its
    /// folder's trust or grants, or the dialog of new servers, drawn after the watch let
    /// it go (late, or one after another). Where it does, the watch is begun again, and
    /// answers it or tells the human as it does any; nothing goes into a dialog. A window
    /// whose harness asks none, or a screen that cannot be read, shows none.
    pub(crate) async fn asks_before_its_first_message(
        self: &Rc<Self>,
        record: &Rc<Record>,
        pane: &Pane,
    ) -> Result<bool, EngineError> {
        let window = record.window.borrow().window.clone();
        let Some(window) = window.filter(|window| window.trusts().is_some()) else {
            return Ok(false);
        };
        if !matches!(self.trust_look(pane, &*window).await, Seen::Asks(..)) {
            return Ok(false);
        }
        if !self.shows(record, pane) {
            return Ok(true);
        }
        let (Some(project), Some(participant)) =
            (self.project_of(record.id)?, self.participant_of(record)?)
        else {
            return Ok(true);
        };
        self.watch_trust(record, &project, &participant, pane).await;
        Ok(true)
    }

    /// Watches a window that was just opened for its harness's question about
    /// trusting its folder, apart from its launch, where its harness may ask one.
    pub(crate) async fn watch_trust(
        self: &Rc<Self>,
        record: &Rc<Record>,
        project: &ProjectView,
        participant: &ParticipantView,
        pane: &Pane,
    ) {
        let window = record.window.borrow().window.clone();
        let Some(window) = window.filter(|window| window.trusts().is_some()) else {
            return;
        };
        {
            let mut part = record.window.borrow_mut();
            part.trust = Trusting::Looking;
            part.trust_keys = 0;
            part.trust_sure = false;
        }
        let (this, held, project, participant, pane) = (
            Rc::clone(self),
            Rc::clone(record),
            project.clone(),
            participant.clone(),
            pane.clone(),
        );
        drop(
            crate::runtime::begin(&*self.seams.spawn, async move {
                if let Err(cause) = this
                    .trust_until_past(&held, &project, &participant, &pane, &window)
                    .await
                {
                    this.write_down(&cause);
                }
            })
            .await,
        );
    }

    /// The watch: until the window is past the question, or is no longer the
    /// record's. A window is past it once its harness's prompt is on its screen, or
    /// its record has grown since it opened (a window opened again on its
    /// conversation has the whole conversation in its record from the first look),
    /// or, where it never asked and is owed no first message, once [`ASKS_WITHIN_MS`]
    /// passed.
    async fn trust_until_past(
        self: &Rc<Self>,
        record: &Rc<Record>,
        project: &ProjectView,
        participant: &ParticipantView,
        pane: &Pane,
        window: &Rc<dyn Window>,
    ) -> Result<(), EngineError> {
        let Some(trusts) = window.trusts().cloned() else {
            return Ok(());
        };
        let mut watch = Watch::new(
            self.now(),
            record,
            project,
            participant,
            pane,
            window,
            trusts,
        );
        let ended = loop {
            // A window no longer the record's ends the watch as any other: what it was
            // told of is forgotten after, and nothing else is done for it.
            if !self.shows(record, pane) {
                break Ended::PAST;
            }
            if let Some(ended) = self.trust_look_and_answer(&mut watch).await? {
                break ended;
            }
            self.seams.time.sleep(LOOK).await;
        };
        self.settle_trust(watch, ended)
    }

    /// Whether `pane` is still the record's window.
    fn shows(&self, record: &Rc<Record>, pane: &Pane) -> bool {
        !self.forgotten(record) && record.window.borrow().pane.as_ref() == Some(pane)
    }

    /// One look at the window's screen.
    async fn trust_look(&self, pane: &Pane, window: &dyn Window) -> Seen {
        let request = SnapshotRequest {
            id: pane.id.clone(),
            generation: pane.generation,
            tail: Some(TAIL_LINES),
            bottom: None,
        };
        let Ok(body) = serde_json::to_value(&request) else {
            return Seen::Unread;
        };
        let Ok(answer) = returning(self.seams.host.request("pane.snapshot", body)).await else {
            return Seen::Unread;
        };
        if answer.get("ok") != Some(&Value::Bool(true)) {
            return Seen::Unread;
        }
        let shown = Shown::from_snapshot(&answer);
        let lines = shown.lines();
        if window.shows_prompt(lines) {
            return Seen::Prompt {
                cleared: !window.names_trust_choices(lines),
            };
        }
        match window.toward_trust(lines) {
            Some(question) => Seen::Asks(question, lines.to_vec()),
            None => Seen::Other,
        }
    }

    /// How many items the harness's record of the window holds now: none where it
    /// could not be read.
    async fn record_length(&self, window: &Rc<dyn Window>) -> Option<usize> {
        let observed = returning(window.observe()).await.ok()?;
        matches!(observed.reading.as_deref(), Some(Reading::Known(_)))
            .then(|| observed.items().len())
    }

    /// Presses `key` into the window, as a person does: whether the host took it.
    async fn press_toward_trust(&self, pane: &Pane, key: &[u8]) -> bool {
        let body = json!({ "id": pane.id, "generation": pane.generation, "bytes": key });
        returning(self.seams.host.request("pane.input", body))
            .await
            .is_ok_and(|answer| answer.get("ok") == Some(&Value::Bool(true)))
    }
}

#[cfg(test)]
mod tests;
