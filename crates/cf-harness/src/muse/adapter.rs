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
//! of 2026-09-19), and Muse's too (MUS-16, T-790): `--yolo` turns approval and the
//! sandbox off and trusts the workspace for the run, so no trust dialog is asked and
//! none is saved. A softer mode keeps Muse's sandbox, which refuses a write outside the
//! project and the temporary folders, and with its default network (`proxy-only`) the
//! member's `cf` its connection to the daemon (Muse 1.4.14, `live_muse_posture`).
//!
//! A member's window has no question tool (MUS-13, T-790): Muse sends no `PreToolUse`
//! for `request_user_input` (T-705, 1.4.13), nor for the `request_user_input_async`
//! that `--yolo` adds (1.4.14), so no hook can answer one from the board, and nobody
//! watches a member's window to answer its dialog. `--disable-request-user-input`
//! withholds both, and the member asks with `cf ask`, as its role text says and as
//! Pi's does. The chief keeps its dialog, which the human answers in its window.

use std::rc::Rc;

use cf_base::env::Env;
use cf_proto::agents::Harness;
use cf_proto::models::ModelListing;
use cf_proto::panes::{Servers, SilentPaste};
use cf_proto::specs::Variant;

use super::index;
use super::launch;
use super::paths;
use super::window::{MuseWindow, CLEAR_INPUT};
use crate::contract::{Adapter, FirstMessage, Input, Launch, Prepared, Records, Skill, Used, Work};
use crate::detect::executable;
use crate::records::Options;
use crate::seams::Services;
use crate::shared::paths::agents_skills;
use crate::shared::role::write_role;
use crate::shared::{specmint, window_args};

/// Withholds Muse's question tools for the run: a member asks with `cf ask`.
const NO_QUESTION_TOOL: &str = "--disable-request-user-input";

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

/// How Muse on Windows reads a paste (`Adapter::silent_paste`): never sooner than 0.5 ms a
/// character (it read at 0.6 to 0.75), by what it drew from the paste's first key on, then 200 ms
/// of nothing (it redraws every 50 to 65 ms while it reads, and took an Enter 157 ms after its last
/// draw), at the most a second and two milliseconds a character: in the live run it still drew
/// the long paste at about 1 ms a character (its Enter went at 8.2 s, under a limit of 8.8 s at 1
/// ms a character that only its fixed second kept ahead), and the Enter goes once it is quiet, so
/// the limit only bounds a window that never goes quiet (T-1121's finding 5).
const MUSE_ON_WINDOWS: SilentPaste = SilentPaste {
    pace_us_per_character: 500,
    drawn_after_ms: 0,
    quiet_ms: 200,
    most_ms: 1_000,
    most_us_per_character: 2_000,
};

impl Adapter for MuseAdapter {
    fn models(&self, _fetch: bool) -> Work<'_, Result<ModelListing, String>> {
        Box::pin(super::models::listed(
            self.env(),
            &*self.services.processes,
            &*self.services.time,
        ))
    }

    /// What its log says the conversation ran on last (CHF-05): its newest turn's model,
    /// read from its end, within a bound, as the reopen of a chief asks.
    fn last_used<'a>(&'a self, session: &'a str, asked: Option<&'a str>) -> Work<'a, Option<Used>> {
        Box::pin(async move { super::last_used::last_used(session, asked, self.env()) })
    }

    /// T-727's probe of Muse's own switch (`tools/probes/chief-harness/muse_model.py`): it
    /// has no key that keeps a switch to one conversation.
    fn model_switch(&self) -> Option<&'static str> {
        Some("To change only the model, type /model in the chief's window: Muse Code has no switch for one conversation, and Enter there makes it your Muse Code default.")
    }

    fn model_gone(&self, words: &str, asked: &str) -> bool {
        super::models::gone(words, asked)
    }

    fn default_model(&self) -> Work<'_, Result<Option<String>, String>> {
        Box::pin(super::models::default_model(
            self.env(),
            &*self.services.processes,
            &*self.services.time,
        ))
    }

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
            if launch.role != "chief" {
                argv.push(NO_QUESTION_TOOL.to_owned());
            }
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
    /// where one is on) needs a model's turn to see, and is not seen yet. On Windows
    /// it is not known (T-792): that shape is told by process groups, which Windows has
    /// none of, and a server and the agent's shell are both children of Muse's process
    /// in the window's job, which nothing here tells apart; so the host leaves them
    /// alone and tells the turn no command of Muse's there. Muse's record keeps no monitor of
    /// a command (Claude's does), so the engine's release of a monitor that said its command
    /// ended never waits on this for a Muse member; what `Unknown` costs is a command its turn
    /// left running, which the host does not name.
    ///
    /// `Servers::ByShell` (Claude's on Windows, T-906) was tried live on zeewin2 (T-1126,
    /// `live finish muse`, its step of a command left running, Muse 1.4.14): Muse runs each
    /// command of its agent in a `powershell.exe -NoLogo -NoProfile -Command …` of its own,
    /// the shape `ByShell` reads, and asked to leave a command running it either used its own
    /// background tool and ended the command itself (3 runs of 5; nothing for any shape to
    /// see), or detached it with `Start-Process`, whose PowerShell exits at once (2 of 5, one
    /// on each shape). A process its shell outlived is no command under `ByShell` either, so
    /// the chief was told nothing of the detached ping under both, and `ByShell` changed no
    /// verdict: Muse stays `Unknown` until a case shows a command `ByShell` would name. Linux
    /// has not been probed.
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

    /// On Windows Muse reads a paste's key presses a character at a time, redrawing its box as it
    /// goes, and takes an Enter that comes while they are still coming, or soon after the last,
    /// for a line break in the paste. A measured heuristic (T-954, Muse Code 1.4.14 on zeewin2,
    /// 2026-11-02, `live paste-enter muse -- probe_the_gap`): a line was drawn whole at 58 to 93
    /// ms, and an Enter 100 ms after the paste stayed in it while one at 250 ms went; 7,775
    /// characters (`[Pasted Content … chars]`) were read at 0.6 to 0.75 ms a character, redrawn
    /// every 50 to 65 ms and whole at 4.5 to 5.8 s, and an Enter at 1.5 and 3 s stayed in it while
    /// one at 6 s went. Unheld, an Enter in the paste's own write was lost at every length, and one
    /// a second after the long paste too (T-1084, 4 of 6). On the Mac the terminal hands Muse a
    /// paste whole, and it took its Enter as it came, 6 of 6 (T-1086).
    fn silent_paste(&self) -> Option<SilentPaste> {
        self.env().on_windows().then_some(MUSE_ON_WINDOWS)
    }

    /// Pasted, and its box cleared with Control-C ([`CLEAR_INPUT`]).
    fn input(&self) -> Input {
        Input::Pasted {
            clear: &CLEAR_INPUT,
        }
    }

    /// Muse reads skills from its own folder (`muse/skills` in its config) and from
    /// `~/.agents/skills`, as its docs say.
    fn specmint(&self, variant: Variant, folder: &str) -> Result<Skill, String> {
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
            folder,
            specmint::installed(&roots, variant),
        )
    }
}

#[cfg(test)]
mod tests;
