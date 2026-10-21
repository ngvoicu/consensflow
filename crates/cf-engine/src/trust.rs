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
//! they answered. Either way nothing goes into the window, and nothing gives its
//! launch up, while the question is on its screen ([`Trusting`]); once it has gone
//! the launch goes on as any other, its first message given its whole time from
//! then. ConsensFlow writes none of the harness's own files: Claude Code rewrites
//! `~/.claude.json` from every running window, and a second writer could lose what
//! one of them wrote.
//!
//! The screen is watched apart from the launch, from the moment the window opens
//! (Claude Code shows the question after its launch has come up), a look every
//! half second: a key is pressed only into a screen
//! that has held still since it drew, so that each press meets the question as the
//! last look read it. The watch ends once the screen holds still with no question,
//! the harness's record has begun, or [`ASKS_WITHIN_MS`] passed with none seen.

use std::cell::RefCell;
use std::collections::HashSet;
use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

use cf_harness::contract::{Pane, Window};
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

/// How long the screen must have printed nothing for a look to read it as drawn:
/// the engine's own measure of a drawn screen.
const QUIET_MS: f64 = 1_500.0;

/// How long a window that never held still, its record empty, is watched for the
/// question: a harness asks it as it starts, before anything else is drawn.
pub(crate) const ASKS_WITHIN_MS: i64 = 30_000;

/// How many keys are pressed toward a yes before the question goes to the human:
/// a screen that is not moved by them is one the engine does not read right.
const MOST_KEYS: usize = 6;

/// Why the board reads a window starting while it may ask, or asks: nothing goes
/// into it then.
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

/// What one look at the window found.
enum Seen {
    /// The question, and the key toward a yes.
    Asks(&'static [u8]),
    /// A screen that held still with no question on it.
    Clear,
    /// A screen still drawing, or none.
    Drawing,
}

impl Dispatcher {
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
        record.window.borrow_mut().trust = Trusting::Looking;
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
    /// record's.
    async fn trust_until_past(
        self: &Rc<Self>,
        record: &Rc<Record>,
        project: &ProjectView,
        participant: &ParticipantView,
        pane: &Pane,
        window: &Rc<dyn Window>,
    ) -> Result<(), EngineError> {
        let folder = window.trusts().unwrap_or_default().to_owned();
        let inside = Path::new(&folder).starts_with(&project.directory);
        let began = self.now();
        let mut keys = 0;
        loop {
            if !self.shows(record, pane) {
                return Ok(());
            }
            match self.trust_look(pane, &**window).await {
                Seen::Asks(key) => {
                    record.window.borrow_mut().trust = Trusting::Asked;
                    // A folder the human said yes to in another window of the
                    // project is answered as they answered it.
                    let accepted = self
                        .trust
                        .accepted
                        .borrow()
                        .contains(&(project.id, folder.clone()));
                    if (inside || accepted) && keys < MOST_KEYS {
                        if inside {
                            self.tell_trust_once(project, &folder, true, || {
                                format!(
                                    "@{}'s window asked whether to trust {folder}; ConsensFlow answered yes, as the project's folder is trusted by choosing it.",
                                    participant.handle
                                )
                            })?;
                        }
                        if self.press_toward_trust(pane, key).await {
                            keys += 1;
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
                Seen::Clear => break,
                // A record begun is a window past the question, whoever answered
                // it: its window may close before its screen holds still again.
                Seen::Drawing => {
                    let asked = record.window.borrow().trust == Trusting::Asked;
                    if (!asked && self.now() - began > ASKS_WITHIN_MS)
                        || self.record_begun(window).await
                    {
                        break;
                    }
                }
            }
            self.seams.time.sleep(LOOK).await;
        }
        // The question left a window that stayed, and no key of ConsensFlow's
        // answered it: the human said yes, for every window of the project.
        let asked = record.window.borrow().trust == Trusting::Asked;
        if asked && keys == 0 {
            self.trust
                .accepted
                .borrow_mut()
                .insert((project.id, folder.clone()));
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
            return Seen::Drawing;
        };
        let Ok(answer) = returning(self.seams.host.request("pane.snapshot", body)).await else {
            return Seen::Drawing;
        };
        let drawn = match answer.get("outputQuietMs") {
            // A host that cannot tell is taken as drawn, as the engine takes it.
            None => true,
            Some(quiet) => quiet.as_f64().is_some_and(|quiet| quiet >= QUIET_MS),
        };
        let shown = Shown::from_snapshot(&answer);
        if !drawn || shown.lines().is_empty() {
            return Seen::Drawing;
        }
        match window.toward_trust(shown.lines()) {
            Some(key) => Seen::Asks(key),
            None => Seen::Clear,
        }
    }

    /// Whether the harness's record of the window has begun: a window past any
    /// question.
    async fn record_begun(&self, window: &Rc<dyn Window>) -> bool {
        returning(window.observe())
            .await
            .is_ok_and(|observed| !observed.items().is_empty())
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
