//! The rig of the checks that play the chief with its own token and run real
//! harnesses as the members of a project (the receipt door, the stops, the
//! resumed Codex thread, the cut chief, the core bench): the daemon this checkout
//! builds, the real pane host, and a project whose chief is the stand-in agent
//! (which only notes what it is sent) or a real harness, and whose members are
//! the machine's harnesses on their cheap models ([`super::agents`]) or the
//! stand-in. A task is given, paused, resumed or answered at the moment a check
//! chooses, with the chief's token ([`Members::as_chief`]), and what a check
//! shows it shows from what the daemon wrote ([`Members::events`]), from the
//! board's own reads and from the harness's own record ([`super::ask::records`],
//! [`super::claude`]).
//!
//! The pieces are the others of this folder: what a project opens with is a
//! [`Plan`] (the folder, what every window of it is told, the roster), and
//! [`Members::open`] starts the rig the plan needs and waits for the chief's
//! window ([`Members::start`] is the same, which goes on at once: the bench
//! records its chief as ready, or not, and goes on). The windows are the
//! stand-in's, but for the real harnesses of the roster; where one of those is
//! Claude Code, its windows start through [`super::machine::rig_with_stand_ins`]'s
//! shim, which has the stand-in play the windows whose agent's model is `fake` or
//! `fake-chief`, and the rig takes away what the stand-ins wrote in the machine's
//! Claude folder when it ends, and what a real Claude kept of the project's folder.
//!
//! A check says what it saw in an [`Account`], waits with [`find`] (which says
//! what it waits for, and ends with what it found, none when the time ran out),
//! and judges an answer at a door with [`judge`].

mod account;
mod board;
mod chief;
mod closing;
mod command;
mod descent;
mod door;
mod leftovers;
mod plan;
#[cfg(test)]
mod tests;
mod words;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{json, Value};
use tempfile::TempDir;

use super::machine::{live_folder_in, Harness, Installed, ISOLATION};
use super::window::trust_claude;
use super::{agents, installed, last_lines, rig, rig_with_stand_ins, Log};
use crate::process::Run;
use crate::rig::{Config, Project, Rig};
use crate::{files, Error, Result};

use board::task_number;
use leftovers::Leftovers;

pub use account::{clock, plural, plus, plus_maybe, squash, Account};
pub use board::{
    answer_to, brief_delivered, delivered_result, find, first_answer, first_question, is_over,
    last_result, notes_of_work, session_of, MemberWindow, Paused,
};
pub use command::{quiet_command, runs_command, shown, LongCommand};
pub use descent::{
    descending_from, end_chosen, holding, with_cygwin_parents, Ended, Identity, Machine, Running,
    ThisMachine,
};
pub use door::{judge, moves, ms_of, waited_then_worked, Finding, Judged, Seen};
pub use plan::{
    member, member_as, question_brief, stand_in_chief, stand_in_question_brief, stand_in_worker,
    tier_flag, Plan, BENCH_BRIEF, BRIEF, STAND_IN_WORKER,
};
pub use words::named;

/// How long the chief's window has to go idle once the project is open: it is
/// started for the first time, on a machine that may be cold.
const CHIEF_READY: Duration = Duration::from_secs(180);

/// How long the daemon has to be gone once it is killed, as the app's quit does.
const GONE: Duration = Duration::from_secs(10);

/// The rig the real harnesses of `installs` need: where one of them is Claude Code,
/// its windows start through the shim ([`rig_with_stand_ins`], made in `shims`),
/// which plays the stand-in `stand_in` for the windows whose agent's model is
/// `fake` or `fake-chief` and takes away what the stand-ins wrote in the machine's
/// Claude folder and what a real Claude kept of the folders the run names; where
/// none is, the stand-in is the Claude of every window.
pub fn rig_for(installs: &[Installed], shims: &Path, stand_in: &Path) -> Result<Config> {
    if installs
        .iter()
        .any(|install| install.harness() == Harness::Claude)
    {
        rig_with_stand_ins(installs, shims, stand_in)
    } else {
        Ok(rig(installs, stand_in))
    }
}

