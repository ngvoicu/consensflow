//! Claude Code's windows. Each launch gets its own settings file: full
//! permission without the one-time dialog, and a Stop hook on every turn so
//! every finished turn is recorded.
//!
//! - The session id is ConsensFlow's: drawn for a fresh window, resumed for
//!   a known one.
//! - A message is pasted into a live window as if the human typed it, once
//!   its input box holds nothing the human typed and has not sent (the
//!   owner's choice, 2026-10-03: never pasted into their text). Claude's own
//!   peer inbox would bypass the input box, but Claude wraps each such
//!   message as a teammate's request from another Claude session, a hundred
//!   tokens of caution per delivery that misnames the human's own answers,
//!   so it is not used (2026-09-22).
//! - Claude's own `sessions/<pid>.json` says busy, idle or waiting (and
//!   why); the transcript holds the conversation and says whether the turn
//!   settled.
//! - A turn the daemon interrupted before Claude wrote a word of it leaves
//!   no record of its end, and Claude puts the message back in its input
//!   box, and out of the conversation it answers from: the window is read at
//!   rest where the daemon pressed the interrupt for that very turn
//!   ([`stopped`]), the look says it took the message back
//!   (`Observed::took_back`), and the box is cleared before the next paste,
//!   which would go in after the old text.
//! - A conversation the app's quit cut in a turn is found with that turn open,
//!   and no process in it. Resumed, Claude waits at its prompt, status idle,
//!   writes nothing of the old turn and closes it, with a message of its own
//!   (`No response requested.`), when the next one comes (Claude 2.1.296,
//!   `npm run live:resume-cut`). A window opened on such a conversation with no
//!   message to begin it is read at rest by that status while its record holds
//!   no more than it held when the window was opened ([`LeftOpen`]).
//! - The window's Claude process is the pane's own child when a status names
//!   it, so the first look already sees a /clear. Otherwise (Claude may run
//!   as another process the child starts) it is the process whose status
//!   first named the launch's conversation, which a /clear before that look
//!   leaves unknown. A /clear or /resume in the window changes the
//!   conversation that status names, never the process, so the window is
//!   followed to it.
//! - Every window starts with the MCP servers, claude.ai connectors and
//!   Claude in Chrome the human's own Claude Code has, the chief's and a
//!   member's alike (the owner's choice, 2026-10-06): the command line
//!   switches none off. ConsensFlow's own hooks come from `--settings`, not
//!   from MCP.

use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use cf_base::env::Env;
use cf_base::js;
use cf_base::text::window_text;
use cf_proto::agents::Harness;
use cf_proto::panes::Servers;
use cf_proto::specs::Variant;
use serde_json::Value;

use super::status::{self, State, Status};
use super::stopped::{self, Pressed};
use super::{install, paths};
use crate::contract::{
    Adapter, Admission, Held, Launch, Observed, Pane, PaneHost, Prepared, Readiness, Records,
    Skill, Waiting, Window, Work,
};
use crate::detect::executable;
use crate::records::{Options, Reading, Settlement};
use crate::seams::{self, Entropy, Services, Time};
use crate::shared::admission::admission;
use crate::shared::left_open::LeftOpen;
use crate::shared::record_state::switched_to;
use crate::shared::{pane, specmint, window_args};

/// Claude Code, as the engine launches its windows.
pub struct ClaudeAdapter {
    env: Env,
    records: Rc<dyn Records>,
    time: Rc<dyn Time>,
    entropy: Rc<dyn Entropy>,
    /// Where Claude keeps its own status of each of its processes, made
    /// whole once, as Node resolved it when it made the adapter; or why
    /// there is none.
    statuses: Result<String, String>,
}

impl ClaudeAdapter {
    /// The adapter of the windows the engine's `services` serve.
    pub fn new(services: &Services) -> Self {
        Self {
            env: services.env.clone(),
            records: Rc::clone(&services.records),
            time: Rc::clone(&services.time),
            entropy: Rc::clone(&services.entropy),
            statuses: status::folder(&services.env),
        }
    }
}

