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
//! once, and the window waits for their answer in it; once they said yes in one
//! window, the project's other windows that ask of that folder are answered as
//! they answered; a window that ended before the screen was looked at again, its
//! task done and its window closed at once, is read off the screen the host kept
//! of it ([`yes_at_exit`]). Either way nothing goes into the window, and nothing gives its
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
//! is never given a second. The watch ends once the window is past the question: its harness's
//! prompt on its screen, or its record grown since it opened (a window opened again
//! on its conversation has that conversation in its record from the first look); a
//! window that never asked is let go after [`ASKS_WITHIN_MS`]. A launch that is to
//! say its role first never meets the question: only a harness that keeps the
//! instructions its conversation began with is told its role so, and that is Codex,
//! which never asks as ConsensFlow opens it.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;
use std::time::Duration;

use cf_harness::contract::{Pane, Trusts, Window};
use cf_harness::records::Reading;
use cf_ledger::{NewNote, ParticipantView, ProjectView};
use cf_proto::panes::{SnapshotRequest, TAIL_LINES};
use serde_json::{json, Value};

use crate::dispatcher::Dispatcher;
use crate::record::Record;
use crate::runtime::returning;
use crate::seams::EngineError;
use crate::shown::Shown;

/// How often the screen is looked at.
const LOOK: Duration = Duration::from_millis(500);

/// How long a window that never held still, its record empty, is watched for the
/// question: a harness asks it as it starts, before anything else is drawn.
pub(crate) const ASKS_WITHIN_MS: i64 = 30_000;

/// How many looks a screen the last key went into may stay as it was before the key
/// is pressed again: the window did not take it.
const PRESS_AGAIN_LOOKS: u32 = 10;

/// How many looks in a row a window that asked may show neither the question nor its
/// prompt before it counts as past the question: 12 s, since a resumed window draws
/// its conversation before its prompt, and a yes is banked only at the prompt.
const GONE_LOOKS: u32 = 24;

/// How many keys are pressed toward a yes before the question goes to the human:
/// a screen that is not moved by them is one the engine does not read right.
const MOST_KEYS: usize = 6;

/// Why the board reads a window starting while it asks: nothing goes into it then,
/// nor while it may still ask.
pub(crate) const HELD: &str = "the window asks whether to trust its folder";

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
}

/// What the engine keeps across windows: the folders each project's notes
/// spoke of already, so that the windows that ask of one folder at once (every
/// member of a project opened in it) are spoken of once, and the folders the
/// human said yes to in a window, which the project's other windows that ask of
/// them are answered for.
#[derive(Debug, Default)]
pub(crate) struct TrustState {
    told: RefCell<HashSet<(i64, String, bool)>>,
    accepted: RefCell<HashSet<(i64, String)>>,
}

impl TrustState {
    /// The human said yes to `folder` in a window of `project`: its other windows that
    /// ask of it are answered as they answered.
    pub(crate) fn accept(&self, project: i64, folder: String) {
        self.accepted.borrow_mut().insert((project, folder));
    }
}

