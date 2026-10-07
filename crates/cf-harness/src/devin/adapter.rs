//! Devin's windows (`src/adapters/devin.js`). Each launch runs on a config of
//! its own (the owner's, plus a hook that gives a session its role
//! instructions), in full-permission mode, with the first message in a prompt
//! file. Devin names its session itself; its own wire log for this launch
//! says which one the window opened, and which one it shows after a /new or
//! /resume, so the window is followed to it. A message is pasted, behind
//! whatever the input box holds, and only while Devin still shows the
//! conversation we know.
//!
//! Kept from Node on purpose: a conversation of an empty id, which no
//! ledger holds, is refused in a sentence of this module's own, where V8
//! threw its `TypeError` for the window it made none of.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use cf_base::env::Env;
use cf_base::js;
use cf_base::text::{console_text, window_text};
use cf_proto::agents::Harness;
use futures_util::future::join;
use serde_json::Value;

use super::channel;
use super::install;
use super::wire::selected_session;
use super::wire_log::WireLog;
use crate::contract::{
    Adapter, Admission, Held, Interrupt, Launch, Observed, Pane, PaneHost, Prepared, Readiness,
    Records, Window, Work,
};
use crate::detect::executable;
use crate::records::Options;
use crate::seams::{Services, Time};
use crate::shared::admission::admission;
use crate::shared::record_state::{dialog_waiting, record_state, switched_to, unnamed};
use crate::shared::role::write_role;
use crate::shared::{pane, window_args};

/// What a window that has not said yet which conversation it shows is held for.
const HOLD: &str = "Devin has not said yet which conversation its window shows";

/// How often the wire log is asked which conversation Devin opened, and how
/// long before it is given up on.
const DISCOVER_EVERY: Duration = Duration::from_millis(250);
const DISCOVER_FOR_MS: i64 = 60_000;

/// Devin's shell on Windows is Git Bash, which names folders /c/Users/…, and
/// its file tools write such a path to C:\c\Users\…: a Devin worker's file
/// landed there and its task's work was lost (2026-10-03).
const WINDOWS_PATHS: &str = "This machine runs Windows and your shell is Git Bash: give file tools Windows paths (C:\\Users\\…) or paths relative to the project folder, never /c/… paths, which they write under C:\\c\\.";

/// The role text a Devin window gets: on Windows, with how to name a file
/// there. A window given none gets none, and is refused as any is.
fn role_text(instructions: &str, env: &Env) -> String {
    if env.on_windows() && !instructions.is_empty() {
        format!("{instructions}\n\n{WINDOWS_PATHS}\n")
    } else {
        instructions.to_owned()
    }
}

/// Devin, as the engine launches its windows.
pub struct DevinAdapter {
    services: Services,
}

impl DevinAdapter {
    /// The adapter of the windows the engine's `services` serve.
    pub fn new(services: &Services) -> Self {
        Self {
            services: services.clone(),
        }
    }
}

impl Adapter for DevinAdapter {
    fn prepare<'a>(&'a self, launch: &'a Launch<'a>) -> Work<'a, Result<Prepared, String>> {
        Box::pin(async move {
            let env = &self.services.env;
            let executable = executable(Harness::Devin, env)?;
            let integration = install::integration(
                &self.services,
                launch.id.as_str(),
                &executable,
                launch.role != "chief",
            )
            .await?;
            let role = write_role(
                Harness::Devin,
                launch.role,
                env,
                launch.id,
                &role_text(launch.instructions, env),
            )?;
            let agent = launch.agent.unwrap_or_default();
            let message = launch.message.map(window_text);
            let open = if launch.resume.is_none() {
                window_args::start
            } else {
                window_args::resume
            };
            // None only for a conversation of an empty id, which no ledger holds.
            let invocation = open(Harness::Devin, agent, launch.resume, message.as_deref())
                .ok_or_else(|| "a Devin window opens on a session id".to_owned())?;
            let invocation = install::prepare_prompt(invocation, &integration)?;
            let mut argv = vec![executable];
            argv.extend(integration.args);
            argv.extend(invocation.args);
            let mut vars = integration.env;
            vars.push(("CF_DEVIN_ROLE_FILE".to_owned(), role.file));
            Ok(Prepared {
                argv,
                env: vars,
                drop_env: invocation
                    .drop_env
                    .iter()
                    .map(|&name| name.to_owned())
                    .collect(),
                native_session: launch.resume.map(str::to_owned),
                window: Rc::new(DevinWindow {
                    env: env.clone(),
                    records: Rc::clone(&self.services.records),
                    time: Rc::clone(&self.services.time),
                    session: RefCell::new(launch.resume.map(str::to_owned)),
                    wire: WireLog::new(&integration.wire, self.services.zone.clone()),
                }),
            })
        })
    }

    /// Devin's own status line says it: "esc twice to interrupt". The same
    /// two at a Devin already idle open its rewind ("Revert"), and the next
    /// Enter confirms it, cutting the conversation back: a third Escape a
    /// second later closes it, and changes nothing after a stopped turn or at
    /// an idle prompt (probed 2026-10-03).
    fn interrupt(&self) -> Interrupt {
        Interrupt {
            presses: 2,
            close_after: Some(Duration::from_secs(1)),
        }
    }
}

