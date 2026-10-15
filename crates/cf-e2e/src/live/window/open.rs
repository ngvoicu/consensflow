//! How a window is opened: the folder it runs in, the agent it runs, the command
//! line its harness opens a new conversation with (the harness code of the
//! product says it, through [`super::super::ask`]), what a Claude window has
//! besides, and the question Claude asks of a folder it has not seen, answered
//! first ([`super::trust_claude`]).

use std::cell::{Cell, RefCell};
use std::path::Path;

use serde_json::{json, Value};

use super::super::agents;
use super::super::ask;
use super::super::machine::{live_folder_in, Harness, Installed, ISOLATION};
use super::super::{list, Log};
use super::{trust_claude, Window};
use crate::rig::Rig;
use crate::{cf, Error, Result};

/// The launch the live windows' settings are written for: one id, so each is
/// written over the last.
const LAUNCH: &str = "11111111-1111-4111-8111-111111111111";

/// The harnesses the app pastes into: Codex, Pi and OpenCode take their
/// messages through their own queues.
const PASTED: [Harness; 2] = [Harness::Claude, Harness::Devin];

/// The harnesses `names` name, which the app pastes into, or why one cannot be
/// run: a word that names none, or one that takes its messages through its own
/// queue.
pub(super) fn pasted_into(names: &[String]) -> Result<Vec<Harness>> {
    names
        .iter()
        .map(|name| {
            let harness = Harness::from_name(name).ok_or_else(|| {
                Error::Daemon(format!(
                    "no harness is called {name}: {}",
                    Harness::ALL.map(Harness::name).join(", ")
                ))
            })?;
            if PASTED.contains(&harness) {
                Ok(harness)
            } else {
                Err(Error::Daemon(format!(
                    "the app pastes into claude and devin only; {name} takes its messages through its own queue"
                )))
            }
        })
        .collect()
}

/// The harnesses a run that sends the app's pastes is asked for: those it names
/// (`--harness`), and Devin where it names none (the harness that showed the
/// failure these runs look for: a paste left waiting in its input, on Windows).
pub(super) fn asked_for(named: Vec<String>) -> Vec<String> {
    if named.is_empty() {
        vec![Harness::Devin.name().to_owned()]
    } else {
        named
    }
}

/// The harnesses a run that sends the app's pastes is asked for (`--harness`, and
/// Devin where it names none), which the app pastes into (Claude Code and Devin
/// alone: the others take their messages through their own queues), found on the
/// machine and said in the `log` ([`Log::found`]): one that is none of them, or
/// cannot be run, is the error, before anything is spent.
pub fn pasted_installs(log: &Log) -> Result<Vec<Installed>> {
    pasted_into(&asked_for(list("harness")))?
        .into_iter()
        .map(|harness| log.found(harness))
        .collect()
}

/// What a run says of what came of asking Claude to trust the folder of the
/// window of `who`: `claude: trust: trusted`.
pub(super) fn trust_line(who: &str, trust: &str) -> String {
    format!("{who}: trust: {trust}")
}

/// A conversation id: a random one in the shape of a version 4 uuid.
pub(super) fn new_session() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// The conversation id a window of the harness kind `kind` is opened on: a new
/// one for Claude Code and Pi, which open on an id they are given, and none for
/// the others, which name their own. A run that opens a window of its own, as the
/// evals' bare arm does, draws its id here.
pub fn session_for(kind: &str) -> Option<String> {
    (kind == "claude-code" || kind == "pi").then(new_session)
}

/// A member's window as the daemon launches it: the adapter prepares it, with the
/// owner's config and the hooks ConsensFlow adds to it, the role's text for a
/// session of it to start with, and its first message in a prompt file
/// ([`ask::devin_launch`]). Devin's alone.
#[derive(Debug, Clone)]
pub struct Member {
    role: String,
    instructions: String,
    message: Option<String>,
}

impl Member {
    /// A member of the role `role` (`worker`) whose sessions start with
    /// `instructions`, opened with no message.
    pub fn new(role: &str, instructions: &str) -> Self {
        Self {
            role: role.to_owned(),
            instructions: instructions.to_owned(),
            message: None,
        }
    }

