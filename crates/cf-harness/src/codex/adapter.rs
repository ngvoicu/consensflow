//! Codex's windows (`src/adapters/codex.js`). Codex runs under ConsensFlow's
//! supervisor, the bundle's native `cf codex-session`: an app-server, a broker
//! that knows the thread the TUI shows and queues messages on it, and the TUI
//! attached to both. The first message is Codex's last argument; the broker
//! names the thread once Codex starts it, and again whenever the human starts
//! or resumes another one in the window (/new, /resume), so the window is
//! followed to it. A Codex too old for the native queue is refused: nothing
//! could reach its window.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use cf_base::text::window_text;
use cf_proto::agents::Harness;

use super::channel::{send, Channel, Session, Shown, Target};
use super::{launch, mcp, role};
use crate::contract::{
    Adapter, Admission, Agent, Held, Launch, Observed, Pane, PaneHost, Prepared, Readiness,
    Records, Window, Work,
};
use crate::detect::executable;
use crate::records::Options;
use crate::seams::{Loopback, Services, Time};
use crate::shared::admission::admission;
use crate::shared::record_state::{record_state, switched_to, unnamed};
use crate::shared::window_args;

/// Codex's question tool (`request_user_input`) is behind a feature still
/// marked under development; the broker answers a member's from the board.
/// The chief has none: it asks the human in plain words in its window.
const QUESTION_TOOL: [&str; 4] = [
    "--enable",
    "default_mode_request_user_input",
    "-c",
    "suppress_unstable_features_warning=true",
];

/// A window that opens on Codex's "Update now?" prompt, whenever a newer
/// release exists, never starts its thread, so a chief opened without a first
/// message waited on it for good. And a login shell re-reads the user's
/// profile, which can put another install's `cf` ahead of this daemon's (an
/// eval worker's `cf` was the live app's older one): without it, commands keep
/// the PATH the daemon gave the window. Both probed on Codex 0.156.1.
const WINDOW: [&str; 4] = [
    "-c",
    "check_for_update_on_startup=false",
    "-c",
    "allow_login_shell=false",
];

/// Why a message waits while the broker names no thread or cannot take one: a
/// refusal there would spend the message's attempts in seconds (an answer to a
/// Codex worker was lost that way), so it is held.
const HOLD: &str =
    "the Codex window cannot take a message yet: starting, switching conversations or reconnecting";

/// How often the broker is asked for the thread a new window opened.
const DISCOVER_EVERY: Duration = Duration::from_millis(250);

/// How long the broker has to name it, in milliseconds.
const DISCOVER_FOR_MS: i64 = 60_000;

/// Codex, as the engine launches its windows.
pub struct CodexAdapter {
    services: Services,
}

impl CodexAdapter {
    /// The adapter of the windows the engine's `services` serve.
    pub fn new(services: &Services) -> Self {
        Self {
            services: services.clone(),
        }
    }
}

impl Adapter for CodexAdapter {
    fn prepare<'a>(&'a self, launch: &'a Launch<'a>) -> Work<'a, Result<Prepared, String>> {
        Box::pin(async move {
            let services = &self.services;
            let executable = executable(Harness::Codex, &services.env)?;
            let launched =
                launch::configuration(services, launch.id, launch.directory, &executable).await?;
            let role = role::arguments(
                services,
                launch.role,
                &executable,
                launch.directory,
                launch.instructions,
            )
            .await?;
            // The chief works with the human and keeps the human's connectors.
            let chief = launch.role == "chief";
            let isolation = if chief {
                Vec::new()
            } else {
                let servers = mcp::listed(&*services.processes, &services.env, &executable).await?;
                mcp::isolation(&servers)?
            };
            // An image agent's window is Codex on its own default model, whose
            // image tool draws: it names no model or effort of its own.
            let agent = match launch.agent {
                Some(agent) if !agent.designer => agent,
                _ => Agent::default(),
            };
            let message = launch.message.map(window_text);
            let open = if launch.resume.is_none() {
                window_args::start
            } else {
                window_args::resume
            };
            // None only for a conversation of an empty id, which no ledger holds.
            let invocation = open(Harness::Codex, agent, launch.resume, message.as_deref())
                .ok_or_else(|| "a Codex window resumes a thread by its id".to_owned())?;
            let mut args = role;
            if !chief {
                args.extend(QUESTION_TOOL.map(str::to_owned));
            }
            args.extend(WINDOW.map(str::to_owned));
            args.extend(isolation);
            args.extend(invocation.args);
            Ok(Prepared {
                argv: launch::with_native_bridge(&services.bundle, &executable, args),
                env: launched.env,
                drop_env: invocation
                    .drop_env
                    .iter()
                    .map(|&name| name.to_owned())
                    .collect(),
                native_session: launch.resume.map(str::to_owned),
                window: Rc::new(CodexWindow {
                    records: Rc::clone(&services.records),
                    time: Rc::clone(&services.time),
                    loopback: Rc::clone(&services.loopback),
                    channel: launched.channel,
                    thread: RefCell::new(launch.resume.map(str::to_owned)),
                }),
            })
        })
    }
}

