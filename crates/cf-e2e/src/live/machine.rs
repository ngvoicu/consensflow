//! This machine, as a live run meets it: the harnesses it has (Claude Code,
//! OpenCode, Pi, Devin and Codex), found as a shell finds them with the home
//! each reads its login from, and the rig whose windows run them.
//!
//! Nothing is copied: a window reads its login where it is, as it does for the
//! person it belongs to (and writes its own sessions there, as it does for
//! them). What a window is given of the machine is what [`rig`] says: the
//! variables the harness reads its login by, the folders it and Node are in, a
//! locale that draws its screen, and the user's name, which a harness may ask
//! the system's keychain by; and never the `CLAUDE_CONFIG_DIR` the rig sets for
//! the stand-in, which makes a real Claude Code look for its login in a folder
//! that holds none and open on its first-run dialog. A run given a Claude config
//! folder of its own (`--claude-config`) has its windows keep their config there:
//! its caller made it and copied the login into it.
//!
//! Claude Code has two programs of ours in front of it: [`isolated_claude`] adds
//! the flags that shut the human's MCP servers and browser out, and
//! [`shimmed_claude`] also plays the windows of the agents the rig stands in for
//! ([`shimmed_claude_with_servers`] gives the real ones the MCP servers a file
//! names, and no others).

mod path;
#[cfg(test)]
mod tests;
mod wrapper;

use std::io;
use std::path::{Path, PathBuf};

use crate::process::{own_var, Run};
use crate::rig::Config;
use crate::{checkout, files, Error, Result};

pub use path::{command, working_command};
pub use wrapper::{isolated_claude, shimmed_claude, shimmed_claude_with_servers, ISOLATION};

/// What the `.cmd` of a command a window opens on says on Windows, which opens a
/// window only on the shape of an npm shim (`"<program>" "<script>" %*`): the
/// program that starts a harness through a wrapper of the run's own is named in it
/// as both.
pub use crate::stand_in::window_shim;

/// A harness the live tests run: the five of ConsensFlow's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Harness {
    Claude,
    OpenCode,
    Pi,
    Devin,
    Codex,
}

impl Harness {
    /// Every harness, in the order the live approval test rotates them through
    /// its positions: each is the chief of one project and the one after it
    /// is that project's worker.
    pub const ALL: [Harness; 5] = [
        Harness::Claude,
        Harness::OpenCode,
        Harness::Pi,
        Harness::Devin,
        Harness::Codex,
    ];

    /// What it is called on a command line, and what its command is called.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::OpenCode => "opencode",
            Self::Pi => "pi",
            Self::Devin => "devin",
            Self::Codex => "codex",
        }
    }

    /// The harness a word on a command line names.
    pub fn from_name(word: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|harness| harness.name() == word)
    }

    /// What the roster calls its kind.
    pub const fn kind(self) -> &'static str {
        match self {
            Self::Claude => "claude-code",
            other => other.name(),
        }
    }

    /// Where its login is, below the home it reads it from: the first that is
    /// there says the machine has been set up for it. Codex's home is its own
    /// folder, which is all there is to look for. Devin's are the places of the
    /// XDG layout; on Windows it keeps its login elsewhere ([`login_places`]).
    const fn login_marks(self) -> &'static [&'static str] {
        match self {
            Self::Claude => &[".claude.json", ".claude"],
            Self::OpenCode => &[".local/share/opencode", ".config/opencode"],
            Self::Pi => &[".pi"],
            Self::Devin => &[".config/devin", ".local/share/devin"],
            Self::Codex => &[""],
        }
    }

    /// What to do about a machine that has no login for it.
    const fn log_in(self) -> &'static str {
        match self {
            Self::Claude => "log in by running `claude` once",
            Self::OpenCode => "log in by running `opencode` once",
            Self::Pi => "log in by running `pi` once",
            Self::Devin => "log in with `devin auth login`",
            Self::Codex => "log in with `codex login`, or name its home in CODEX_HOME",
        }
    }
}

/// A harness installed on this machine, as its windows run it: its command,
/// the home its login is read from, and the folder of Node, which one that npm
/// installed needs on the `PATH` to start (on Windows its command is a shim
/// that starts Node).
///
/// A window reads its login by variables of its environment, which the rig
/// sets to a home of its own: Codex's is `CODEX_HOME`, and its `home` is that
/// folder; every other one's is the user's own home (`HOME`, with the folders
/// of the XDG layout below it), and its `home` is that.
#[derive(Debug, Clone)]
pub struct Installed {
    harness: Harness,
    command: PathBuf,
    home: PathBuf,
    node: Option<PathBuf>,
}

