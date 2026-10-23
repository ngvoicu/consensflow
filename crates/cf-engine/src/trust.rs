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
//! answer them (a cursor the reader cannot place), told of again. Either way nothing goes into the window, and nothing gives its
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
//! is never given a second. The watch ends once the window is past the question:
//! its harness's prompt on its screen, or its record grown since it opened (a window
//! opened again on its conversation has that conversation in its record from the
//! first look); a window that never asked is let go after [`ASKS_WITHIN_MS`]. A look
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
//! no folder above the project's has a `.mcp.json` of its own (Claude Code asks of
//! those servers in the same dialog); elsewhere, or where its choice cannot be placed,
//! it is held and told of as the grants are. A launch that is to
//! say its role first never meets the question: only a harness that keeps the
//! instructions its conversation began with is told its role so, and that is Codex,
//! which never asks as ConsensFlow opens it.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;
use std::time::Duration;

use cf_harness::contract::{Pane, Question, Trusts, Window};
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
/// them are answered for. The question of a folder's grants is spoken of apart
/// (`told_grants`), until the window it was told of is past it; so is the dialog of
/// new servers (`told_servers`): once that ConsensFlow used them, and, until the
/// window it was told of is past it, that the human is to answer it.
#[derive(Debug, Default)]
pub(crate) struct TrustState {
    told: RefCell<HashSet<(i64, String, bool)>>,
    told_grants: RefCell<HashSet<(i64, String)>>,
    told_servers: RefCell<HashSet<(i64, String, bool)>>,
    accepted: RefCell<HashSet<(i64, String)>>,
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
        let Some(Trusts {
            folder,
            inside,
            servers_above,
        }) = window.trusts().cloned()
        else {
            return Ok(());
        };
        let began = self.now();
        // What the record held at the first look that could read it: a window opened
        // again on its conversation holds it all from the start, and a look that could
        // not read it says nothing of growth.
        let mut opened_with: Option<usize> = None;
        // The keys pressed in the watch, and into the question it asks now.
        let mut keys = 0;
        let mut asked_keys = 0;
        let mut asks_now = None;
        // The screen the last key was pressed on, and how many looks have found it
        // since: a key goes into a screen that moved since the last one, so a slow
        // redraw is never given a second, or into one that stayed as it was for
        // [`PRESS_AGAIN_LOOKS`], whose window did not take the key.
        let mut pressed_on: Option<(Vec<String>, u32)> = None;
        // How many looks in a row found the question gone with no prompt on the screen,
        // and how many found the question.
        let mut gone = 0;
        let mut asking = 0;
        // Since when the host has shown no screen of a window that asked, and whether
        // the human was told so: once a watch, so a second time its screen goes unread
        // in the same watch is told of to nobody.
        let mut unread_since = None;
        let mut told_unread = false;
        // Whether the question was read with its choice placed: only that is a question
        // whose yes is carried to the project's other windows, the only ones a key of
        // ConsensFlow's can answer.
        let mut sure = false;
        // Whether the human was told of the question of the folder's grants, or of the
        // dialog of new servers: once its window is past it, the next window that asks it
        // is told of again.
        let mut grants = false;
        let mut servers = false;
        let (at_prompt, cleared) = loop {
            // A window no longer the record's ends the watch as any other: what it was
            // told of is forgotten below, and nothing else is done for it.
            if !self.shows(record, pane) {
                break (false, false);
            }
            let length = self.record_length(window).await;
            if opened_with.is_none() {
                opened_with = length;
            }
            let grown = matches!((opened_with, length), (Some(from), Some(now)) if now > from);
            let seen = self.trust_look(pane, &**window).await;
            if !matches!(seen, Seen::Unread) {
                unread_since = None;
            }
            match seen {
                Seen::Asks(question, lines) => {
                    record.window.borrow_mut().trust = Trusting::Asked;
                    gone = 0;
                    self.changed();
                    // Another question than the last look's: its own keys, its own time.
                    if asks_now.replace(dialog(question)) != Some(dialog(question)) {
                        asked_keys = 0;
                        asking = 0;
                        pressed_on = None;
                    }
                    // A folder the human said yes to in another window of the
                    // project is answered as they answered it.
                    let accepted = self
                        .trust
                        .accepted
                        .borrow()
                        .contains(&(project.id, folder.clone()));
                    // A choice the reader cannot place is pressed nothing. A question
                    // not answered here is said nothing of until it has stayed so: a
                    // conversation drawn before its prompt may quote it.
                    asking += 1;
                    sure |= matches!(question, Question::Toward(_));
                    let pressing = match question {
                        Question::Toward(key) if (inside || accepted) && asked_keys < MOST_KEYS => {
                            Some(key)
                        }
                        Question::Grants(Some(key)) if inside && asked_keys < MOST_KEYS => {
                            Some(key)
                        }
                        Question::Servers(Some(key))
                            if inside && servers_above.is_none() && asked_keys < MOST_KEYS =>
                        {
                            Some(key)
                        }
                        _ => None,
                    };
                    // A window inside the project's folder whose conversation quotes the
                    // dialog, its cursor and all, before its prompt is read as asking
                    // and given a key toward a yes, which goes into its input box,
                    // and the human is told ConsensFlow answered (as since round 4).
                    if let Some(key) = pressing {
                        if matches!(question, Question::Servers(_)) {
                            self.tell_servers_once(project, &folder, true, || {
                                format!(
                                    "@{}'s window asked whether to use a new MCP server that the project's .mcp.json names; ConsensFlow said to use it, as the project's folder is trusted by choosing it. Claude Code starts the server's command, and keeps the choice in {folder}/.claude/settings.local.json.",
                                    participant.handle
                                )
                            })?;
                        } else if inside {
                            self.tell_trust_once(project, &folder, true, || {
                                if matches!(question, Question::Grants(_)) {
                                    format!(
                                        "@{}'s window asked whether to trust {folder} and let the settings of the project's folder {} apply without asking; ConsensFlow answered yes, as the project's folder is trusted by choosing it.",
                                        participant.handle, project.directory
                                    )
                                } else {
                                    format!(
                                        "@{}'s window asked whether to trust {folder}; ConsensFlow answered yes, as the project's folder is trusted by choosing it.",
                                        participant.handle
                                    )
                                }
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
                            keys += 1;
                            asked_keys += 1;
                            pressed_on = Some((lines, 0));
                        }
                    } else if asking >= TELL_LOOKS && matches!(question, Question::Servers(_)) {
                        // Its servers are not the project's alone, or its choice cannot be
                        // placed: the human's.
                        servers = true;
                        self.tell_servers_held(
                            project,
                            &folder,
                            inside,
                            servers_above.as_deref(),
                            participant,
                        )?;
                    } else if asking >= TELL_LOOKS && matches!(question, Question::Grants(_)) {
                        // Its yes would record a folder outside the project's, or its
                        // choice cannot be placed: the human's.
                        grants = true;
                        self.tell_grants_once(project, &folder, inside, participant)?;
                    } else if asking >= TELL_LOOKS {
                        // Once a project and folder: a conversation that quotes the
                        // question for longer (under load) is told as asking, and a
                        // real question of the same folder in another window then
                        // gets no note of its own, only its lane.
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
                Seen::Prompt { cleared } => break (true, cleared),
                Seen::Other => {
                    asking = 0;
                    // Its record grown past what it held: past the question, whoever
                    // answered it (or a process before it writing still).
                    if grown {
                        break (false, false);
                    }
                    let asked = record.window.borrow().trust == Trusting::Asked;
                    if asked {
                        // The question gone for good with no prompt to read (a prompt
                        // drawn otherwise than the harness's reader knows): past it.
                        gone += 1;
                        if gone >= GONE_LOOKS {
                            break (false, false);
                        }
                    } else if self.now() - began > ASKS_WITHIN_MS {
                        break (false, false);
                    }
                }
                // No screen says nothing of the question: a window that asked is held
                // until a look reads it past the question, and the human is told once it
                // was not read for [`UNREAD_MS`]; one that never did is let go after its
                // time.
                Seen::Unread => {
                    if grown {
                        break (false, false);
                    }
                    let asked = record.window.borrow().trust == Trusting::Asked;
                    if asked {
                        let since = *unread_since.get_or_insert(self.now());
                        let unseen = self.now() - since;
                        if unseen >= UNREAD_MS && !told_unread {
                            told_unread = true;
                            self.seams.ledger.borrow_mut().note(
                                project.id,
                                &NewNote {
                                    from: None,
                                    to: "human".to_owned(),
                                    task: None,
                                    body: format!(
                                        "@{}'s window asked whether to trust {folder}, and its screen could not be read for {} s since. Nothing goes into it until its screen can be read and shows it past the question; if it still asks, answer it in that window.",
                                        participant.handle,
                                        unseen / 1_000
                                    ),
                                },
                            )?;
                            self.changed();
                        }
                    } else if self.now() - began > ASKS_WITHIN_MS {
                        break (false, false);
                    }
                }
            }
            self.seams.time.sleep(LOOK).await;
        };
        let asked = record.window.borrow().trust == Trusting::Asked;
        // The question left a window now at its prompt, its choices gone from the
        // screen, and no key of ConsensFlow's answered it: the human said yes. Nothing
        // else says so: not the question's absence, which a No leaves on the way out,
        // nor the record's growth, which may be a process before the window's, nor a
        // prompt below a conversation that quoted the choices. Only the last lines are
        // read: a quote of the dialog, its cursor and all, that more of the conversation
        // pushes out of them before the prompt is drawn still banks a yes nobody gave.
        if asked && keys == 0 && at_prompt && cleared {
            if sure {
                // For every window of the project that asks of the folder.
                self.trust
                    .accepted
                    .borrow_mut()
                    .insert((project.id, folder.clone()));
            } else {
                // A question whose choice the reader could not place: no key of
                // ConsensFlow's answers the others, and each is told of again.
                self.trust
                    .told
                    .borrow_mut()
                    .remove(&(project.id, folder.clone(), false));
            }
        }
        // A no to the grants is the window's alone, and the next window asks again; so
        // is a window that went while it asked.
        if grants {
            self.forget_grants_told(project, &folder);
        }
        if servers {
            self.trust
                .told_servers
                .borrow_mut()
                .remove(&(project.id, folder.clone(), false));
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

    /// A note to the human about the question of `folder`'s grants that
    /// `participant`'s window asks, which ConsensFlow does not answer: `folder`, which a
    /// yes records, is outside the project's folder, or, `inside` it, the question's
    /// choice could not be placed. The first time the project's windows ask it since the
    /// last that did was past it.
    fn tell_grants_once(
        &self,
        project: &ProjectView,
        folder: &str,
        inside: bool,
        participant: &ParticipantView,
    ) -> Result<(), EngineError> {
        if !self
            .trust
            .told_grants
            .borrow_mut()
            .insert((project.id, folder.to_owned()))
        {
            return Ok(());
        }
        let handle = &participant.handle;
        self.seams.ledger.borrow_mut().note(
            project.id,
            &NewNote {
                from: None,
                to: "human".to_owned(),
                task: None,
                body: format!(
                    "@{handle}'s window asks whether the settings of its folder {directory} may apply without asking. Claude Code takes the folder as trusted, by a folder above it or by a record of its own, but has none for {folder}, which its yes records as trusted (the root of the repository the folder is in, or the folder itself out of one), and the folder's own settings grant more than its trust: tools they pre-approve, folders they add, or commands that mint the headers of MCP servers. {why} Answer it in that window: its no continues without them, its yes trusts {folder} and lets them apply. What waits for @{handle} goes on by itself once you have.",
                    directory = project.directory,
                    why = if inside {
                        "ConsensFlow could not answer it.".to_owned()
                    } else {
                        format!("{folder} is outside the project's folder, so ConsensFlow does not answer it.")
                    },
                ),
            },
        )?;
        self.changed();
        Ok(())
    }

    /// That the human was told of the question of `folder`'s grants no longer: the
    /// next window that asks it is told of again.
    fn forget_grants_told(&self, project: &ProjectView, folder: &str) {
        self.trust
            .told_grants
            .borrow_mut()
            .remove(&(project.id, folder.to_owned()));
    }

    /// A note to the human about the dialog of new servers that `participant`'s window
    /// asks, which ConsensFlow does not answer: `folder`, which a yes to the first
    /// question records, is outside the project's folder, or a folder above the
    /// project's has servers of its own (`servers_above`), or, neither, its choice could
    /// not be placed. The first time the project's windows ask it since the last that
    /// did was past it.
    fn tell_servers_held(
        &self,
        project: &ProjectView,
        folder: &str,
        inside: bool,
        servers_above: Option<&str>,
        participant: &ParticipantView,
    ) -> Result<(), EngineError> {
        let why = match (inside, servers_above) {
            (false, _) => format!(
                "The project's folder {} is in the repository {folder}, which is outside it, so ConsensFlow does not answer it.",
                project.directory
            ),
            (true, Some(above)) => format!(
                "{above}, above the project's folder, has a .mcp.json of its own, whose servers Claude Code asks of in the same dialog, so ConsensFlow does not answer it."
            ),
            (true, None) => "ConsensFlow could not answer it.".to_owned(),
        };
        self.tell_servers_once(project, folder, false, || {
            format!(
                "@{handle}'s window asks whether to use a new MCP server that its folder's .mcp.json, or a folder above it, names: a server it uses starts its command on this machine. {why} Answer it in that window: \"Use this MCP server\" uses it, \"Continue without using this MCP server\" goes on without it. What waits for @{handle} goes on by itself once you have.",
                handle = participant.handle
            )
        })
    }

    /// A note to the human about the dialog of new servers that the project's windows
    /// ask in `folder`: that ConsensFlow `answered` it, the first time; or that the human
    /// is to, the first time since the last window it was told of was past it.
    fn tell_servers_once(
        &self,
        project: &ProjectView,
        folder: &str,
        answered: bool,
        body: impl FnOnce() -> String,
    ) -> Result<(), EngineError> {
        if !self
            .trust
            .told_servers
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