    /// With `message` the window's first, which its launch puts in a prompt file:
    /// what the daemon does with the task of a new window, and with the note that a
    /// window the quit cut is told so by when it is opened again.
    #[must_use]
    pub fn first_message(mut self, message: &str) -> Self {
        self.message = Some(message.to_owned());
        self
    }

    /// The window's first message, where it has one.
    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }
}

/// How a window is to be opened.
pub struct Open<'a> {
    rig: &'a Rig,
    install: &'a Installed,
    folder: String,
    id: String,
    agent: Option<Value>,
    env: Vec<(String, String)>,
    drop_env: Vec<String>,
    trusting: bool,
    resume: Option<String>,
    member: Option<Member>,
}

impl<'a> Open<'a> {
    /// The window `id` of the harness `install`, in the folder called `folder` of
    /// the home the rig gives its windows (`.consensflow-candidate/live/<folder>`
    /// below it), on the harness's cheap model.
    pub fn new(rig: &'a Rig, install: &'a Installed, folder: &str, id: &str) -> Self {
        Self {
            rig,
            install,
            folder: folder.to_owned(),
            id: id.to_owned(),
            agent: None,
            env: Vec::new(),
            drop_env: Vec::new(),
            trusting: true,
            resume: None,
            member: None,
        }
    }

    /// The window is the one the app opens on the conversation `session`, which
    /// the harness already keeps, in the same folder: a window a quit cut, opened
    /// again as the daemon that starts after the quit opens it.
    #[must_use]
    pub fn resume(mut self, session: &str) -> Self {
        self.resume = Some(session.to_owned());
        self
    }

    /// The window is a member's as the daemon launches it ([`Member`]): its command
    /// line, its config and its variables are what the adapter prepares for it,
    /// whole, and not the flags of a bare window. Each window is a launch of its own.
    #[must_use]
    pub fn member(mut self, member: Member) -> Self {
        self.member = Some(member);
        self
    }

    /// The window runs `agent` (`kind`, `model`, `effort`: [`agents::agent`] is
    /// the harness's cheap one) instead of the harness's cheap model.
    #[must_use]
    pub fn agent(mut self, agent: Value) -> Self {
        self.agent = Some(agent);
        self
    }

    /// With the variable `name` set to `value` in the window, over what its
    /// harness starts with.
    #[must_use]
    pub fn var(mut self, name: &str, value: &str) -> Self {
        self.env.push((name.to_owned(), value.to_owned()));
        self
    }

    /// Without the variable `name`, which its harness would start it with.
    #[must_use]
    pub fn without(mut self, name: &str) -> Self {
        self.drop_env.push(name.to_owned());
        self
    }

    /// Without asking Claude to trust the folder first: for a program that is no
    /// Claude Code (the stand-in agent, in the tests of this support), which asks
    /// nothing of a folder and has no dialog to wait for.
    #[must_use]
    pub fn without_trust(mut self) -> Self {
        self.trusting = false;
        self
    }

