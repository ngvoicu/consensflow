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

use crate::shared::paths::{home, set};

/// Every harness, in the order Node's harness list had them: the order
/// detection answers in and the Harnesses page lists its rows in. It is not
/// [`Harness::ALL`]'s, the roster's.
const DETECTION_ORDER: [Harness; 5] = [
    Harness::Devin,
    Harness::Claude,
    Harness::Codex,
    Harness::Opencode,
    Harness::Pi,
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

/// Where Pi's own installers (`install.ps1`, `install.sh`) put the launcher of
/// a managed install, under the home: `bin` of the agent folder, which is
/// `~/.pi/agent` unless the human named another (`PI_CODING_AGENT_DIR`, [`managed`]).
/// On Windows it is the only place, and the installer puts it first on the
/// user's PATH, which an app started before that keeps the old one of;
/// elsewhere the installer also links it from the first folder of the PATH it
/// may write in, and the launcher is there all the same.
const MANAGED_PI: [&str; 3] = [".pi", "agent", "bin"];

/// The variable both installers read for the agent folder: `install.ps1` when it
/// holds something, `install.sh` as `${PI_CODING_AGENT_DIR:-$HOME/.pi/agent}`, an
/// empty one being none in both.
const AGENT_DIR: &str = "PI_CODING_AGENT_DIR";

/// The folders of `harness`'s managed install, which only Pi has: `bin` of the
/// agent folder the human named, then of the default one under the home (an install
/// made before the variable was set is still there). A relative folder is read
/// against the working folder, where the installers read it against theirs; none
/// where the environment names no folder and has no home.
fn managed(harness: Harness, env: &Env) -> Vec<PathBuf> {
    if harness != Harness::Pi {
        return Vec::new();
    }
    let named = set(env, AGENT_DIR).map(|agent| PathBuf::from(agent).join("bin"));
    let default = home(env).ok().map(|home| {
        MANAGED_PI
            .iter()
            .fold(PathBuf::from(home), |folder, part| folder.join(part))
    });
    named.into_iter().chain(default).collect()
}

/// The absolute path `harness`'s CLI resolves to here, or none: a pane opens
/// only on an absolute program. On PATH, else in the harness's own places,
/// else in the common ones, npm's folder on Windows the last of them, else in
/// the system's folders of programs, else in the folder of a managed install.
/// That one is the last so that a place added to the others never changes which
/// CLI is found where one was: a Pi in npm's folder is still the Pi found when
/// a managed one is there too.
pub fn harness_path(harness: Harness, env: &Env) -> Option<PathBuf> {
    let command = harness.as_str();
    cf_process::on_search_path(command, env).or_else(|| {
        let folders = homed(harness, env).into_iter().chain(npm_global(env));
        let folders = folders.chain(env.tool_folders().iter().cloned());
        let folders = folders.chain(managed(harness, env));
        cf_process::find_in(command, folders, env)
    })
}

/// All supported harnesses, whether installed or not (`knownHarnesses`).
pub fn known_harnesses() -> [Harness; 5] {
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