impl Adapter for ClaudeAdapter {
    fn prepare<'a>(&'a self, launch: &'a Launch<'a>) -> Work<'a, Result<Prepared, String>> {
        Box::pin(async move {
            let executable = executable(Harness::Claude, &self.env)?;
            let settings = install::settings(&self.env, launch.id, launch.role != "chief")?;
            let role = install::role(&self.env, launch)?;
            // Claude keeps a conversation only once something was said in
            // it: a window that closed before that (opened by hand, then
            // lost to a restart) has nothing to resume, and `--resume` would
            // exit at once. It starts afresh under the same id, so the
            // conversation stays bound.
            let session = match launch.resume {
                Some(resume) => resume.to_owned(),
                None => seams::uuid(&*self.entropy)?,
            };
            // Kept from Node on purpose: an environment that names no home
            // fails here, where Node read the process's own and opened a
            // window it would never see the status of.
            let statuses = self.statuses.clone()?;
            let resumable = match launch.resume {
                Some(resume) => self.records.has_transcript(Harness::Claude, resume).await?,
                None => false,
            };
            // What the record holds before this window exists is the old
            // process's: a turn it left open, which a window with no message
            // to begin one has to be told apart from by what the record holds
            // later. A window given a message begins its own turn.
            let left_open = if resumable && launch.message.is_none() {
                let before = self
                    .records
                    .look(Harness::Claude, &session, &Options::default())
                    .await;
                LeftOpen::of(&before)
            } else {
                None
            };
            let agent = launch.agent.unwrap_or_default();
            let message = launch.message.map(window_text);
            let open = if resumable {
                window_args::resume
            } else {
                window_args::start
            };
            // None only for a conversation of an empty id, which no ledger holds.
            let invocation = open(Harness::Claude, agent, Some(&session), message.as_deref())
                .ok_or_else(|| "a Claude window opens on a session id".to_owned())?;
            let mut argv = vec![executable];
            argv.extend(settings);
            argv.extend(role);
            argv.extend(invocation.args);
            Ok(Prepared {
                argv,
                env: Vec::new(),
                drop_env: invocation
                    .drop_env
                    .iter()
                    .map(|&name| name.to_owned())
                    .collect(),
                native_session: Some(session.clone()),
                window: Rc::new(ClaudeWindow {
                    records: Rc::clone(&self.records),
                    time: Rc::clone(&self.time),
                    statuses,
                    session: RefCell::new(session),
                    pid: Cell::new(None),
                    claude_pid: Cell::new(None),
                    asked: RefCell::new(None),
                    pressed: RefCell::new(None),
                    restored: Cell::new(false),
                    left_open: RefCell::new(left_open),
                }),
            })
        })
    }

    /// Claude Code runs each command of its Bash tool, in the foreground and in
    /// the background alike, in a process group of its own (a
    /// `/bin/zsh -c source …/shell-snapshots/snapshot-zsh-….sh …` that leads its
    /// group, seen on macOS with 3.0.1's workers), while its MCP servers and its
    /// own helpers (`caffeinate`) stay in the group of its window. It starts its
    /// servers as it takes its first message, after the window's turn is marked.
    /// What a server starts is the harness's whatever group it starts it in: the
    /// browser of `@playwright/mcp` and `chrome-devtools-mcp` leaves it.
    fn servers(&self) -> Servers {
        if self.env.on_windows() {
            Servers::Unknown
        } else {
            Servers::InGroup
        }
    }

    fn specmint(&self, variant: Variant) -> Result<Skill, String> {
        specmint::place(
            Harness::Claude,
            &self.env,
            variant,
            paths::global_specmint(&self.env, variant),
        )
    }
}

/// A Claude window, and what the engine told it of itself.
struct ClaudeWindow {
    records: Rc<dyn Records>,
    time: Rc<dyn Time>,
    statuses: String,
    /// The conversation the window shows: its launch's, or the one it was
    /// followed to.
    session: RefCell<String>,
    /// The pane's own child, once the host named it.
    pid: Cell<Option<u32>>,
    /// The Claude whose status first named the window's conversation.
    claude_pid: Cell<Option<u32>>,
    /// The id of the user's last message in the latest look: the turn the
    /// window is in, and what an interrupt is pressed over.
    asked: RefCell<Option<Arc<str>>>,
    /// The interrupt the daemon pressed for that turn, if it did.
    pressed: RefCell<Option<Pressed>>,
    /// The latest look read the window at rest by that press: Claude has put
    /// the message back in its input box.
    restored: Cell<bool>,
    /// The turn the record had open when the window was opened, while the
    /// window has begun no turn of its own: the old process's.
    left_open: RefCell<Option<LeftOpen>>,
}

