//! Claude Code's windows (`src/adapters/claude-code.js`). Each launch gets
//! its own settings file: full permission without the one-time dialog, and
//! a Stop hook on every turn so every finished turn is recorded.
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
//! - The window's Claude process is the pane's own child when a status names
//!   it, so the first look already sees a /clear. Otherwise (Claude may run
//!   as another process the child starts) it is the process whose status
//!   first named the launch's conversation, which a /clear before that look
//!   leaves unknown. A /clear or /resume in the window changes the
//!   conversation that status names, never the process, so the window is
//!   followed to it.

use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use cf_base::env::Env;
use cf_base::js;
use cf_base::text::window_text;
use cf_proto::agents::Harness;
use serde_json::Value;

use super::install;
use super::status::{self, State, Status};
use crate::contract::{
    Adapter, Admission, Held, Launch, Observed, Pane, PaneHost, Prepared, Readiness, Records,
    Waiting, Window, Work,
};
use crate::detect::executable;
use crate::records::{Options, Reading, Settlement};
use crate::seams::{self, Entropy, Services};
use crate::shared::admission::admission;
use crate::shared::{pane, window_args};

/// A member runs in full-permission mode and reads what others wrote, so it
/// starts without the human's MCP servers, claude.ai connectors and Claude
/// in Chrome: an eval reviewer reached for the human's own browser, and this
/// Mac's setup includes a brokerage connector. The chief, which works with
/// the human, keeps them. ConsensFlow's own hooks come from `--settings`,
/// not from MCP.
const MEMBER_ISOLATION: [&str; 2] = ["--strict-mcp-config", "--no-chrome"];

/// Claude Code, as the engine launches its windows.
pub struct ClaudeAdapter {
    env: Env,
    records: Rc<dyn Records>,
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
            if launch.role != "chief" {
                argv.extend(MEMBER_ISOLATION.map(str::to_owned));
            }
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
                    statuses,
                    session: RefCell::new(session),
                    pid: Cell::new(None),
                    claude_pid: Cell::new(None),
                }),
            })
        })
    }
}

/// A Claude window, and what the engine told it of itself.
struct ClaudeWindow {
    records: Rc<dyn Records>,
    statuses: String,
    /// The conversation the window shows: its launch's, or the one it was
    /// followed to.
    session: RefCell<String>,
    /// The pane's own child, once the host named it.
    pid: Cell<Option<u32>>,
    /// The Claude whose status first named the window's conversation.
    claude_pid: Cell<Option<u32>>,
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
}

impl Window for ClaudeWindow {
    fn opened(&self, pid: Option<u32>) {
        if pid.is_some() {
            self.pid.set(pid);
        }
    }

    fn follow(&self, session: &str) {
        session.clone_into(&mut self.session.borrow_mut());
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
            let sent = pane::write_paste(host, pane, &window_text(text)).await;
            Ok(admission(&sent, "the window refused the paste", false))
        })
    }

    fn observe(&self) -> Work<'_, Result<Observed, String>> {
        Box::pin(async move {
            // Node read both at once, and Claude's few small status files
            // are read before a transcript is: the status first, then the
            // conversation, which is then never older than Claude's word.
            let live = self.status();
            let session = self.session.borrow().clone();
            let reading = self
                .records
                .look(Harness::Claude, &session, &Options::default())
                .await;
            let (empty, settled, failed, quota) = match &*reading {
                Reading::Known(record) => (
                    record.items.is_empty(),
                    record.settlement == Settlement::Settled,
                    record.failed,
                    record.quota.clone(),
                ),
                Reading::Unknown(_) => (true, false, false, None),
            };
            let observed = |settled, waiting, switched| Observed {
                reading: Some(Arc::clone(&reading)),
                settled,
                waiting,
                failed,
                quota: quota.clone(),
                switched,
                unnamed: false,
            };
            Ok(match live {
                // The window shows another conversation: the look is the
                // old one's last, and the engine follows the window there.
                Some(live) if self.elsewhere(&live) => observed(false, None, Some(live.session)),
                live => {
                    // Claude's own status is the word on whether the window
                    // is at its prompt: a new window has no transcript until
                    // its first message, and a resumed one carries a
                    // transcript that settled before this window opened, so
                    // a paste on its word alone lands before the prompt is up.
                    let idle = live.as_ref().is_some_and(|live| live.state == State::Idle);
                    let waiting = match live {
                        Some(Status {
                            state: State::Waiting(reason),
                            ..
                        }) => Some(Waiting { reason }),
                        _ => None,
                    };
                    observed(idle && (settled || empty), waiting, None)
                }
            })
        })
    }
}