/// A project of the rig whose chief the check plays.
pub struct Members {
    rig: Rig,
    /// What the rig started with: the app started again on the same home starts
    /// with it ([`Members::restart`]).
    config: Config,
    project: i64,
    chief: String,
    folder: PathBuf,
    trust: Option<&'static str>,
    tiers: BTreeMap<String, String>,
    /// What the real harnesses kept of the folder in places of their own: it goes
    /// when the project does, unless it is to be kept.
    _leftovers: Leftovers,
    /// Where the shim that starts Claude Code is, which the windows run through.
    _shims: TempDir,
}

impl Members {
    /// [`Members::start`], and the chief's window waited for to be idle: until
    /// then nothing is typed into it. A chief that never is the error.
    pub fn open(plan: &Plan, stand_in: &Path, log: &Log) -> Result<Self> {
        let members = Self::start(plan, stand_in, log)?;
        if members.chief_is_idle(log)? {
            return Ok(members);
        }
        Err(Error::Daemon(format!(
            "the chief's window never went idle: {}",
            members.lane("chief")?["activity"]
        )))
    }

    /// Starts the rig the plan needs and opens its project: the roster written,
    /// the folder made and told what the plan says, Claude Code asked to trust it
    /// (where a real one runs in it), the project open with its chief. The chief's
    /// window is still starting ([`Members::chief_is_idle`]). `stand_in` is the
    /// stand-in agent's program.
    pub fn start(plan: &Plan, stand_in: &Path, log: &Log) -> Result<Self> {
        let harnesses = plan.harnesses();
        let installs = harnesses
            .iter()
            .map(|harness| found(*harness, log))
            .collect::<Result<Vec<_>>>()?;
        let shims = tempfile::tempdir().map_err(|source| Error::File {
            action: "make a folder in",
            path: std::env::temp_dir(),
            source,
        })?;
        let config = plan.vars.iter().fold(
            rig_for(&installs, shims.path(), stand_in)?,
            |config, (name, value)| config.var(name, value),
        );
        let mut rig = Rig::start(config.clone())?;
        if super::keep_requested() {
            log.say(format!(
                "{} is set: the home and the project stay at {}",
                super::KEEP,
                rig.keep_home().display()
            ));
        }
        agents::write_roster(&rig, &plan.roster, log)?;
        let folder = prepare(&rig, plan)?;
        rig.forget_claude_memory_of(&folder);
        let trust = match installs
            .iter()
            .find(|install| install.harness() == Harness::Claude)
            .filter(|_| !plan.repository)
        {
            Some(claude) => Some(trusted(&rig, claude, &folder, log)?),
            None => None,
        };
        let project = Project::open(&rig, &plan.chief, json!({ "directory": folder }))?.id();
        let leftovers = Leftovers::of(&harnesses, rig.var("HOME"), &folder);
        Ok(Self {
            rig,
            config,
            project,
            chief: plan.chief.clone(),
            folder,
            trust,
            tiers: BTreeMap::new(),
            _leftovers: leftovers,
            _shims: shims,
        })
    }

    /// Whether the chief's window goes idle, as the board says, within three
    /// minutes (it is started for the first time, on a machine that may be cold):
    /// until it is, nothing is typed into it.
    pub fn chief_is_idle(&self, log: &Log) -> Result<bool> {
        Ok(find(
            log,
            "the chief's window to go idle",
            CHIEF_READY,
            Duration::from_millis(500),
            || Ok((self.lane("chief")?["activity"]["state"] == "idle").then_some(())),
        )?
        .is_some())
    }

    /// The rig.
    pub const fn rig(&self) -> &Rig {
        &self.rig
    }