impl Installed {
    /// A `harness` whose command is `command` and whose login is read from `home`.
    pub fn at(harness: Harness, command: impl Into<PathBuf>, home: impl Into<PathBuf>) -> Self {
        Self {
            harness,
            command: command.into(),
            home: home.into(),
            node: None,
        }
    }

    /// This harness, whose windows also find Node in the folder `node`.
    #[must_use]
    pub fn with_node(mut self, node: impl Into<PathBuf>) -> Self {
        self.node = Some(node.into());
        self
    }

    /// Which harness it is.
    pub fn harness(&self) -> Harness {
        self.harness
    }

    /// Where its command is.
    pub fn command(&self) -> &Path {
        &self.command
    }

    /// The home its login is read from.
    pub fn home(&self) -> &Path {
        &self.home
    }

    /// The version it says it is, as its first line says it; `unknown` when it
    /// says none.
    pub fn version(&self) -> String {
        let word = if self.harness == Harness::Devin {
            "version"
        } else {
            "--version"
        };
        let ran = Run::new(&self.command)
            .arg(word)
            .inheriting_env()
            .run()
            .ok()
            .filter(|ran| ran.code == Some(0));
        let line = ran
            .as_ref()
            .and_then(|ran| ran.stdout.lines().next().map(|line| line.trim().to_owned()))
            .filter(|line| !line.is_empty())
            .unwrap_or_else(|| "unknown".to_owned());
        // A process it left held its output open: what it said is all that was read.
        if ran.is_some_and(|ran| ran.held) {
            format!("{line} (a process it left held its output open)")
        } else {
            line
        }
    }

    /// The folders its windows need on their `PATH`: its command's, and Node's
    /// where that is another.
    fn folders(&self) -> Vec<&Path> {
        let folder = self.command.parent().unwrap_or_else(|| Path::new(""));
        let mut folders = vec![folder];
        if let Some(node) = self.node.as_deref().filter(|node| *node != folder) {
            folders.push(node);
        }
        folders
    }

    /// The variables that make its windows read the machine's login, where the
    /// rig would have them read its own home's: `machine` says what a variable
    /// is on this machine.
    fn login(&self, machine: &dyn Fn(&str) -> Option<String>) -> Vec<(String, String)> {
        let home = self.home.to_string_lossy().into_owned();
        if self.harness == Harness::Codex {
            return vec![("CODEX_HOME".to_owned(), home)];
        }
        let mut vars = vec![("HOME".to_owned(), home.clone())];
        if cfg!(windows) {
            vars.push(("USERPROFILE".to_owned(), home.clone()));
        }
        // The rig puts the config folder below its own home: where the machine
        // keeps it, or where the XDG layout does, is where these read theirs.
        let config = machine("XDG_CONFIG_HOME")
            .filter(|folder| !folder.is_empty())
            .unwrap_or_else(|| self.home.join(".config").to_string_lossy().into_owned());
        vars.push(("XDG_CONFIG_HOME".to_owned(), config));
        // The others default below the home, which is the machine's now; one
        // the machine names is its own.
        for name in ["XDG_DATA_HOME", "XDG_STATE_HOME", "XDG_CACHE_HOME"] {
            if let Some(folder) = machine(name).filter(|folder| !folder.is_empty()) {
                vars.push((name.to_owned(), folder));
            }
        }
        vars
    }

    /// A rig whose windows run the stand-in `stand_in` for Claude and find this
    /// harness, logged in as the machine is.
    pub fn rig(&self, stand_in: impl Into<PathBuf>) -> Config {
        rig(std::slice::from_ref(self), stand_in)
    }
}

/// What a Windows window reads of the machine besides what every program of a
/// rig is given already (the system's own folders, the user's name, `TEMP` and
/// the command interpreter, which the rig and `process` pass): the folders the
/// user's settings and the programs are in, and what Node describes the machine
/// by. Devin keeps its settings and sessions under `APPDATA`.
const WINDOWS: [&str; 9] = [
    "APPDATA",
    "LOCALAPPDATA",
    "ProgramData",
    "ProgramFiles",
    "ProgramFiles(x86)",
    "CommonProgramFiles",
    "NUMBER_OF_PROCESSORS",
    "PROCESSOR_ARCHITECTURE",
    "OS",
];

/// The variables of [`WINDOWS`] that `machine` has, in the order they are listed,
/// where `windows` says the run is on Windows; on another machine there are none.
fn windows_environment(
    machine: &dyn Fn(&str) -> Option<String>,
    windows: bool,
) -> Vec<(String, String)> {
    if !windows {
        return Vec::new();
    }
    WINDOWS
        .into_iter()
        .filter_map(|name| Some((name.to_owned(), machine(name)?)))
        .collect()
}

