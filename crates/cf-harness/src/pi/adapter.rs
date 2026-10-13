//! Pi's windows. Pi takes the session name we give it (`--session-id` creates
//! it the first time and resumes it after) and the first message as its last
//! argument. ConsensFlow's extension runs inside Pi: a message is a file in its
//! inbox, which it hands to Pi only when Pi is idle, and acknowledges only once
//! Pi shows it as a user message. The same extension marks each settled turn,
//! which Pi's own log cannot, and says which conversation the window shows, so
//! a /new or /resume in it is followed.
//!
//! A conversation the app's quit cut in a turn is found with that turn open
//! (a user's message with no answer, or a tool's call with no result), and no
//! process in it: resumed, Pi waits at its prompt and writes nothing of it
//! (`npm run live:resume-cut -- --harness pi`). A window opened on such a
//! conversation with no message to begin it is read at rest once its extension
//! names the conversation and has seen no turn of the window at work, while its
//! record holds no more than it held when the window was opened ([`LeftOpen`]).

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use cf_base::env::Env;
use cf_base::text::window_text;
use cf_proto::agents::Harness;
use cf_proto::panes::Servers;

use super::channel::{hex, launch_configuration, send, Channel};
use super::install::prepare_extension;
use super::record::evidence::working;
use crate::contract::{
    Adapter, Admission, Held, Launch, Observed, Pane, PaneHost, Prepared, Readiness, Records,
    Window, Work,
};
use crate::detect::executable;
use crate::records::{Options, PiSettlement};
use crate::seams::{Entropy, Services, Time};
use crate::shared::admission::admission;
use crate::shared::left_open::LeftOpen;
use crate::shared::record_state::{record_state, switched_to, unnamed};
use crate::shared::role::write_role;
use crate::shared::window_args;

/// Why a window that has not named its conversation waits.
const HOLD: &str = "Pi has not said yet which conversation its window shows";

/// Pi, as the engine launches its windows.
pub struct PiAdapter {
    env: Env,
    records: Rc<dyn Records>,
    time: Rc<dyn Time>,
    entropy: Rc<dyn Entropy>,
}

impl PiAdapter {
    /// The adapter of the windows the engine's `services` serve.
    pub fn new(services: &Services) -> Self {
        Self {
            env: services.env.clone(),
            records: Rc::clone(&services.records),
            time: Rc::clone(&services.time),
            entropy: Rc::clone(&services.entropy),
        }
    }
}

impl Adapter for PiAdapter {
    fn prepare<'a>(&'a self, launch: &'a Launch<'a>) -> Work<'a, Result<Prepared, String>> {
        Box::pin(async move {
            let executable = executable(Harness::Pi, &self.env)?;
            let extension = prepare_extension(&self.env).into_path()?;
            let configuration =
                launch_configuration(&self.env, launch.id, launch.directory, &extension)?;
            let role = write_role(
                Harness::Pi,
                launch.role,
                &self.env,
                launch.id,
                launch.instructions,
            )?;
            let session = match launch.resume {
                Some(resume) => resume.to_owned(),
                None => {
                    let mut drawn = [0; 4];
                    self.entropy.fill(&mut drawn)?;
                    format!("cf-{}-{}-{}", launch.project, launch.handle, hex(&drawn))
                }
            };
            let message = launch.message.map(window_text);
            let open = if launch.resume.is_none() {
                window_args::start
            } else {
                window_args::resume
            };
            let agent = launch.agent.unwrap_or_default();
            // None only for a conversation of an empty id, which no ledger holds.
            let invocation = open(Harness::Pi, agent, Some(&session), message.as_deref())
                .ok_or_else(|| "a Pi window opens on a session id".to_owned())?;
            // What the record holds before this window exists is the old
            // process's: a turn it left open, which a window with no message to
            // begin one is told apart from by what the record holds later. A
            // window given a message begins its own turn.
            let left_open = if launch.resume.is_some() && launch.message.is_none() {
                let options = Options {
                    pi_settlement: Some(PiSettlement {
                        directory: Some(configuration.channel.settled.clone()),
                        launch_id: Some(configuration.channel.launch_id.clone()),
                    }),
                };
                let before = self.records.look(Harness::Pi, &session, &options).await;
                LeftOpen::of(&before)
            } else {
                None
            };
            let mut argv = vec![executable];
            argv.extend(configuration.args);
            argv.extend(["--skill".to_owned(), role.file]);
            argv.extend([
                "--append-system-prompt".to_owned(),
                launch.instructions.to_owned(),
            ]);
            argv.extend(invocation.args);
            Ok(Prepared {
                argv,
                env: configuration.env,
                drop_env: invocation
                    .drop_env
                    .iter()
                    .map(|&name| name.to_owned())
                    .collect(),
                native_session: Some(session.clone()),
                window: Rc::new(PiWindow {
                    env: self.env.clone(),
                    records: Rc::clone(&self.records),
                    time: Rc::clone(&self.time),
                    entropy: Rc::clone(&self.entropy),
                    channel: configuration.channel,
                    session: RefCell::new(session),
                    left_open: RefCell::new(left_open),
                }),
            })
        })
    }

    /// Pi's `--session-id` makes the session where there is none and resumes it
    /// where there is, so a window opened on any session of its does not end,
    /// whatever was said in it. Its file of a session is not asked after: Pi's
    /// `sessionDir` setting and the working directory of the launch choose the
    /// folder it is in, which a look from here does not know, and a file not found
    /// there would say Pi holds nothing where it holds the session.
    fn resumable<'a>(&'a self, _session: &'a str) -> Work<'a, Option<bool>> {
        Box::pin(async { Some(true) })
    }

    /// Pi's bash tool starts each command `detached` wherever there are process
    /// groups to detach in (`process.platform !== "win32"`, in
    /// `@earendil-works/pi-coding-agent/dist/core/tools/bash.js`): a session of
    /// its own, which the group of its window does not hold.
    fn servers(&self) -> Servers {
        if self.env.on_windows() {
            Servers::Unknown
        } else {
            Servers::InGroup
        }
    }
}