    /// The project, as the page's views of it.
    pub fn project(&self) -> Project<'_> {
        Project::new(&self.rig, self.project)
    }

    /// The project's id.
    pub const fn project_id(&self) -> i64 {
        self.project
    }

    /// The project's folder.
    pub fn folder(&self) -> &Path {
        &self.folder
    }

    /// What came of asking Claude to trust the folder: none where no real
    /// Claude runs in it.
    pub const fn trust(&self) -> Option<&'static str> {
        self.trust
    }

    /// The id of the agent that is the project's chief.
    pub fn chief(&self) -> &str {
        &self.chief
    }

    /// The Claude config folder the project's Claude windows keep their sessions
    /// and transcripts in ([`super::claude`] reads them there): the
    /// `CLAUDE_CONFIG_DIR` the rig gives them, and else `.claude` in its home,
    /// which is the machine's where a real Claude runs.
    pub fn claude_config(&self) -> PathBuf {
        let var = |name: &str| self.rig.var(name).filter(|value| !value.is_empty());
        match (var("CLAUDE_CONFIG_DIR"), var("HOME")) {
            (Some(config), _) => PathBuf::from(config),
            (None, Some(home)) => Path::new(home).join(".claude"),
            (None, None) => PathBuf::from(".claude"),
        }
    }

    /// The tier the member `agent` was given when it was added.
    pub fn tier_of(&self, agent: &str) -> Option<&str> {
        self.tiers.get(agent).map(String::as_str)
    }

    /// What the page asks the daemon of the project: `op` with `body`, which is
    /// given the project (`project.open` has none of its own to give).
    pub fn page(&self, op: &str, mut body: Value) -> Result<Value> {
        if let Some(fields) = body.as_object_mut() {
            fields.insert("project".to_owned(), json!(self.project));
        }
        self.rig.page(op, body)
    }

    /// The board.
    pub fn board(&self) -> Result<Value> {
        self.project().board()
    }

    /// The lane of the member or session `handle`, the newest one: a member's
    /// work runs in a session of its own, and its lane is its newest session's.
    pub fn lane(&self, handle: &str) -> Result<Value> {
        self.project().lane(handle)
    }

    /// The task `number`, as the board reads it, with its whole thread: each
    /// message's state and receipt.
    pub fn task(&self, number: i64) -> Result<Value> {
        self.project().task(number)
    }

    /// The messages in the inbox of `participant`; of the human's, whose name is
    /// none, where there is none.
    pub fn inbox(&self, participant: Option<&str>) -> Result<Vec<Value>> {
        match participant {
            Some(participant) => self.project().inbox(participant),
            None => Ok(self.page("inbox.get", json!({}))?["messages"]
                .as_array()
                .cloned()
                .unwrap_or_default()),
        }
    }

    /// Adds the agent `agent` to the staff, as the human does, with the `roles`
    /// asked for (none: a worker). Its tier, which the tasks for it name.
    pub fn add(&mut self, agent: &str, roles: &[&str]) -> Result<String> {
        let mut request = json!({ "agent": agent });
        if !roles.is_empty() {
            request["roles"] = json!(roles);
        }
        let added = self.page("member.add", request)?;
        if added["ok"] != true {
            return Err(Error::Daemon(format!("member.add {agent}: {added}")));
        }
        let tier = added["member"]["tier"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        self.tiers.insert(agent.to_owned(), tier.clone());
        Ok(tier)
    }

    /// A task for the member `agent`, given by the chief: its number.
    pub fn give(&self, agent: &str, brief: &str) -> Result<i64> {
        let tier = self
            .tier_of(agent)
            .ok_or_else(|| Error::Daemon(format!("{agent} was never added to the staff")))?;
        let mut words: Vec<String> = vec!["task".to_owned(), "add".to_owned()];
        words.extend(tier_flag(tier));
        words.extend(["--json".to_owned(), brief.to_owned()]);
        let added = self.as_chief(&words)?;
        task_number(&added.stdout).ok_or_else(|| Error::Daemon(format!("cf task add: {added}")))
    }

    /// The window of the member `agent`'s newest session: none until the daemon
    /// has opened one.
    pub fn window_of(&self, agent: &str) -> Result<Option<MemberWindow>> {
        Ok(self.window_in(self.lane(agent)?))
    }

    /// The window `lane` of the board has: none while it has no pane. For a check
    /// that has the lane already, and asks the board nothing again.
    pub fn window_in(&self, lane: Value) -> Option<MemberWindow> {
        if lane["pane"].is_null() {
            return None;
        }
        let pane = lane["pane"].clone();
        let open = self
            .rig
            .open_frames()
            .into_iter()
            .rev()
            .find(|frame| frame["id"] == pane["id"]);
        Some(MemberWindow {
            handle: lane["participant"]["handle"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
            pane,
            open,
            activity: lane["activity"].clone(),
            lane,
        })
    }

    /// Which process the program of `window` is, as the pane host opened it for the
    /// daemon: read once, as the host answered the open, and kept
    /// ([`Rig::window_process`]). What descends from it is the window's own tree, and
    /// nothing does once it has gone, whatever has its number since.
    pub fn window_process(&self, window: &MemberWindow) -> Option<Identity> {
        self.rig.window_process(&window.pane)
    }

    /// Every `pane.open` the daemon made for the windows of the member `agent`,
    /// the oldest first: a member's work runs in a session of its own, and each
    /// has its window, which is closed when the daemon gives it up and is none
    /// of the lane's after.
    pub fn windows_opened_for(&self, agent: &str) -> Vec<Value> {
        let prefix = format!("p{}-{agent}-", self.project);
        self.rig
            .open_frames()
            .into_iter()
            .filter(|frame| {
                frame["id"]
                    .as_str()
                    .is_some_and(|id| id.starts_with(&prefix))
            })
            .collect()
    }

    /// The end of the screen of the window whose pane is `pane` (its id), as a
    /// line ([`last_lines`]): what a check says of a window to say what it saw.
    pub fn tail(&self, pane: &str) -> String {
        last_lines(&self.rig.screen(pane))
    }

    /// Pastes `body` into the window `pane` as the daemon does (`pane.write_paste`:
    /// a bracketed paste, then Enter once the window has drawn it).
    pub fn paste(&self, pane: &Value, body: &str) -> Result<Value> {
        self.rig.host(
            "pane.write_paste",
            json!({ "id": pane["id"], "generation": pane["generation"], "body": body }),
        )
    }

    /// Presses `keys` in the window `pane` as the human does (`pane.input`: the
    /// bytes the keys send).
    pub fn press(&self, pane: &Value, keys: &[u8]) -> Result<Value> {
        self.rig.host(
            "pane.input",
            json!({ "id": pane["id"], "generation": pane["generation"], "bytes": keys }),
        )
    }

    /// The human's Pause of the task, and when it was made.
    pub fn pause(&self, task: i64) -> Result<Paused> {
        let made_ms = jiff::Timestamp::now().as_millisecond();
        let answer = self.page("task.pause", json!({ "task": task }))?;
        Ok(Paused { made_ms, answer })
    }

    /// What the daemon wrote as it went, from its event file.
    pub fn events(&self) -> Vec<Value> {
        self.rig.events()
    }

    /// Whether the project is open, as the daemon says: it is not after the app
    /// started again, until the human resumes it ([`Members::resume`]).
    pub fn is_open(&self) -> Result<bool> {
        let listed = self.rig.page("projects.list", json!({}))?;
        Ok(listed["projects"]
            .as_array()
            .and_then(|projects| projects.iter().find(|found| found["id"] == self.project))
            .is_some_and(|found| found["state"] == "open"))
    }

    /// The human's Resume of the project, as the page asks it (`project.resume`):
    /// its chief's window comes back, and the window of each member with a task
    /// at work or waiting, each on its own conversation, a window the quit cut
    /// told so. The daemon's answer.
    pub fn resume(&self) -> Result<Value> {
        let resumed = self.page("project.resume", json!({}))?;
        if resumed["ok"] != true {
            return Err(Error::Daemon(format!("project.resume: {resumed}")));
        }
        Ok(resumed)
    }

    /// The app's quit, and the app started again on the same home: the daemon
    /// dies first, then the pane host, which ends every window, as the quit
    /// orders it. The daemon that starts suspends the project and opens nothing:
    /// it waits for the human's Resume ([`Members::resume`]).
    pub fn restart(self, log: &Log) -> Result<Self> {
        let Self {
            mut rig,
            config,
            project,
            chief,
            folder,
            trust,
            tiers,
            _leftovers,
            _shims,
        } = self;
        rig.kill_daemon();
        rig.wait_for_daemon_exit(GONE)?;
        // Real harnesses' windows: a window stopped in a command takes its time to leave.
        let root = rig.close_keeping_the_root_within(closing::EXIT)?;
        let mut rig = Rig::start(config.clone().existing_root(root))?;
        if super::keep_requested() {
            rig.keep_home();
        }
        rig.forget_claude_memory_of(&folder);
        log.say("the app started again on the same home");
        Ok(Self {
            rig,
            config,
            project,
            chief,
            folder,
            trust,
            tiers,
            _leftovers,
            _shims,
        })
    }

    /// Ends the rig: the daemon, the pane host and their windows, and the
    /// records of the stand-ins. The harnesses' windows are given the time the
    /// host gives them to leave, and a rig that does not end says which of its
    /// processes were left ([`closing`]).
    pub fn close(self) -> Result {
        self.close_within(closing::EXIT)
    }

    /// [`Members::close`], with `within` for the daemon and the pane host to end.
    pub fn close_within(self, within: Duration) -> Result {
        closing::end_within(self.rig, within)
    }
}

/// The machine's `harness`, found, with its version and where its login is read
/// from said.
fn found(harness: Harness, log: &Log) -> Result<Installed> {
    let found = installed(harness)?;
    log.say(format!(
        "{} is {} ({}); its login is read from {}",
        harness.name(),
        found.command().display(),
        found.version(),
        found.home().display()
    ));
    Ok(found)
}

/// The folder of the project, made in the live folders of the home the rig gives
/// its windows, and what the plan says of it written in: the brief where Claude
/// Code reads it (`CLAUDE.md`) and the other harnesses do (`AGENTS.md`), and the
/// files.
fn prepare(rig: &Rig, plan: &Plan) -> Result<PathBuf> {
    let home = rig
        .var("HOME")
        .filter(|home| !home.is_empty())
        .ok_or_else(|| Error::Daemon("the rig gives its windows no home".to_owned()))?;
    let folder = live_folder_in(Path::new(home), &plan.folder)?;
    for name in ["AGENTS.md", "CLAUDE.md"] {
        files::write(&folder.join(name), plan.brief)?;
    }
    for (path, text) in &plan.files {
        files::write(&folder.join(path), text)?;
    }
    if plan.repository {
        let made = Run::new(Path::new("git"))
            .args(["init", "-q"])
            .arg(&folder)
            .inheriting_env()
            .run()?;
        if made.code != Some(0) {
            return Err(Error::Daemon(format!(
                "git init {}: {made}",
                folder.display()
            )));
        }
    }
    Ok(folder)
}

/// Claude Code asked to trust `folder`, as a person is: through the real Claude
/// and the flags that shut the human's MCP servers and browser out.
fn trusted(rig: &Rig, claude: &Installed, folder: &Path, log: &Log) -> Result<&'static str> {
    let argv: Vec<String> = std::iter::once(claude.command().to_string_lossy().into_owned())
        .chain(ISOLATION.map(str::to_owned))
        .collect();
    let trust = trust_claude(rig, &argv, folder)?;
    log.say(format!("Claude Code and the folder: {trust}"));
    Ok(trust)
}
