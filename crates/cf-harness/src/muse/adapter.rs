//! Muse Code's windows. Each launch runs the binary Muse's launcher names
//! ([`launch::pinned`]) in full permission (`--yolo`), on its agent's model and
//! effort, contributor mode off, the owner's hooks with ConsensFlow's SessionStart
//! hook added for the one run (`-c hooks=`), which gives each session the window starts
//! its role text and writes it down for the window to follow. No first message is on
//! the command line, fresh or resumed: it is pasted once the window's prompt shows, as
//! any later message is ([`FirstMessage::Delivered`]). The window is [`MuseWindow`];
//! its record is the session's log (`record`).
//!
//! Full permission is the posture every harness's window runs in (the owner's decision
//! of 2026-09-19) until MUS-16 says otherwise: `--yolo` turns approval and the sandbox
//! off and trusts the workspace for the run, so no trust dialog is asked and none is
//! saved. It keeps Muse's question tool, which MUS-13's door answers.

use std::rc::Rc;

use cf_base::env::Env;
use cf_proto::agents::Harness;
use cf_proto::panes::Servers;
use cf_proto::specs::Variant;

use super::index;
use super::launch;
use super::paths;
use super::window::{MuseWindow, CLEAR_INPUT};
use crate::contract::{Adapter, FirstMessage, Input, Launch, Prepared, Records, Skill, Work};
use crate::detect::executable;
use crate::records::Options;
use crate::seams::Services;
use crate::shared::paths::agents_skills;
use crate::shared::role::write_role;
use crate::shared::{specmint, window_args};

/// Muse Code, as the engine launches its windows.
pub struct MuseAdapter {
    services: Services,
}

impl MuseAdapter {
    /// The adapter of the windows the engine's `services` serve.
    pub fn new(services: &Services) -> Self {
        Self {
            services: services.clone(),
        }
    }

    fn env(&self) -> &Env {
        &self.services.env
    }

    fn records(&self) -> &Rc<dyn Records> {
        &self.services.records
    }
}

impl Adapter for MuseAdapter {
    fn prepare<'a>(&'a self, launch: &'a Launch<'a>) -> Work<'a, Result<Prepared, String>> {
        Box::pin(async move {
            let env = self.env();
            let executable = executable(Harness::Muse, env)?;
            let role = write_role(
                Harness::Muse,
                launch.role,
                env,
                launch.id,
                launch.instructions,
            )?;
            // A chief given specs reads its Specmint skill by the path its role text
            // names: where Muse has none of its own, ConsensFlow's is written where it
            // is kept for every launch.
            specmint::write_own(Harness::Muse, env, launch)?;
            let integration = launch::integration(
                env,
                launch.id,
                &executable,
                &self.services.bundle.pane_cf,
                &role.file,
            )?;
            let agent = launch.agent.unwrap_or_default();
            let open = if launch.resume.is_none() {
                window_args::start
            } else {
                window_args::resume
            };
            let invocation = open(Harness::Muse, agent, launch.resume, None)
                .ok_or_else(|| "a Muse window opens on a session id".to_owned())?;
            let mut argv = vec![integration.program];
            argv.extend(integration.args);
            argv.extend(invocation.args);
            Ok(Prepared {
                argv,
                env: integration.env,
                drop_env: invocation
                    .drop_env
                    .iter()
                    .map(|&name| name.to_owned())
                    .collect(),
                native_session: launch.resume.map(str::to_owned),
                window: Rc::new(MuseWindow::new(
                    &self.services,
                    launch.resume,
                    integration.said,
                    launch.first_message_ms,
                )),
            })
        })
    }

    /// The session's log says (`index::resumable`), and Muse's index only where the
    /// log cannot be read.
    fn resumable<'a>(&'a self, session: &'a str) -> Work<'a, Option<bool>> {
        Box::pin(async move {
            let looked = self
                .records()
                .look(Harness::Muse, session, &Options::default())
                .await;
            index::resumable(&looked, index::row(session, self.env()))
        })
    }

    /// Muse starts its MCP servers with the window, before its trust question and any
    /// turn, each a child of its own process in a group of its own, and the human's
    /// `!` command likewise (the research of T-700, Muse 1.4.13 on macOS): Codex's
    /// shape. Where the agent's own tool calls run (its managed shell, in a sandbox
    /// where one is on) needs a model's turn to see, and is not seen yet. Linux and
    /// Windows have not been probed.
    fn servers(&self) -> Servers {
        if cfg!(target_os = "macos") && !self.env().on_windows() {
            Servers::AtLaunch
        } else {
            Servers::Unknown
        }
    }

    /// Pasted once the window's prompt shows, fresh or resumed: nothing of it is on
    /// the command line, which every process on the machine reads.
    fn first_message(&self) -> FirstMessage {
        FirstMessage::Delivered
    }

    /// Pasted, and its box cleared with Control-C ([`CLEAR_INPUT`]).
    fn input(&self) -> Input {
        Input::Pasted {
            clear: &CLEAR_INPUT,
        }
    }

    /// Muse reads skills from its own folder (`muse/skills` in its config) and from
    /// `~/.agents/skills`, as its docs say.
    fn specmint(&self, variant: Variant) -> Result<Skill, String> {
        let env = self.env();
        let mut roots = Vec::new();
        roots.extend(
            paths::settings(env)
                .ok()
                .and_then(|file| file.parent().map(|folder| folder.join("skills")))
                .map(|folder| folder.to_string_lossy().into_owned()),
        );
        roots.extend(agents_skills(env).ok());
        specmint::place(
            Harness::Muse,
            env,
            variant,
            specmint::installed(&roots, variant),
        )
    }
}

#[cfg(test)]
mod tests;
