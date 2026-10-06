//! The standalone verbs of the CLI (`bin/cf.mjs`), answered here as Node
//! answered them, word for word: the usage, the version, `catalog`,
//! `agent add|list|edit|remove`, and `setup` and `doctor`, which wire the
//! launcher, the stale hooks and the app's preparation
//! (`cf_launcher`, `cf_harness`). `tests/cli_goldens` holds them to what Node
//! said (`npm run goldens:cli`), file by file.
//!
//! They are what `cf` answers by default (the flip, step 4), for a tokenless
//! command (a window has its participant's token, and there `cf` is the
//! board) in a home that has not taken the way back: with the `use-node` file
//! in it every tokenless command goes to the CLI's Node sources instead
//! (`crate::run` asks `cf_base::way_back`, once). `ui` is the one verb that is
//! not answered here: it is the daemon's.
//!
//! A verb says what it prints as it goes, and what stops it as `cf: <words>`
//! with exit code 1: Node's `fail` and every error `main` caught.

mod agent;
mod catalog;
mod doctor;
mod setup;

use std::ffi::OsString;
use std::io::{self, Write};
use std::path::PathBuf;

use cf_base::env::Env;
use cf_base::home::config_root;
use cf_base::js;
use cf_base::refusal::Refusal;
use cf_catalog::{roster_path, Catalog, Roster};
use cf_daemon::machine;
use cf_harness::detect::detect_harnesses;
use serde::Serialize;
use serde_json::Value;

/// Said when the environment names no folder to keep ConsensFlow's things in.
/// Node asked the system for the user's home then; here it is not asked for.
const NO_HOME: &str =
    "ConsensFlow has no folder to keep its things in: set CONSENSFLOW_HOME, or HOME";

/// What `cf help` prints, with the version where `{version}` stands.
const USAGE: &str = include_str!("usage.txt");

/// What stops a verb: words it says, or the output it could not write.
enum Stop {
    /// `cf: <words>`, and exit code 1.
    Said(String),
    /// The reader of the output went away, or the output failed.
    Io(io::Error),
}

impl From<io::Error> for Stop {
    fn from(failed: io::Error) -> Self {
        Self::Io(failed)
    }
}

impl From<Refusal> for Stop {
    fn from(refusal: Refusal) -> Self {
        Self::Said(refusal.message)
    }
}

/// How a verb ended.
type Done = Result<(), Stop>;

/// Runs `args` as a standalone verb when it is one answered here: its exit
/// code. None when it is not for this module: the verb is `ui`, which
/// `cf_daemon` answers. The caller has found no window's token and no way back
/// to Node. Only a failure to write is an error.
pub fn run(
    env: &Env,
    args: &[OsString],
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> io::Result<Option<u8>> {
    // Node reads its arguments as UTF-8, and a byte that is none as U+FFFD.
    let words: Vec<String> = args
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    let (command, rest) = words
        .split_first()
        .map_or((None, &[][..]), |(command, rest)| {
            (Some(command.as_str()), rest)
        });
    let done = match command {
        None | Some("help" | "--help") => writeln!(out, "{}", usage()).map_err(Stop::from),
        Some("--version" | "-v" | "version") => {
            writeln!(out, "{}", env!("CARGO_PKG_VERSION")).map_err(Stop::from)
        }
        Some("catalog") => catalog::run(rest, out),
        Some("agent") => agent::run(env, rest, out),
        Some("setup") => setup::run(env, rest, out),
        // Whatever words follow it are no matter, as in Node.
        Some("doctor") => doctor::run(env, out),
        Some("ui") => return Ok(None),
        Some(other) => Err(Stop::Said(format!(
            "unknown command {} — run `cf help`",
            js::stringify(&Value::from(other))
        ))),
    };
    match done {
        Ok(()) => Ok(Some(0)),
        Err(Stop::Said(words)) => {
            writeln!(err, "cf: {words}")?;
            Ok(Some(1))
        }
        Err(Stop::Io(failed)) => Err(failed),
    }
}

/// The usage, as `USAGE` of `bin/cf.mjs` reads: it ends with a line break of
/// its own, and `cf help` adds another.
fn usage() -> String {
    USAGE.replace("{version}", env!("CARGO_PKG_VERSION"))
}

/// The catalog this build ships, which a verb lists or names agents by.
fn bundled() -> Result<Catalog, Stop> {
    Catalog::bundled().map_err(|failed| Stop::Said(failed.to_string()))
}

/// `value` as JSON, for the verb that prints it.
fn to_json<T: Serialize>(value: &T) -> Result<Value, Stop> {
    serde_json::to_value(value).map_err(|failed| Stop::Said(failed.to_string()))
}

/// What stops a verb that needs ConsensFlow's folder where none is named.
fn no_home() -> Stop {
    Stop::Said(NO_HOME.to_owned())
}

/// ConsensFlow's folder, which the verbs that keep something in it refuse to
/// go on without.
fn home(env: &Env) -> Result<PathBuf, Stop> {
    config_root(env).ok_or_else(no_home)
}

/// The roster of the home `env` names, over `catalog`.
fn roster<'a>(env: &Env, catalog: &'a Catalog) -> Result<Roster<'a>, Stop> {
    let path = roster_path(env).ok_or_else(no_home)?;
    Ok(Roster::new(catalog, path))
}

/// How many agents there are, the catalog's and the human's together: what
/// `setup` and `doctor` count (`listAgents(env).length`).
fn agents_saved(env: &Env) -> Result<usize, Stop> {
    let catalog = bundled()?;
    Ok(roster(env, &catalog)?.list()?.len())
}

/// The harnesses installed here, by id, in the order detection lists them.
fn harness_ids(env: &Env) -> Vec<&'static str> {
    detect_harnesses(env)
        .iter()
        .map(|found| found.id.as_str())
        .collect()
}

/// The `cf` of the bundle this program is in: the one a launcher runs, and
/// the one `doctor` asks a launcher about.
fn own_cf() -> Result<PathBuf, Stop> {
    match std::env::current_exe() {
        Ok(exe) => Ok(machine::bundle_of(&exe).cf),
        Err(cause) => Err(Stop::Said(format!(
            "cannot tell which folder it is in: {cause}"
        ))),
    }
}

#[cfg(test)]
mod tests;
