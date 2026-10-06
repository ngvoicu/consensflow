//! The standalone verbs of the CLI (`bin/cf.mjs`) that need no launcher, answered
//! here as Node answered them, word for word: the usage, the version,
//! `catalog`, and `agent add|list|edit|remove`. `tests/cli_goldens.rs` holds
//! them to what Node said (`npm run goldens:cli`), file by file.
//!
//! They are dormant until the flip (step 4): reached only with
//! `CONSENSFLOW_DAEMON=native`, as `cf ui` is, and tokenless (a window has its
//! participant's token, and there `cf` is the board). Without the switch every
//! tokenless verb goes to the CLI's Node sources as it always did, and so do
//! `setup` and `doctor` with it, until a landing brings the launcher and the
//! stale hooks they need.
//!
//! A verb says what it prints as it goes, and what stops it as `cf: <words>`
//! with exit code 1: Node's `fail` and every error `main` caught.

mod agent;
mod catalog;

use std::ffi::OsString;
use std::io::{self, Write};

use cf_base::env::Env;
use cf_base::js;
use cf_base::refusal::Refusal;
use cf_catalog::Catalog;
use serde::Serialize;
use serde_json::Value;

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

/// Runs `args` as a standalone verb when it is one answered here, and the
/// switch is on: its exit code. None when it is not for this module: the
/// switch is off, a window's token is there, or the verb is one of those still
/// answered by the CLI's Node sources (`setup`, `doctor`, and `ui`, which
/// `cf_daemon` answers). Only a failure to write is an error.
pub fn run(
    env: &Env,
    args: &[OsString],
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> io::Result<Option<u8>> {
    if env.text("CONSENSFLOW_DAEMON") != Some("native") || env.text("CONSENSFLOW_TOKEN").is_some() {
        return Ok(None);
    }
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
        Some("setup" | "doctor" | "ui") => return Ok(None),
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

#[cfg(test)]
mod tests;