/// A Codex window, and the channel to the broker of its supervisor.
struct CodexWindow {
    records: Rc<dyn Records>,
    time: Rc<dyn Time>,
    loopback: Rc<dyn Loopback>,
    channel: Channel,
    /// The thread the window shows: its launch's, the one the broker named
    /// when Codex started it, or the one it was followed to. None until the
    /// broker has named it.
    thread: RefCell<Option<String>>,
}

impl CodexWindow {
    /// The broker's word on the window now.
    async fn shown(&self) -> Option<Shown> {
        self.channel.shown(&*self.time, &*self.loopback).await
    }
}

impl Window for CodexWindow {
    /// The thread is named by the broker, never by the window's process.
    fn opened(&self, _pid: Option<u32>) {}

    fn follow(&self, session: &str) {
        *self.thread.borrow_mut() = Some(session.to_owned());
    }

    fn started(&self) -> Work<'_, Result<Option<String>, String>> {
        Box::pin(async move {
            let named = self.thread.borrow().is_some();
            if named {
                return Ok(None);
            }
            let deadline = self.time.wall_ms().saturating_add(DISCOVER_FOR_MS);
            while self.time.wall_ms() < deadline {
                if let Some(Shown {
                    session: Session::Thread(thread),
                    ..
                }) = self.shown().await
                {
                    *self.thread.borrow_mut() = Some(thread.clone());
                    return Ok(Some(thread));
                }
                self.time.sleep(DISCOVER_EVERY).await;
            }
            Err("the Codex broker never named the thread it opened".to_owned())
        })
    }

    /// A message waits until the broker can take one and the window shows its
    /// own thread.
    fn ready<'a>(
        &'a self,
        _host: &'a dyn PaneHost,
        _pane: &'a Pane,
    ) -> Work<'a, Result<Readiness, String>> {
        Box::pin(async move {
            Ok(match self.shown().await {
                Some(shown) if shown.available => {
                    if shown.session.is(self.thread.borrow().as_deref()) {
                        Readiness::Ready
                    } else {
                        Readiness::Held(Held::ShowsAnother)
                    }
                }
                _ => Readiness::Held(Held::Because(HOLD.to_owned())),
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
            let thread = self.thread.borrow().clone();
            let target = Target {
                channel: &self.channel,
                thread: thread.as_deref(),
                pane,
                host,
            };
            let sent = send(&*self.time, &*self.loopback, &target, &window_text(text)).await?;
            Ok(admission(
                &sent.reading(),
                "the Codex broker refused it",
                true,
            ))
        })
    }

    fn observe(&self) -> Work<'_, Result<Observed, String>> {
        Box::pin(async move {
            let thread = self.thread.borrow().clone();
            let Some(thread) = thread else {
                return Ok(Observed {
                    reading: None,
                    settled: false,
                    waiting: None,
                    failed: false,
                    quota: None,
                    switched: None,
                    unnamed: false,
                });
            };
            // The broker's word first, then the record, which is then never
            // older than the word: the order Node's reads finish in when the
            // broker answers beside a record. Kept from Node on purpose: Node
            // started both at once, and a record its look had read before the
            // broker answered could be older.
            let shown = self.shown().await;
            let reading = self
                .records
                .look(Harness::Codex, &thread, &Options::default())
                .await;
            let observed = record_state(reading);
            Ok(match shown {
                Some(Shown {
                    session: Session::Thread(shown),
                    ..
                }) => {
                    // The thread the window was followed to meanwhile, if it was.
                    if self.thread.borrow().as_deref() == Some(shown.as_str()) {
                        observed
                    } else {
                        switched_to(observed, shown)
                    }
                }
                _ => unnamed(observed, HOLD),
            })
        })
    }
}
