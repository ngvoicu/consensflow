//! Where each harness's CLI is on this machine (`harnessPath`,
//! `src/harnesses.js`): on PATH first, then in the places each installs
//! itself, which a Finder-launched app's PATH may lack. Every place is the
//! user's own: a system-wide one (`/opt/homebrew/bin`) is on every login
//! PATH already, and a test with a home of its own then sees only the CLIs
//! it put there. And which harnesses those are: the ones that are
//! installed here, and the ones that are not.

use std::path::PathBuf;

use cf_base::env::Env;
use cf_proto::agents::Harness;
use serde::Serialize;

use crate::shared::paths::home;

/// Every harness, in the order `src/harnesses.js` lists them (`HARNESSES`):
/// the order detection answers in and the Harnesses page lists its rows in.
/// It is not [`Harness::ALL`]'s, the roster's.
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

/// The absolute path `harness`'s CLI resolves to here, or none: a pane opens
/// only on an absolute program.
pub fn harness_path(harness: Harness, env: &Env) -> Option<PathBuf> {
    let command = harness.as_str();
    cf_process::on_path(command, env).or_else(|| {
        let home = PathBuf::from(home(env).ok()?);
        let folders = locations(harness).iter().chain(COMMON.iter()).map(|parts| {
            parts
                .iter()
                .fold(home.clone(), |folder, part| folder.join(part))
        });
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

/// The program a window of `harness` opens on (`executableFor`,
/// `src/adapters/shared.js`): its CLI's absolute path here, or the refusal
/// that names it.
pub(crate) fn executable(harness: Harness, env: &Env) -> Result<String, String> {
    harness_path(harness, env)
        .map(|path| path.to_string_lossy().into_owned())
        .ok_or_else(|| format!("{} is not installed on this machine", harness.as_str()))
}

#[cfg(test)]
mod tests;
