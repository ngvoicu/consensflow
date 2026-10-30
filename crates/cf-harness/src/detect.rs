//! Where each harness's CLI is on this machine: on the PATH the daemon was
//! given first ([`Env::search_path`]), then in the places each installs itself,
//! which a Finder-launched app's PATH may lack, under the home (and npm's folder
//! on Windows, which the environment names), then in the system's own folders
//! of programs (`/opt/homebrew/bin` on a Mac: [`Env::tool_folders`]), and last
//! Pi's managed install, `~/.pi/agent/bin`, which Pi's installers put on the
//! PATH of the shell they ran in, not of an app already open. Only a
//! login shell puts those on the PATH, and one too slow to answer left a
//! Homebrew `codex` not installed (2026-10-15). The daemon runs everything with
//! them after its PATH, and a harness is still looked for on the PATH as given:
//! a CLI the human keeps in the home comes before an older one of Homebrew's. A
//! test with a home of its own sees only the CLIs it put there: an environment a
//! test makes has no folders of the system's unless it names them. And which
//! harnesses those are: the ones that are installed here, and the ones that are
//! not.

use std::path::PathBuf;

use cf_base::env::Env;
use cf_proto::agents::Harness;
use serde::Serialize;

use crate::pi::paths::agent_dir;
use crate::shared::paths::{home, set};

/// Every harness, in the order Node's harness list had them: the order
/// detection answers in and the Harnesses page lists its rows in. It is not
/// [`Harness::ALL`]'s, the roster's.
const DETECTION_ORDER: [Harness; 6] = [
    Harness::Devin,
    Harness::Claude,
    Harness::Codex,
    Harness::Opencode,
    Harness::Pi,
    Harness::Muse,
];

/// A harness whose CLI is installed here, as `detectHarnesses` lists it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Detected {
    pub id: Harness,
    /// The command it is run by: its CLI's own name.
    pub command: &'static str,
}

/// The folders under the home a harness's CLI installs itself in.
fn locations(harness: Harness) -> &'static [&'static [&'static str]] {
    match harness {
        Harness::Devin => &[&[".local", "bin"]],
        // `.claude/local` is the old local installer's place, a legacy one: Claude
        // Code 2.1.305, the newest seen, still updates and diagnoses an install
        // there, but nothing installs there fresh (it installs to `.local/bin`).
        Harness::Claude => &[&[".local", "bin"], &[".claude", "local"]],
        Harness::Codex => &[&[".codex", "bin"], &[".local", "bin"]],
        Harness::Opencode => &[&[".opencode", "bin"], &[".local", "bin"]],
        Harness::Pi => &[&[".pi", "bin"], &[".local", "bin"]],
        // Muse's installer (`install.sh`): its launcher and the binaries it runs, in
        // `.local/bin` on macOS and Linux. On Windows it is elsewhere ([`windows_programs`]).
        Harness::Muse => &[&[".local", "bin"]],
    }
}

/// The folders under the home any of them might land in.
const COMMON: [&[&str]; 3] = [
    &[".bun", "bin"],
    &[".npm-global", "bin"],
    &[".volta", "bin"],
];

/// The folders under the home `harness`'s CLI may be in, its own places
/// first, then the common ones; none where the environment has no home.
fn homed(harness: Harness, env: &Env) -> Vec<PathBuf> {
    let Ok(home) = home(env) else {
        return Vec::new();
    };
    locations(harness)
        .iter()
        .chain(COMMON.iter())
        .map(|parts| {
            parts
                .iter()
                .fold(PathBuf::from(&home), |folder, part| folder.join(part))
        })
        .collect()
}

/// npm's global folder on Windows, `%APPDATA%\npm`: where `npm install -g` puts
/// the shims of a CLI, and which a terminal reaches through the shell's own
/// setup, as an app started from the Start menu does not. It comes from the
/// `APPDATA` of the environment given, never the machine's own; one that is
/// missing or empty adds nothing, and so does any other system.
fn npm_global(env: &Env) -> Option<PathBuf> {
    if !env.on_windows() {
        return None;
    }
    Some(PathBuf::from(set(env, "APPDATA")?).join("npm"))
}

/// The folder a harness's own installer puts its CLI in on Windows, outside the home,
/// which only Muse has: `%LOCALAPPDATA%\Programs\muse` (`install.ps1`), holding the
/// `muse.cmd` that starts its launcher. It comes from the `LOCALAPPDATA` of the
/// environment given; one that is missing or empty adds nothing, and so does any other
/// system and any other harness.
fn windows_programs(harness: Harness, env: &Env) -> Option<PathBuf> {
    if harness != Harness::Muse || !env.on_windows() {
        return None;
    }
    Some(
        PathBuf::from(set(env, "LOCALAPPDATA")?)
            .join("Programs")
            .join("muse"),
    )
}