/// Where the report of a rig built for the real harnesses is kept, from the checkout's
/// root: under its build folder, not in the machine's temp folder. The windows of such a
/// rig show what a real harness drew with the person's login, which no masker is sure to
/// hide, and the temp folder would keep it for good; a clean of the build folder takes
/// this away. CI uploads it for a failed job (ten cases of the rig suite build on this
/// rig with the stand-in, and hold no real harness's output there).
const REPORTS: &str = "app/src-tauri/target/live-reports";

/// A rig whose windows run the stand-in `stand_in` for Claude and find every
/// harness of `installs`, logged in as the machine is: each one's command's
/// folder, and Node's where it is another, are on their `PATH` (in the order
/// given, none twice), and the variables they read their logins by are the
/// machine's. Claude Code reads its config where `CLAUDE_CONFIG_DIR` does not
/// point, so a rig that has it has the variable unset, but where the run was given a
/// folder of its own for it (`--claude-config`: one its caller made and copied a login
/// into, which keeps the owner's own config out of the run). The windows have a
/// locale that draws their screens, and the user's name, which a harness may
/// ask the system's keychain by; on Windows they also have the machine's own
/// folders for settings and programs ([`WINDOWS`]).
pub fn rig(installs: &[Installed], stand_in: impl Into<PathBuf>) -> Config {
    let config = rig_keeping_claude_in(installs, stand_in, super::option("claude-config"));
    keeping_pi_in(config, installs, super::option("pi-agent-dir"))
}

/// `config`, whose Pi windows keep their login, settings and sessions in `agent_dir`
/// where it names a folder (`--pi-agent-dir`: one the run's caller made and copied
/// Pi's login into, which keeps the owner's own folder out of the run). The daemon
/// reads Pi's sessions by the same variable.
fn keeping_pi_in(config: Config, installs: &[Installed], agent_dir: Option<String>) -> Config {
    match agent_dir {
        Some(folder)
            if installs
                .iter()
                .any(|install| install.harness == Harness::Pi) =>
        {
            config.var("PI_CODING_AGENT_DIR", folder)
        }
        _ => config,
    }
}

/// [`rig`], whose Claude windows keep their config in `claude_config` where it names a
/// folder.
fn rig_keeping_claude_in(
    installs: &[Installed],
    stand_in: impl Into<PathBuf>,
    claude_config: Option<String>,
) -> Config {
    let machine = |name: &str| own_var(name);
    let mut config = Config::new(stand_in)
        .var("LANG", "en_US.UTF-8")
        .reports_in(checkout::path(REPORTS));
    for name in ["USER", "LOGNAME"] {
        if let Some(value) = machine(name) {
            config = config.var(name, value);
        }
    }
    for (name, value) in windows_environment(&machine, cfg!(windows)) {
        config = config.var(&name, value);
    }
    let mut folders: Vec<&Path> = Vec::new();
    for install in installs {
        for folder in install.folders() {
            if !folders.contains(&folder) {
                folders.push(folder);
                config = config.also_on_path(folder);
            }
        }
        for (name, value) in install.login(&machine) {
            config = config.var(&name, value);
        }
        if install.harness == Harness::Claude {
            config = match &claude_config {
                Some(folder) => config.var("CLAUDE_CONFIG_DIR", folder.clone()),
                None => config.unset("CLAUDE_CONFIG_DIR"),
            };
        }
    }
    config
}

/// A rig for `installs`, one of which is the machine's Claude Code, whose Claude
/// windows are the stand-in agent's (`stand_in`) where the agent's model is
/// `fake` or `fake-chief`, and the real Claude Code's otherwise, started through
/// [`shimmed_claude`] (made in `folder`). The stand-ins write their records
/// where the machine's Claude keeps its own, which is the person's: so the rig
/// takes away those it wrote when it ends, and no others
/// ([`Config::stand_ins_write_in`]).
pub fn rig_with_stand_ins(
    installs: &[Installed],
    folder: &Path,
    stand_in: &Path,
) -> Result<Config> {
    let claude = installs
        .iter()
        .find(|install| install.harness == Harness::Claude)
        .ok_or_else(|| Error::Program {
            action: "find",
            program: Harness::Claude.name().to_owned(),
            source: io::Error::new(
                io::ErrorKind::NotFound,
                "a rig whose Claude windows are the real Claude Code's and the stand-in's needs Claude Code installed",
            ),
        })?;
    let shim = shimmed_claude(folder, claude.command(), stand_in)?;
    // Where the daemon reads Claude's records: the run's own config where it was given one.
    let records =
        super::option("claude-config").map_or_else(|| claude.home().join(".claude"), PathBuf::from);
    Ok(rig(installs, shim).stand_ins_write_in(records))
}