    /// [`Open::open`], and what came of asking Claude to trust the folder said in
    /// the `log` where it was asked, as the run of `who` (`claude: trust: trusted`).
    pub fn open_telling(self, log: &Log, who: &str) -> Result<Window<'a>> {
        let window = self.open()?;
        if let Some(trust) = window.trust() {
            log.say(trust_line(who, trust));
        }
        Ok(window)
    }

    /// What the window opens on: the harness's own flags for a bare window, with the
    /// program the machine has it at; a member's launch as the adapter prepares it,
    /// with the program the adapter found. A launch is made once, so each is under
    /// an id of its own.
    fn command(
        &self,
        agent: &Value,
        session: Option<&str>,
        workspace: &Path,
        env: &[(String, String)],
    ) -> Result<(ask::Start, String)> {
        let harness = self.install.harness();
        if let Some(member) = &self.member {
            if harness != Harness::Devin {
                return Err(Error::Daemon(format!(
                    "a member's launch is prepared for Devin alone, not {}",
                    harness.name()
                )));
            }
            let start = ask::devin_launch(&ask::DevinLaunch {
                id: &new_session(),
                role: &member.role,
                directory: workspace,
                resume: self.resume.as_deref(),
                message: member.message.as_deref(),
                agent,
                instructions: &member.instructions,
                cf: cf::binary()?,
                env,
            })?;
            let program = start.command.clone();
            return Ok((start, program));
        }
        let start = match &self.resume {
            Some(resumed) => ask::resume(agent, resumed)?,
            None => ask::start(agent, session, None)?,
        }
        .ok_or_else(|| {
            Error::Daemon(format!(
                "{} has no window of its own to open on {}",
                harness.name(),
                if self.resume.is_some() {
                    "a conversation it keeps"
                } else {
                    "a new conversation"
                }
            ))
        })?;
        let program = self.install.command().to_string_lossy().into_owned();
        Ok((start, program))
    }

    /// Opens it. A Claude window is opened after Claude has been asked to trust
    /// its folder, and says what came of that ([`Window::trust`]).
    pub fn open(self) -> Result<Window<'a>> {
        let harness = self.install.harness();
        let home = self
            .rig
            .var("HOME")
            .filter(|home| !home.is_empty())
            .ok_or_else(|| Error::Daemon("the rig gives its windows no home".to_owned()))?;
        let workspace = live_folder_in(Path::new(home), &self.folder)?;
        let agent = self.agent.clone().unwrap_or_else(|| agents::agent(harness));
        let kind = agent["kind"]
            .as_str()
            .unwrap_or_else(|| harness.kind())
            .to_owned();
        // A window resumed on a conversation is given its id; one that is not opens on
        // an id of its harness's kind ([`session_for`]).
        let session = self.resume.clone().or_else(|| session_for(&kind));
        let env = window_env(self.rig, &workspace);
        let (start, executable) = self.command(&agent, session.as_deref(), &workspace, &env)?;
        let claude = harness == Harness::Claude;
        let trust = if claude && self.trusting {
            let shut_out = ISOLATION.map(str::to_owned);
            let argv: Vec<String> = std::iter::once(executable.clone())
                .chain(shut_out)
                .collect();
            Some(trust_claude(self.rig, &argv, &workspace)?)
        } else {
            None
        };
        let mut argv = vec![executable];
        if claude {
            // The settings the app writes for each launch (which skip the full-permission
            // warning), and no MCP server, connector or browser.
            argv.extend(ask::claude_settings(&env, LAUNCH, false)?);
            argv.extend(ISOLATION.map(str::to_owned));
        }
        argv.extend(start.args);
        let argv = ask::program(&argv, &env).map_err(|failed| {
            Error::Daemon(format!("{} cannot open a window: {failed}", harness.name()))
        })?;
        let mut window_vars: serde_json::Map<String, Value> = start
            .env
            .iter()
            .map(|(name, value)| (name.clone(), json!(value)))
            .collect();
        window_vars.extend(
            self.env
                .iter()
                .map(|(name, value)| (name.clone(), json!(value))),
        );
        let drop_env: Vec<String> = start.drop_env.into_iter().chain(self.drop_env).collect();
        let opened_at_ms = jiff::Timestamp::now().as_millisecond();
        let opened = self.rig.host(
            "pane.open",
            json!({
                "id": self.id, "generation": 1, "cwd": workspace, "argv": argv,
                "env": window_vars, "dropEnv": drop_env,
                "size": { "rows": 40, "cols": 120 },
            }),
        )?;
        if opened["ok"] != true {
            return Err(Error::Daemon(format!(
                "{} did not open: {opened}",
                harness.name()
            )));
        }
        Ok(Window {
            rig: self.rig,
            harness,
            kind,
            id: self.id.clone(),
            workspace,
            argv,
            session: RefCell::new(session),
            opened_at_ms,
            env,
            trust,
            killed: Cell::new(false),
        })
    }
}

/// The environment a harness is asked where its records are with, for a window in
/// `workspace`: the rig's own, with the folder as its ConsensFlow home (the
/// settings of a Claude window are written there).
fn window_env(rig: &Rig, workspace: &Path) -> Vec<(String, String)> {
    let mut env = rig.environment().to_vec();
    env.push((
        "CONSENSFLOW_HOME".to_owned(),
        workspace
            .join(".consensflow")
            .to_string_lossy()
            .into_owned(),
    ));
    env
}