impl ClaudeWindow {
    /// Claude's status of the window's process: the pane's own child's,
    /// when the host named it and Claude keeps one, else the status of the
    /// process that first named the window's conversation.
    fn status(&self) -> Option<Status> {
        let statuses = status::statuses(&self.statuses);
        let of = |pid: u32| {
            statuses
                .iter()
                .find(|(held, _)| *held == pid)
                .map(|(_, status)| status.clone())
        };
        if let Some(status) = self.pid.get().and_then(of) {
            return Some(status);
        }
        if self.claude_pid.get().is_none() {
            let session = self.session.borrow();
            let named = statuses
                .iter()
                .find(|(_, status)| status.session == *session);
            self.claude_pid.set(named.map(|(pid, _)| *pid));
        }
        self.claude_pid.get().and_then(of)
    }

    /// Whether `status` names another conversation than the window's.
    fn elsewhere(&self, status: &Status) -> bool {
        status.session != *self.session.borrow()
    }

    /// Whether the interrupt the daemon pressed for the turn the window is in
    /// is what ended it, though Claude wrote no record of that ([`stopped`]).
    fn stopped(&self, reading: &Reading) -> bool {
        let Reading::Known(record) = reading else {
            return false;
        };
        self.pressed
            .borrow()
            .as_ref()
            .is_some_and(|pressed| pressed.stopped(record, self.time.wall_ms()))
    }
}

impl Window for ClaudeWindow {
    fn opened(&self, pid: Option<u32>) {
        if pid.is_some() {
            self.pid.set(pid);
        }
    }

    fn follow(&self, session: &str) {
        session.clone_into(&mut self.session.borrow_mut());
        // What was pressed for was a turn of the conversation the window left,
        // and so was the turn its record had open.
        *self.asked.borrow_mut() = None;
        *self.pressed.borrow_mut() = None;
        self.restored.set(false);
        *self.left_open.borrow_mut() = None;
    }