/// A Pi window, and the channel to the extension inside it.
struct PiWindow {
    env: Env,
    records: Rc<dyn Records>,
    time: Rc<dyn Time>,
    entropy: Rc<dyn Entropy>,
    channel: Channel,
    /// The conversation the window shows: its launch's, or the one it was
    /// followed to.
    session: RefCell<String>,
    /// The turn the record had open when the window was opened, while the
    /// window has begun no turn of its own: the old process's.
    left_open: RefCell<Option<LeftOpen>>,
}

impl Window for PiWindow {
    /// Pi's own conversation is named by the extension, never by the window's
    /// process.
    fn opened(&self, _pid: Option<u32>) {}

    fn follow(&self, session: &str) {
        session.clone_into(&mut self.session.borrow_mut());
        // The turn the record had open was a conversation's the window left.
        *self.left_open.borrow_mut() = None;
    }

    fn started(&self) -> Work<'_, Result<Option<String>, String>> {
        Box::pin(async { Ok(None) })
    }

    /// A message waits until the extension has said which conversation the
    /// window shows, and it is the window's own.
    fn ready<'a>(
        &'a self,
        _host: &'a dyn PaneHost,
        _pane: &'a Pane,
    ) -> Work<'a, Result<Readiness, String>> {
        Box::pin(async move {
            Ok(match self.channel.shown_session()? {
                None => Readiness::Held(Held::Because(HOLD.to_owned())),
                Some(shown) if shown == *self.session.borrow() => Readiness::Ready,
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
            let target = self.channel.target(&session, pane, host);
            let sent = send(&*self.time, &*self.entropy, &target, &window_text(text)).await?;
            Ok(admission(&sent.reading(), "Pi refused it", true))
        })
    }

    fn observe(&self) -> Work<'_, Result<Observed, String>> {
        Box::pin(async move {
            let session = self.session.borrow().clone();
            let options = Options {
                pi_settlement: Some(PiSettlement {
                    directory: Some(self.channel.settled.clone()),
                    launch_id: Some(self.channel.launch_id.clone()),
                }),
            };
            // The conversation the window shows first, then its record, which
            // is then never older than the extension's word: the order Node's
            // reads finish in when a small file is read beside a record. Kept
            // from Node on purpose: Node started both at once, and a record
            // its look had read before the file came back could be older.
            let shown = self.channel.shown_session()?;
            let reading = self.records.look(Harness::Pi, &session, &options).await;
            let mut observed = record_state(Arc::clone(&reading));
            // A turn the quit cut is over, though no record says so: the
            // extension, which names the conversation, has seen no turn of this
            // window begin, and the window has begun none of its own.
            if !observed.settled
                && shown.as_deref() == Some(session.as_str())
                && self
                    .left_open
                    .borrow()
                    .as_ref()
                    .is_some_and(|open| open.still(&reading))
                && !working(&session, &self.env, &options)?
            {
                observed.settled = true;
            }
            Ok(match shown {
                None => unnamed(observed, HOLD),
                Some(shown) if shown == *self.session.borrow() => observed,
                Some(shown) => switched_to(observed, shown),
            })
        })
    }
}
