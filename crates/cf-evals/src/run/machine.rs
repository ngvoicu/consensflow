//! This machine as a run meets it: the user's home and the folders a run works in
//! below it, the harnesses of the run found as a shell finds them, and the rig
//! those harnesses run in, on the user's own logins.
//!
//! A run's windows get the user's own home, never this shell's session identity
//! and none of the test pane host's sandbox. Its Claude Code windows run on a
//! config folder of the run's own, with a copy of the machine's login
//! ([`super::login`]), never on the user's `~/.claude`: the rig starts the daemon and
//! every window on it, and the folder goes with the run. The wrappers the run writes
//! (`~/.consensflow-candidate/evals/bin`) are first on the `PATH`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use cf_e2e::live::{self, Harness, Installed};
use cf_e2e::process::own_var;
use cf_e2e::rig::{ClaudeConfig, Config};
use cf_e2e::{checkout, Error, Result as Support};

use super::login::Throwaway;
use super::options::Crew;
use crate::plan::ChiefSetup;

/// Where the folders of a run are, below the user's home: one fixed workspace,
/// trusted once per run (Claude asks about an unknown folder), and the wrappers.
const EVALS: [&str; 2] = [".consensflow-candidate", "evals"];

/// The folders on a Mac's or a Linux machine's `PATH` that a window finds its
/// programs in besides the harnesses', as a login shell has them.
const SYSTEM: [&str; 6] = [
    "/opt/homebrew/bin",
    "/usr/local/bin",
    "/usr/bin",
    "/bin",
    "/usr/sbin",
    "/sbin",
];

/// This machine, for a run.
#[derive(Debug)]
pub struct Machine {
    /// The user's own home: `HOME`, and on Windows the user's profile.
    pub home: PathBuf,
    /// The harnesses of the run, each as its windows run it.
    pub installs: Vec<Installed>,
    /// The folder of Node, which a harness that npm installed needs on the `PATH`
    /// to start, and the wrappers on Windows run on.
    pub node: Option<PathBuf>,
    /// The Claude config folder the run's Claude Code windows run on: a copy of the
    /// machine's login in a folder of the run's own. None where the run has no real
    /// Claude Code (the dry runs) and where the machine has no login to copy (Windows):
    /// its Claude is as it was.
    pub login: Option<Arc<Throwaway>>,
}

impl Machine {
    /// The machine's harnesses of this run: each found as a shell finds it. One
    /// that is not on the `PATH` is an error that says so; its login is the
    /// window's to ask for (a login page shows on the chief's screen in the report).
    /// The windows of Claude Code run on `login`, where there is one.
    pub fn of(crew: &Crew, login: Option<Arc<Throwaway>>) -> Support<Self> {
        let home = live::machine::home()?;
        let mut names: Vec<&str> = vec![crew.chief.as_str()];
        names.extend(crew.staff.iter().map(String::as_str));
        let node = live::command("node")
            .ok()
            .and_then(|node| node.parent().map(Path::to_path_buf));
        let mut installs: Vec<Installed> = Vec::new();
        for name in names {
            let Some(harness) = Harness::from_name(name) else {
                continue;
            };
            if installs.iter().any(|install| install.harness() == harness) {
                continue;
            }
            let install = installed(harness, live::command(name)?, &home, login.as_ref());
            installs.push(match &node {
                Some(node) => install.with_node(node),
                None => install,
            });
        }
        Ok(Self {
            home,
            installs,
            node,
            login,
        })
    }

    /// The folder the run's Claude Code windows keep their records in, made if it is
    /// not (the login is refused here, before the run spends anything, if it would end
    /// before the run does): the run's own where it has a login, the user's `.claude` where
    /// Claude Code is as it was, and none where Claude Code has no window in the run.
    pub fn claude_config(&self) -> Support<Option<PathBuf>> {
        if self.install("claude").is_err() {
            return Ok(None);
        }
        match &self.login {
            Some(login) => login.folder().map(Some),
            None => Ok(Some(self.home.join(".claude"))),
        }
    }

    /// Where the run's Claude Code windows keep their records: the folder of [`Self::claude_config`],
    /// or the user's `.claude` for a run that has no Claude Code (nothing reads it then).
    pub fn claude_records(&self) -> Support<PathBuf> {
        Ok(self
            .claude_config()?
            .unwrap_or_else(|| self.home.join(".claude")))
    }