    fn started(&self) -> Work<'_, Result<Option<String>, String>> {
        Box::pin(async { Ok(None) })
    }

    /// A paste waits for the window to be readable, on its conversation,
    /// with no paste going in.
    fn ready<'a>(
        &'a self,
        host: &'a dyn PaneHost,
        pane: &'a Pane,
    ) -> Work<'a, Result<Readiness, String>> {
        Box::pin(async move {
            if self.status().is_some_and(|live| self.elsewhere(&live)) {
                return Ok(Readiness::Held(Held::ShowsAnother));
            }
            let snapshot = pane::snapshot(host, pane)
                .await
                .map_err(|failed| failed.message)?;
            if snapshot.get("ok") != Some(&Value::Bool(true)) {
                // The host's own word for why, as a template writes it.
                let said = snapshot
                    .get("error")
                    .filter(|error| !error.is_null())
                    .map_or(Cow::Borrowed("no answer"), |error| js::text(Some(error)));
                let reason = format!("the window cannot be read: {said}");
                return Ok(Readiness::Held(Held::Because(reason)));
            }
            if js::truthy(snapshot.get("pasteInFlight")) {
                let reason = "a paste is on its way to the window".to_owned();
                return Ok(Readiness::Held(Held::Because(reason)));
            }
            Ok(if js::truthy(snapshot.get("unsent")) {
                Readiness::Held(Held::Unsent)
            } else {
                Readiness::Ready
            })
        })
    }

    fn deliver<'a>(
        &'a self,
        host: &'a dyn PaneHost,
        pane: &'a Pane,
        text: &'a str,
    ) -> Work<'a, Result<Admission, String>> {
        Box::pin(async move {
            // A window read at rest by an interrupt of ours has the message it
            // was given in its input box again, and this one would be pasted
            // after it, to go as one: the box is cleared first.
            if self.restored.get() {
                if let Err(cause) = pane::write_keys(host, pane, &stopped::CLEAR_INPUT).await {
                    let reason = format!("the window's input box could not be cleared: {cause}");
                    return Ok(Admission::Refused { reason });
                }
            }
            let sent = pane::write_paste(host, pane, &window_text(text)).await;
            let admitted = admission(&sent, "the window refused the paste", false);
            if matches!(admitted, Admission::Admitted { .. }) {
                // The turn the paste begins is its own: no press was for it.
                *self.pressed.borrow_mut() = None;
                self.restored.set(false);
            }
            Ok(admitted)
        })
    }

    fn observe(&self) -> Work<'_, Result<Observed, String>> {
        Box::pin(async move {
            // Claude's status first, then the conversation, which is then
            // never older than Claude's word: the order Node's reads finish
            // in when Claude's few small status files are read before a
            // transcript is. Kept from Node on purpose until the status read
            // leaves the event thread (3.5): Node started both at once, and
            // a transcript its look had read before the status came back
            // could be older than the status.
            let live = self.status();
            let session = self.session.borrow().clone();
            let reading = self
                .records
                .look(Harness::Claude, &session, &Options::default())
                .await;
            let (empty, settled, failed, quota, agents) = match &*reading {
                Reading::Known(record) => (
                    record.items.is_empty(),
                    record.settlement == Settlement::Settled,
                    record.failed,
                    record.quota.clone(),
                    record.agents_pending,
                ),
                Reading::Unknown(_) => (true, false, false, None, false),
            };
            let observed = |settled, waiting, took_back| Observed {
                reading: Some(Arc::clone(&reading)),
                settled,
                waiting,
                failed,
                quota: quota.clone(),
                switched: None,
                unnamed: false,
                took_back,
            };
            Ok(match live {
                Some(live) if self.elsewhere(&live) => {
                    switched_to(observed(false, None, false), live.session)
                }
                live => {
                    // Claude's own status is the word on whether the window
                    // is at its prompt: a new window has no transcript until
                    // its first message, and a resumed one carries a
                    // transcript that settled before this window opened, so
                    // a paste on its word alone lands before the prompt is up.
                    let idle = live.as_ref().is_some_and(|live| live.state == State::Idle);
                    // Claude says `busy` for as long as a background agent or
                    // workflow runs, though its turn is over and its prompt takes
                    // input: a turn the transcript says ended with some pending is
                    // at rest, and what is sent goes in between turns. The stop
                    // hook's summary comes a moment before the duration that says
                    // them, and Claude is busy through that moment: the look says
                    // at rest only once the duration says why. A window that said
                    // no status has said nothing, and one on its own dialog
                    // (waiting) is no agents' business.
                    let working = live
                        .as_ref()
                        .is_some_and(|live| live.state == State::Working);
                    let at_prompt = idle || (agents && working);
                    let waiting = match live {
                        Some(Status {
                            state: State::Waiting(reason),
                            ..
                        }) => Some(Waiting { reason }),
                        _ => None,
                    };
                    // A turn the daemon interrupted before a word of it was
                    // written has no end in the transcript: the press is it.
                    // Claude then has its message in the input box again, and
                    // out of the conversation it answers from: the look says so.
                    let by_press = idle && self.stopped(&reading);
                    self.restored.set(by_press);
                    *self.asked.borrow_mut() = stopped::last_user(&reading);
                    // A turn the quit cut is over, though its record never says
                    // so: Claude at its prompt has begun no other.
                    let left = self
                        .left_open
                        .borrow()
                        .as_ref()
                        .is_some_and(|open| open.still(&reading));
                    observed(
                        at_prompt && (settled || empty || left) || by_press,
                        waiting,
                        by_press,
                    )
                }
            })
        })
    }

    fn interrupted(&self) {
        let over = self.asked.borrow().clone();
        *self.pressed.borrow_mut() = over.map(|over| Pressed::new(self.time.wall_ms(), over));
    }
}
