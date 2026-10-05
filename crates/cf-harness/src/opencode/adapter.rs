//! OpenCode's windows (`src/adapters/opencode.js`). A fresh conversation is
//! created on a throwaway `opencode serve` first, so its id is known before
//! the window opens; the TUI then runs its own server on a private port and
//! password, with ConsensFlow's plugin loaded. OpenCode ignores a prompt on
//! a `--session` launch, so the first message goes through that server once
//! it answers, and every later one through the plugin, which posts it to the
//! session the TUI is showing.
//!
//! An empty conversation says nothing about the window: until the plugin
//! reports that the TUI shows this conversation, OpenCode is still loading
//! (or the human is on its home screen or session list) and nothing is sent.
//! When the human opens another conversation in it (/new, or one from the
//! list), the window is followed there.
//!
//! The window's live status, which the plugin reports with the conversation
//! it shows, has the last word where the store cannot: a refused request
//! never reaches the store (OpenCode waits to retry it until the limit
//! resets), and an answer a lost window never finished stays unfinished
//! there for good once the conversation is reopened, though OpenCode is
//! idle.
//!
//! Kept from Node on purpose:
//! - a conversation of an empty id is refused in a sentence of this module's
//!   own, where V8 threw its `TypeError`;
//! - a look reads what the plugin says first and the window's record after
//!   it, where Node began both at once, so the record is never older than
//!   the word it is read beside;
//! - a plugin that sends the conversation's id as a list is read as the
//!   text the id's pattern reads it as, and the conversation it names is
//!   never the window's (`channel::state`).

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use cf_base::env::Env;
use cf_base::text::window_text;
use cf_proto::agents::Harness;
use serde_json::Value;

use super::channel::{
    create_session, launch_configuration, seed_session, send, session_state, Channel, Seed, Serve,
    Shown, Target, Wires,
};
use super::child_env::{child_env, Declared};
use super::install::prepare_extension;
use super::quota::retry_quota;
use super::role::configure;
use crate::contract::{
    Adapter, Admission, Held, Launch, Observed, Pane, PaneHost, Prepared, Readiness, Records,
    Window, Work,
};
use crate::detect::executable;
use crate::records::Options;
use crate::seams::{Entropy, Loopback, Ports, Processes, Services, Time};
use crate::shared::admission::admission;
use crate::shared::record_state::{dialog_waiting, record_state, switched_to, unnamed};
use crate::shared::window_args;

/// Why a window whose plugin does not answer waits.
const STARTING: &str = "the OpenCode window is starting: its plugin does not answer yet";

/// Why a window that shows no conversation waits.
const HOLD: &str =
    "the OpenCode window shows no conversation: its home screen or session list is open";

/// OpenCode, as the engine launches its windows.
pub struct OpenCodeAdapter {
    env: Env,
    records: Rc<dyn Records>,
    time: Rc<dyn Time>,
    entropy: Rc<dyn Entropy>,
    ports: Rc<dyn Ports>,
    loopback: Rc<dyn Loopback>,
    processes: Rc<dyn Processes>,
}

impl OpenCodeAdapter {
    /// The adapter of the windows the engine's `services` serve.
    pub fn new(services: &Services) -> Self {
        Self {
            env: services.env.clone(),
            records: Rc::clone(&services.records),
            time: Rc::clone(&services.time),
            entropy: Rc::clone(&services.entropy),
            ports: Rc::clone(&services.ports),
            loopback: Rc::clone(&services.loopback),
            processes: Rc::clone(&services.processes),
        }
    }

    fn wires(&self) -> Wires<'_> {
        Wires {
            time: &*self.time,
            loopback: &*self.loopback,
            processes: &*self.processes,
        }
    }
}

impl Adapter for OpenCodeAdapter {
    fn prepare<'a>(&'a self, launch: &'a Launch<'a>) -> Work<'a, Result<Prepared, String>> {
        Box::pin(async move {
            let executable = executable(Harness::Opencode, &self.env)?;
            let settings = prepare_extension(&self.env).into_config()?;
            let role = configure(&self.env, launch.id, launch.role, launch.instructions)?;
            let launched = launch_configuration(
                &self.env,
                &*self.ports,
                &*self.entropy,
                launch.id,
                launch.directory,
                &settings,
            )?;
            let session = match launch.resume {
                Some(resume) => resume.to_owned(),
                None => {
                    let with_role = Env::from_vars(
                        self.env
                            .iter()
                            .map(|(name, value)| (name.to_owned(), value.to_owned()))
                            .chain([("OPENCODE_CONFIG_CONTENT".into(), role.clone().into())]),
                    );
                    let serve = Serve {
                        executable: &executable,
                        directory: launch.directory,
                        env: &child_env(&with_role, &Declared::default()),
                        launched: &launched,
                    };
                    create_session(self.wires(), &serve).await?
                }
            };
            let agent = launch.agent.unwrap_or_default();
            let open = if launch.resume.is_none() {
                window_args::start
            } else {
                window_args::resume
            };
            // None only for a conversation of an empty id, which no ledger holds.
            let invocation = open(Harness::Opencode, agent, Some(&session), None)
                .ok_or_else(|| "an OpenCode window opens on a session id".to_owned())?;
            let mut argv = vec![executable];
            argv.extend(launched.args);
            argv.extend(invocation.args);
            let mut env = launched.env;
            env.push(("OPENCODE_CONFIG_CONTENT".to_owned(), role));
            Ok(Prepared {
                argv,
                env,
                drop_env: invocation
                    .drop_env
                    .iter()
                    .map(|&name| name.to_owned())
                    .collect(),
                native_session: Some(session.clone()),
                window: Rc::new(OpenCodeWindow {
                    records: Rc::clone(&self.records),
                    time: Rc::clone(&self.time),
                    loopback: Rc::clone(&self.loopback),
                    processes: Rc::clone(&self.processes),
                    channel: launched.channel,
                    directory: launch.directory.to_owned(),
                    first_message: launch.message.map(window_text),
                    resumed: launch.resume.is_some(),
                    model: launch
                        .agent
                        .and_then(|agent| agent.model)
                        .map(str::to_owned),
                    effort: launch
                        .agent
                        .and_then(|agent| agent.effort)
                        .map(str::to_owned),
                    session: RefCell::new(session),
                }),
            })
        })
    }
}