/// The machine's `harness`, found as a person's shell finds it, with the home
/// its login is read from. A machine without it, or without the home it logs
/// in at, is an error that says what to do.
pub fn installed(harness: Harness) -> Result<Installed> {
    let name = harness.name();
    let command = command(name)?;
    let home = home_of(
        harness,
        own_var("CODEX_HOME"),
        own_var("HOME").or_else(|| own_var("USERPROFILE")),
    )
    .ok_or_else(|| Error::Program {
        action: "find the home of",
        program: name.to_owned(),
        source: io::Error::new(
            io::ErrorKind::NotFound,
            "this run has neither a home nor CODEX_HOME to look for its login in",
        ),
    })?;
    let places = login_places(harness, &home, own_var("APPDATA").as_deref(), cfg!(windows));
    if !has_login(&places) {
        return Err(no_login(harness, &places));
    }
    let installed = Installed::at(harness, command, home);
    // A machine with no Node has a harness that needs none (a native one).
    Ok(match command_folder("node") {
        Some(node) => installed.with_node(node),
        None => installed,
    })
}

/// The machine's Codex: [`installed`] for the harness the image designer runs on.
pub fn codex() -> Result<Installed> {
    installed(Harness::Codex)
}

/// Where `harness` keeps its login on a machine whose home is `home`: the places
/// of its marks below it, but for Devin on Windows (`windows`), which keeps its
/// login, its config and its sessions in the one folder `devin` in `%APPDATA%`
/// (`appdata`; `AppData\Roaming` in the home where the machine names none), as
/// the product's own lookup has it.
fn login_places(
    harness: Harness,
    home: &Path,
    appdata: Option<&str>,
    windows: bool,
) -> Vec<PathBuf> {
    if harness == Harness::Devin && windows {
        let roaming = match appdata.filter(|folder| !folder.is_empty()) {
            Some(folder) => PathBuf::from(folder),
            None => home.join("AppData").join("Roaming"),
        };
        return vec![roaming.join("devin")];
    }
    harness
        .login_marks()
        .iter()
        .map(|mark| home.join(mark))
        .collect()
}

/// Whether one of `places` is there: the machine has been set up for the harness.
fn has_login(places: &[PathBuf]) -> bool {
    places.iter().any(|place| place.exists())
}

/// A machine that has no login for `harness`: the places it keeps one in, the
/// first named and the others after it, and what to do.
fn no_login(harness: Harness, places: &[PathBuf]) -> Error {
    let others: Vec<String> = places
        .iter()
        .skip(1)
        .map(|place| place.display().to_string())
        .collect();
    let also = if others.is_empty() {
        String::new()
    } else {
        format!(" (or in {})", others.join(", "))
    };
    Error::File {
        action: "find",
        path: places.first().cloned().unwrap_or_default(),
        source: io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "{} keeps its login there{also}: {}",
                harness.name(),
                harness.log_in()
            ),
        ),
    }
}

/// The folder the program `name` is in, if it is on the `PATH`.
fn command_folder(name: &str) -> Option<PathBuf> {
    command(name).ok()?.parent().map(Path::to_path_buf)
}

/// The home `harness` reads its login from: Codex's is `CODEX_HOME` when it is
/// set, otherwise `.codex` in the user's home; every other one's is the user's
/// home. Made whole against the folder the run is in.
fn home_of(
    harness: Harness,
    codex_home: Option<String>,
    user_home: Option<String>,
) -> Option<PathBuf> {
    let named = |value: Option<String>| value.filter(|value| !value.is_empty());
    let home = match (harness, named(codex_home)) {
        (Harness::Codex, Some(home)) => PathBuf::from(home),
        (Harness::Codex, None) => Path::new(&named(user_home)?).join(".codex"),
        (_, _) => PathBuf::from(named(user_home)?),
    };
    Some(std::path::absolute(&home).unwrap_or(home))
}

/// The user's own home on this machine: `HOME`, else `USERPROFILE`.
pub fn home() -> Result<PathBuf> {
    own_var("HOME")
        .or_else(|| own_var("USERPROFILE"))
        .filter(|home| !home.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| Error::Program {
            action: "find the home of",
            program: "the user".to_owned(),
            source: io::Error::new(
                io::ErrorKind::NotFound,
                "this run has neither HOME nor USERPROFILE",
            ),
        })
}

/// A live test's own folder, `~/.consensflow-candidate/live/<name>`, made: where
/// a harness's window runs, so that what Claude Code asks of a folder (its trust)
/// is asked once and kept.
pub fn live_folder(name: &str) -> Result<PathBuf> {
    live_folder_in(&home()?, name)
}

/// [`live_folder`] under the home `home`: the home a rig gives its windows, which
/// is the machine's where the windows run the machine's harnesses.
pub fn live_folder_in(home: &Path, name: &str) -> Result<PathBuf> {
    let folder = home.join(".consensflow-candidate").join("live").join(name);
    files::make_dir(&folder)?;
    Ok(folder)
}