/// What one look at the window found.
enum Seen {
    /// The question, the key toward a yes, and the lines it was read in.
    Asks(&'static [u8], Vec<String>),
    /// The harness's prompt: the window is past any question.
    Prompt,
    /// A screen still drawing, or one that is neither.
    Other,
}

/// The folder a yes was said to in a window that ended, where one was: the window was
/// asking when it ended, none of ConsensFlow's keys had answered it, and the screen the
/// host kept of it is its harness's prompt. The screen is looked at every half second, and
/// a window can be gone between two looks, its task done and its window closed at once, or
/// ended of itself: the human's yes is then read off the screen it ended with, and banked
/// for the project's other windows as one the watch saw is. A No leaves the question, or
/// nothing, on the screen it ends with.
pub(crate) fn yes_at_exit(record: &Record, shown: &Shown) -> Option<String> {
    let part = record.window.borrow();
    if part.trust != Trusting::Asked || part.trust_keys > 0 {
        return None;
    }
    let window = part.window.as_ref()?;
    let Trusts { folder, .. } = window.trusts()?;
    window.shows_prompt(shown.lines()).then(|| folder.clone())
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
            part.trust == Trusting::Asked && part.trust_keys == 0
        };
        if !asking {
            return;
        }
        let shown = self.look_at(pane).await;
        // The window may have gone, or been opened again, while the host answered.
        if record.window.borrow().pane.as_ref() != Some(pane) {
            return;
        }
        let Some(folder) = yes_at_exit(record, &shown) else {
            return;
        };
        if let Ok(Some(project)) = self.project_of(record.id) {
            self.trust.accept(project.id, folder);
        }
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
    /// or, where it never asked, once [`ASKS_WITHIN_MS`] passed.
    async fn trust_until_past(
        self: &Rc<Self>,
        record: &Rc<Record>,
        project: &ProjectView,
        participant: &ParticipantView,
        pane: &Pane,
        window: &Rc<dyn Window>,
    ) -> Result<(), EngineError> {
        let Some(Trusts { folder, inside }) = window.trusts().cloned() else {
            return Ok(());
        };
        let began = self.now();
        // What the record held at the first look that could read it: a window opened
        // again on its conversation holds it all from the start, and a look that could
        // not read it says nothing of growth.
        let mut opened_with: Option<usize> = None;
        // The screen the last key was pressed on, and how many looks have found it
        // since: a key goes into a screen that moved since the last one, so a slow
        // redraw is never given a second, or into one that stayed as it was for
        // [`PRESS_AGAIN_LOOKS`], whose window did not take the key.
        let mut pressed_on: Option<(Vec<String>, u32)> = None;
        // How many looks in a row found the question gone with no prompt on the screen.
        let mut gone = 0;
        let at_prompt = loop {
            if !self.shows(record, pane) {
                return Ok(());
            }
            let length = self.record_length(window).await;
            if opened_with.is_none() {
                opened_with = length;
            }
            let grown = matches!((opened_with, length), (Some(from), Some(now)) if now > from);
            match self.trust_look(pane, &**window).await {
                Seen::Asks(key, lines) => {
                    record.window.borrow_mut().trust = Trusting::Asked;
                    gone = 0;
                    self.changed();
                    // A folder the human said yes to in another window of the
                    // project is answered as they answered it.
                    let accepted = self
                        .trust
                        .accepted
                        .borrow()
                        .contains(&(project.id, folder.clone()));
                    if (inside || accepted) && record.window.borrow().trust_keys < MOST_KEYS {
                        if inside {
                            self.tell_trust_once(project, &folder, true, || {
                                format!(
                                    "@{}'s window asked whether to trust {folder}; ConsensFlow answered yes, as the project's folder is trusted by choosing it.",
                                    participant.handle
                                )
                            })?;
                        }
                        let press = match pressed_on.as_mut() {
                            Some((on, looks)) if *on == lines => {
                                *looks += 1;
                                *looks >= PRESS_AGAIN_LOOKS
                            }
                            _ => true,
                        };
                        if press && self.press_toward_trust(pane, key).await {
                            record.window.borrow_mut().trust_keys += 1;
                            pressed_on = Some((lines, 0));
                        }
                    } else {
                        self.tell_trust_once(project, &folder, false, || {
                            if inside {
                                format!(
                                    "@{}'s window asks whether to trust {folder}, and ConsensFlow could not answer it. Answer it in that window: what waits for @{} goes on by itself once you have.",
                                    participant.handle, participant.handle
                                )
                            } else {
                                format!(
                                    "@{}'s window asks whether to trust {folder}, which is outside the project's folder {}. Answer it in that window: what waits for @{} goes on by itself once you have.",
                                    participant.handle, project.directory, participant.handle
                                )
                            }
                        })?;
                    }
                }
                Seen::Prompt => break true,
                Seen::Other => {
                    // Its record grown past what it held: past the question, whoever
                    // answered it (or a process before it writing still).
                    if grown {
                        break false;
                    }
                    let asked = record.window.borrow().trust == Trusting::Asked;
                    if asked {
                        // The question gone for good with no prompt to read (a prompt
                        // drawn otherwise than the harness's reader knows): past it.
                        gone += 1;
                        if gone >= GONE_LOOKS {
                            break false;
                        }
                    } else if self.now() - began > ASKS_WITHIN_MS {
                        break false;
                    }
                }
            }
            self.seams.time.sleep(LOOK).await;
        };
        let asked = record.window.borrow().trust == Trusting::Asked;
        // The question left a window now at its prompt, and no key of ConsensFlow's
        // answered it: the human said yes, for every window of the project. Nothing
        // else says so: not the question's absence, which a No leaves on the way out,
        // nor the record's growth, which may be a process before the window's.
        if asked && record.window.borrow().trust_keys == 0 && at_prompt {
            self.trust.accept(project.id, folder.clone());
        }
        if !self.shows(record, pane) {
            return Ok(());
        }
        record.window.borrow_mut().trust = Trusting::Past;
        // A first message the question held gets its whole time from now.
        if asked {
            if let Some(delivering) = record.delivery.borrow_mut().delivering.as_mut() {
                delivering.since = self.now();
            }
        }
        self.changed();
        Ok(())
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
        };
        let Ok(body) = serde_json::to_value(&request) else {
            return Seen::Other;
        };
        let Ok(answer) = returning(self.seams.host.request("pane.snapshot", body)).await else {
            return Seen::Other;
        };
        let shown = Shown::from_snapshot(&answer);
        let lines = shown.lines();
        if window.shows_prompt(lines) {
            return Seen::Prompt;
        }
        match window.toward_trust(lines) {
            Some(key) => Seen::Asks(key, lines.to_vec()),
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

    /// A note to the human about `folder`, the first time the project's windows
    /// ask of it: that ConsensFlow `answered` it, or that the human is to.
    fn tell_trust_once(
        &self,
        project: &ProjectView,
        folder: &str,
        answered: bool,
        body: impl FnOnce() -> String,
    ) -> Result<(), EngineError> {
        if !self
            .trust
            .told
            .borrow_mut()
            .insert((project.id, folder.to_owned(), answered))
        {
            return Ok(());
        }
        self.seams.ledger.borrow_mut().note(
            project.id,
            &NewNote {
                from: None,
                to: "human".to_owned(),
                task: None,
                body: body(),
            },
        )?;
        self.changed();
        Ok(())
    }
}

#[cfg(test)]
mod tests;
