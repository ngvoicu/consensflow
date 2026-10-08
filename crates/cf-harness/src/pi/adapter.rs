//! Pi's windows. Pi takes the session name we give it (`--session-id` creates
//! it the first time and resumes it after) and the first message as its last
//! argument. ConsensFlow's extension runs inside Pi: a message is a file in its
//! inbox, which it hands to Pi only when Pi is idle, and acknowledges only once
//! Pi shows it as a user message. The same extension marks each settled turn,
//! which Pi's own log cannot, and says which conversation the window shows, so
//! a /new or /resume in it is followed.

use std::cell::RefCell;
use std::rc::Rc;

use cf_base::env::Env;
use cf_base::text::window_text;
use cf_proto::agents::Harness;

use super::channel::{hex, launch_configuration, send, Channel};
use super::install::prepare_extension;
use crate::contract::{
    Adapter, Admission, Held, Launch, Observed, Pane, PaneHost, Prepared, Readiness, Records,
    Window, Work,
};
use crate::detect::executable;
use crate::records::{Options, PiSettlement};
use crate::seams::{Entropy, Services, Time};
use crate::shared::admission::admission;
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
                    records: Rc::clone(&self.records),
                    time: Rc::clone(&self.time),
                    entropy: Rc::clone(&self.entropy),
                    channel: configuration.channel,
                    session: RefCell::new(session),
                }),
            })
        })
    }
}

/// A Pi window, and the channel to the extension inside it.
struct PiWindow {
    records: Rc<dyn Records>,
    time: Rc<dyn Time>,
    entropy: Rc<dyn Entropy>,
    channel: Channel,
    /// The conversation the window shows: its launch's, or the one it was
    /// followed to.
    session: RefCell<String>,
}

impl Window for PiWindow {
    /// Pi's own conversation is named by the extension, never by the window's
    /// process.
    fn opened(&self, _pid: Option<u32>) {}

    fn follow(&self, session: &str) {
        session.clone_into(&mut self.session.borrow_mut());
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
            let observed = record_state(self.records.look(Harness::Pi, &session, &options).await);
            Ok(match shown {
                None => unnamed(observed, HOLD),
                Some(shown) if shown == *self.session.borrow() => observed,
                Some(shown) => switched_to(observed, shown),
            })
        })
    }
}