    /// What the run did to the user's Claude setup or to the copy of the login, if it did
    /// anything, once its rig has ended.
    pub fn login_findings(&self) -> Option<String> {
        self.login.as_ref().and_then(|login| login.findings())
    }

    /// The harness `name` of the run.
    pub fn install(&self, name: &str) -> Support<&Installed> {
        Harness::from_name(name)
            .and_then(|harness| {
                self.installs
                    .iter()
                    .find(|install| install.harness() == harness)
            })
            .ok_or_else(|| Error::Daemon(format!("{name} is no harness of this run")))
    }

    /// The folder a run works in below the home: `workspace` and `bin`.
    fn below(&self, name: &str) -> PathBuf {
        EVALS
            .iter()
            .fold(self.home.clone(), |folder, part| folder.join(part))
            .join(name)
    }

    /// The one fixed workspace, trusted once per run.
    pub fn workspace(&self) -> PathBuf {
        self.below("workspace")
    }

    /// The wrappers every Claude and Codex window of a run starts through, first on
    /// the `PATH` of the daemon.
    pub fn bin(&self) -> PathBuf {
        self.below("bin")
    }

    /// The `PATH` of a run's windows: the wrappers first, then the folders of the
    /// harnesses and of Node, then the system's (on Windows the machine's own: a
    /// window finds there what it needs of Windows).
    pub fn path(&self) -> String {
        let mut folders: Vec<PathBuf> = vec![self.bin()];
        for install in &self.installs {
            folders.extend(install.command().parent().map(Path::to_path_buf));
        }
        folders.extend(self.node.clone());
        if cfg!(windows) {
            folders.extend(std::env::split_paths(&own_var("PATH").unwrap_or_default()));
        } else {
            folders.extend(SYSTEM.iter().map(PathBuf::from));
        }
        let mut seen: Vec<PathBuf> = Vec::new();
        for folder in folders {
            if !folder.as_os_str().is_empty() && !seen.contains(&folder) {
                seen.push(folder);
            }
        }
        std::env::join_paths(&seen)
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    /// The rig a run is on: the windows run the machine's harnesses on its own
    /// logins, through the wrappers, with what the arm and the chief's model add.
    /// `stand_in` is the program a rig has first on the `PATH` of the windows it
    /// has no use for here (the rig's own, which the `PATH` below leaves out).
    pub fn rig(&self, crew: &Crew, chief: &ChiefSetup, stand_in: &Path) -> Config {
        let home = self.home.to_string_lossy().into_owned();
        let mut config = live::rig(&self.installs, stand_in)
            .var("HOME", &home)
            .var(
                "CODEX_HOME",
                codex_home(&self.home).to_string_lossy().into_owned(),
            )
            .var("PATH", self.path());
        if cfg!(windows) {
            config = config.var("USERPROFILE", &home);
        }
        if crew.nocard {
            config = config.var(
                "CONSENSFLOW_EVAL_CHIEF_CARD",
                checkout::path("evals/fixtures/no-card.md")
                    .to_string_lossy()
                    .into_owned(),
            );
        }
        for (name, value) in &chief.env {
            config = config.var(name, value);
        }
        config
    }
}

/// The harness `harness` at `command` as the run's windows start it: the user's home for its
/// login, and for Claude Code, where the run has a throwaway login, the config folder of it.
fn installed(
    harness: Harness,
    command: PathBuf,
    home: &Path,
    login: Option<&Arc<Throwaway>>,
) -> Installed {
    let install = Installed::at(harness, command, login_of(harness, home));
    match login {
        Some(login) if harness == Harness::Claude => {
            install.with_claude_config(Arc::clone(login) as Arc<dyn ClaudeConfig>)
        }
        _ => install,
    }
}

/// The home `harness` reads its login from: Codex's own folder (`CODEX_HOME`, or
/// `.codex` in the user's home), every other one's the user's home.
fn login_of(harness: Harness, home: &Path) -> PathBuf {
    if harness == Harness::Codex {
        codex_home(home)
    } else {
        home.to_path_buf()
    }
}

/// Codex's own folder: `CODEX_HOME` where the machine names one.
fn codex_home(home: &Path) -> PathBuf {
    own_var("CODEX_HOME")
        .filter(|folder| !folder.is_empty())
        .map_or_else(|| home.join(".codex"), PathBuf::from)
}

#[cfg(test)]
mod tests;