/// A Devin window, and what the engine told it of itself.
struct DevinWindow {
    env: Env,
    records: Rc<dyn Records>,
    time: Rc<dyn Time>,
    /// The conversation the window shows: its launch's, the one it named
    /// itself, or the one it was followed to.
    session: RefCell<Option<String>>,
    wire: WireLog,
}

impl DevinWindow {
    /// Whether `shown`, a conversation Devin says its window shows, is the
    /// one the window is known to be on.
    fn known(&self, shown: &str) -> bool {
        self.session.borrow().as_deref() == Some(shown)
    }
}

impl Window for DevinWindow {
    fn opened(&self, _pid: Option<u32>) {}

    fn follow(&self, session: &str) {
        *self.session.borrow_mut() = Some(session.to_owned());
    }

    fn started(&self) -> Work<'_, Result<Option<String>, String>> {
        Box::pin(async move {
            if self.session.borrow().is_some() {
                return Ok(None);
            }
            let deadline = self.time.wall_ms() + DISCOVER_FOR_MS;
            while self.time.wall_ms() < deadline {
                if let Ok(Some(session)) = selected_session(self.wire.path()) {
                    *self.session.borrow_mut() = Some(session.clone());
                    return Ok(Some(session));
                }
                self.time.sleep(DISCOVER_EVERY).await;
            }
            Err("Devin never said which session it opened (its wire log stayed empty)".to_owned())
        })
    }

    fn ready<'a>(
        &'a self,
        host: &'a dyn PaneHost,
        pane: &'a Pane,
    ) -> Work<'a, Result<Readiness, String>> {
        Box::pin(async move {
            let Some(shown) = self.wire.read(self.time.wall_ms())?.shown else {
                return Ok(Readiness::Held(Held::Because(HOLD.to_owned())));
            };
            if !self.known(&shown) {
                return Ok(Readiness::Held(Held::ShowsAnother));
            }
            let snapshot = pane::snapshot(host, pane)
                .await
                .map_err(|failed| failed.message)?;
            if snapshot.get("ok") != Some(&Value::Bool(true))
                || js::truthy(snapshot.get("pasteInFlight"))
            {
                return Ok(Readiness::Held(Held::Unsaid));
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
            // Windows' console drops a paste's non-ASCII marks on their way to Devin.
            let body = if self.env.on_windows() {
                console_text(&window_text(text))
            } else {
                window_text(text)
            };
            let session = self.session.borrow().clone();
            let sent = channel::send(host, pane, self.wire.path(), session.as_deref(), &body).await;
            Ok(admission(&sent, "Devin refused the paste", false))
        })
    }

    fn observe(&self) -> Work<'_, Result<Observed, String>> {
        Box::pin(async move {
            let Some(session) = self.session.borrow().clone() else {
                return Ok(Observed {
                    reading: None,
                    settled: false,
                    waiting: None,
                    failed: false,
                    quota: None,
                    switched: None,
                    unnamed: false,
                    took_back: false,
                });
            };
            // The record and the wire log are read together: the log as it is
            // when the look begins, the record when its reading is released.
            let (reading, wire) = join(
                self.records
                    .look(Harness::Devin, &session, &Options::default()),
                async { self.wire.read(self.time.wall_ms()) },
            )
            .await;
            let wire = wire?;
            let mut observed = record_state(reading);
            observed.waiting = dialog_waiting(observed.reading.as_deref());
            observed.quota = wire.quota;
            Ok(match wire.shown {
                None => unnamed(observed, HOLD),
                Some(shown) if !self.known(&shown) => switched_to(observed, shown),
                Some(_) => observed,
            })
        })
    }
}

#[cfg(test)]
mod tests;
