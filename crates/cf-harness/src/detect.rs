//! Where each harness's CLI is on this machine: on PATH first, then in the
//! places each installs itself, which a Finder-launched app's PATH may lack.
//! Every place is the user's own, under the home but for npm's folder on
//! Windows, which the environment names: a system-wide one
//! (`/opt/homebrew/bin`) is on every login PATH already, and a test with a home
//! of its own then sees only the CLIs it put there. And which harnesses those
//! are: the ones that are installed here, and the ones that are not.

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

/// The absolute path `harness`'s CLI resolves to here, or none: a pane opens
/// only on an absolute program. On PATH, else in the harness's own places,
/// else in the common ones, npm's folder on Windows the last of them.
pub fn harness_path(harness: Harness, env: &Env) -> Option<PathBuf> {
    let command = harness.as_str();
    cf_process::on_path(command, env).or_else(|| {
        let folders = homed(harness, env).into_iter().chain(npm_global(env));
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