/// Where Pi's own installers (`install.ps1`, `install.sh`) put the launcher of
/// a managed install, under the home: `bin` of the agent folder, which is
/// `~/.pi/agent` unless the human named another (`PI_CODING_AGENT_DIR`, [`managed`]).
/// On Windows it is the only place, and the installer puts it first on the
/// user's PATH, which an app started before that keeps the old one of;
/// elsewhere the installer also links it from the first folder of the PATH it
/// may write in, and the launcher is there all the same.
const MANAGED_PI: [&str; 3] = [".pi", "agent", "bin"];

/// The folders of `harness`'s managed install, which only Pi has: `bin` of the agent
/// folder as Pi reads it (`pi::paths::agent_dir`: `PI_CODING_AGENT_DIR`, which both
/// installers read too, an empty one being none, else `~/.pi/agent`; `~`, `~/` and on
/// Windows `~\` are the home), then of the default one under the home when that is
/// another (an install made before the variable was set is still there). A relative
/// folder is the home's: the installers read it against where they ran, which a
/// terminal opens in the home, and this daemon's own working folder is no place of
/// theirs: with no home there is nothing to read a relative folder against, and it is no place.
/// None where the environment names no folder and has no home.
fn managed(harness: Harness, env: &Env) -> Vec<PathBuf> {
    if harness != Harness::Pi {
        return Vec::new();
    }
    let home = home(env).ok().map(PathBuf::from);
    let named = agent_dir(env).ok().and_then(|agent| {
        let agent = PathBuf::from(agent);
        match &home {
            Some(home) if agent.is_relative() => Some(home.join(agent)),
            None if agent.is_relative() => None,
            _ => Some(agent),
        }
        .map(|folder| folder.join("bin"))
    });
    let default = home.map(|home| {
        MANAGED_PI
            .iter()
            .fold(home, |folder, part| folder.join(part))
    });
    let default = default.filter(|default| Some(default) != named.as_ref());
    named.into_iter().chain(default).collect()
}

/// The absolute path `harness`'s CLI resolves to here, or none: a pane opens
/// only on an absolute program. On PATH, else in the harness's own places,
/// else in the common ones, npm's folder on Windows the last of them, else in
/// its installer's folder on Windows (Muse's), else in the system's folders of
/// programs, else in the folder of a managed install.
/// That one is the last so that a place added to the others never changes which
/// CLI is found where one was: a Pi in npm's folder is still the Pi found when
/// a managed one is there too.
pub fn harness_path(harness: Harness, env: &Env) -> Option<PathBuf> {
    let command = harness.as_str();
    cf_process::on_search_path(command, env).or_else(|| {
        let folders = homed(harness, env).into_iter().chain(npm_global(env));
        let folders = folders.chain(windows_programs(harness, env));
        let folders = folders.chain(env.tool_folders().iter().cloned());
        let folders = folders.chain(managed(harness, env));
        cf_process::find_in(command, folders, env)
    })
}

/// All supported harnesses, whether installed or not (`knownHarnesses`).
pub fn known_harnesses() -> [Harness; 6] {
    DETECTION_ORDER
}

/// The supported harnesses whose CLI is not installed here
/// (`missingHarnesses`).
pub fn missing_harnesses(env: &Env) -> Vec<Harness> {
    DETECTION_ORDER
        .into_iter()
        .filter(|harness| harness_path(*harness, env).is_none())
        .collect()
}

/// The supported harnesses whose CLI is installed here, each with the
/// command it is run by (`detectHarnesses`).
pub fn detect_harnesses(env: &Env) -> Vec<Detected> {
    DETECTION_ORDER
        .into_iter()
        .filter(|harness| harness_path(*harness, env).is_some())
        .map(|harness| Detected {
            id: harness,
            command: harness.as_str(),
        })
        .collect()
}

/// The program a window of `harness` opens on: its CLI's absolute path here, or
/// the refusal that names it.
pub(crate) fn executable(harness: Harness, env: &Env) -> Result<String, String> {
    harness_path(harness, env)
        .map(|path| path.to_string_lossy().into_owned())
        .ok_or_else(|| format!("{} is not installed on this machine", harness.as_str()))
}

#[cfg(test)]
mod tests;