/// An OpenCode window, and the channel to its server and its plugin.
struct OpenCodeWindow {
    records: Rc<dyn Records>,
    time: Rc<dyn Time>,
    loopback: Rc<dyn Loopback>,
    processes: Rc<dyn Processes>,
    channel: Channel,
    /// The folder the window works in.
    directory: String,
    /// The first message as a window takes text, none for a window opened
    /// without one.
    first_message: Option<String>,
    resumed: bool,
    /// The model and effort of the agent it runs on, as the launch gave them.
    model: Option<String>,
    effort: Option<String>,
    /// The conversation the window shows: its launch's, or the one it was
    /// followed to.
    session: RefCell<String>,
}

impl OpenCodeWindow {
    fn wires(&self) -> Wires<'_> {
        Wires {
            time: &*self.time,
            loopback: &*self.loopback,
            processes: &*self.processes,
        }
    }

    /// What the plugin says the window shows.
    async fn shown(&self) -> Option<Shown> {
        session_state(self.wires(), &self.channel).await
    }

    /// Whether the conversation the plugin says the window shows is this
    /// window's: its id, as text.
    fn shows_ours(&self, shown: &Shown) -> bool {
        !shown.listed && shown.session.as_deref() == Some(&*self.session.borrow())
    }
}

impl Window for OpenCodeWindow {
    /// OpenCode's own conversation is named by the plugin, never by the
    /// window's process.
    fn opened(&self, _pid: Option<u32>) {}

    fn follow(&self, session: &str) {
        session.clone_into(&mut self.session.borrow_mut());
    }

    /// The first message goes in once the server is up, through the server.
    fn started(&self) -> Work<'_, Result<Option<String>, String>> {
        Box::pin(async move {
            let Some(text) = &self.first_message else {
                return Ok(None);
            };
            let session = self.session.borrow().clone();
            let fresh = !self.resumed;
            let seed = Seed {
                session: &session,
                directory: &self.directory,
                text,
                model: self.model.as_deref().filter(|_| fresh),
                variant: self.effort.as_deref().filter(|_| fresh),
                resume: self.resumed,
            };
            seed_session(self.wires(), &self.channel, &seed).await?;
            Ok(None)
        })
    }

    /// A message waits until the plugin has said which conversation the
    /// window shows, and it is the window's own.
    fn ready<'a>(
        &'a self,
        _host: &'a dyn PaneHost,
        _pane: &'a Pane,
    ) -> Work<'a, Result<Readiness, String>> {
        Box::pin(async move {
            Ok(match self.shown().await {
                None => Readiness::Held(Held::Because(STARTING.to_owned())),
                Some(Shown { session: None, .. }) => {
                    Readiness::Held(Held::Because(HOLD.to_owned()))
                }
                Some(shown) if self.shows_ours(&shown) => Readiness::Ready,
                Some(_) => Readiness::Held(Held::ShowsAnother),
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
            let session = self.session.borrow().clone();
            let target = Target {
                session: &session,
                pane,
                host,
            };
            let sent = send(self.wires(), &self.channel, &target, &window_text(text)).await?;
            Ok(admission(&sent, "OpenCode refused it", true))
        })
    }

    fn observe(&self) -> Work<'_, Result<Observed, String>> {
        Box::pin(async move {
            let session = self.session.borrow().clone();
            // The window first, then its record. Kept from Node on purpose:
            // Node began both at once, and the record it read could be older
            // than the word it heard.
            let window = self.shown().await;
            let reading = self
                .records
                .look(Harness::Opencode, &session, &Options::default())
                .await;
            let mut observed = record_state(reading);
            let showing = window.as_ref().is_some_and(|shown| self.shows_ours(shown));
            let status = window
                .as_ref()
                .filter(|_| showing)
                .and_then(|shown| shown.status.get("type"))
                .and_then(Value::as_str);
            let idle = status == Some("idle");
            // A session waiting to retry a request is at work, whatever its
            // record says.
            let retrying = status == Some("retry");
            let retry = match window.as_ref().filter(|_| showing) {
                Some(shown) => retry_quota(&shown.status, self.time.wall_ms())?,
                None => None,
            };
            observed.quota = retry.map(Arc::new).or(observed.quota);
            observed.settled = showing && !retrying && (observed.settled || idle);
            observed.waiting = dialog_waiting(observed.reading.as_deref());
            // Until its plugin answers, the window names no conversation: a
            // message waits, as ready() holds it.
            Ok(match window {
                None => unnamed(observed, STARTING),
                Some(Shown { session: None, .. }) => unnamed(observed, HOLD),
                Some(Shown {
                    session: Some(other),
                    ..
                }) => {
                    if showing {
                        observed
                    } else {
                        switched_to(observed, other)
                    }
                }
            })
        })
    }
}
