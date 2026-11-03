//! The shim: the program `cf.exe` is in the user's terminal on Windows
//! (`cf-shim`, which `install` and `repair` copy to `cf.exe` and
//! `consensflow.exe` in the home's `bin`; see [`crate::program`]).
//!
//! It does one thing. It reads the launch file beside it, `cf.launch` for
//! `cf.exe`, and runs the `cf` that names, with the arguments it was started
//! with, in the console it was started in, and ends with the code that `cf`
//! ends with. The file also pins a home, which the `cf` it starts is told by
//! `CONSENSFLOW_HOME`, and not the shim's own environment otherwise.
//!
//! Arguments are the point. Windows hands a program one command line, and the
//! program splits it; `std` splits it as the C runtime does and quotes what it
//! starts by the same rules, so an argument goes through as the shell gave it,
//! whatever it holds: line breaks, quotes, `%`, `&`, a trailing backslash. A
//! `.cmd` in this place cut an argument at its first line break.
//!
//! The Ctrl+C a terminal sends is sent to both processes, which share its
//! console; the `cf` that has something to do about it does it, and the shim,
//! which has nothing to do, ends with the rest.

use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::Command;

use crate::places::launch_of;
use crate::text::{pinned_home, runs, Runs};

/// What a launch file says to run.
#[derive(Debug, PartialEq, Eq)]
struct Plan {
    /// The `cf` to start, as the file spells it.
    cf: String,
    /// The home to tell it by `CONSENSFLOW_HOME`, when the file pins one.
    home: Option<String>,
}

/// What the launch file in `text` says to run: the native `cf` of a bundle. The
/// old shape names a runtime and a `cf.mjs`, which the app has not carried since
/// the native `cf`, and which only the app's repair can mend.
fn plan(text: &str) -> Result<Plan, String> {
    match runs(text, true) {
        Some(Runs::Native { cf }) => Ok(Plan {
            cf,
            home: pinned_home(text, true),
        }),
        Some(Runs::Node { .. }) => Err(
            "it names a runtime of an older ConsensFlow; open the app, or run `cf setup` from it"
                .to_owned(),
        ),
        None => Err("it names no cf to run; run `cf setup` from the app".to_owned()),
    }
}

/// Whether `cf` is the program at `exe` itself: a launch file that named its
/// own program would start it again, and again.
fn names_itself(cf: &str, exe: &Path) -> bool {
    match (fs::canonicalize(cf), fs::canonicalize(exe)) {
        (Ok(named), Ok(own)) => named == own,
        _ => false,
    }
}

/// Runs the `cf` that the launch file beside the program at `exe` names, with
/// `args`, and the code it ends with.
// The one program that starts another with `std` alone, on purpose: cf-process
// brings an async runtime the user's terminal would load for every `cf` it
// starts, and this program is the whole of what runs before it.
#[allow(clippy::disallowed_methods)]
fn start(exe: &Path, args: &[OsString]) -> Result<i32, String> {
    let launch = launch_of(exe);
    let held = fs::read(&launch).map_err(|failed| {
        format!(
            "{}: {failed}; `cf setup` from the app makes it again",
            launch.display()
        )
    })?;
    let Plan { cf, home } = plan(&String::from_utf8_lossy(&held))
        .map_err(|why| format!("{}: {why}", launch.display()))?;
    if names_itself(&cf, exe) {
        return Err(format!(
            "{} names this program itself; `cf setup` from the app makes it again",
            launch.display()
        ));
    }
    let mut command = Command::new(&cf);
    command.args(args);
    if let Some(home) = home {
        command.env("CONSENSFLOW_HOME", home);
    }
    let status = command.status().map_err(|failed| {
        format!("cannot run {cf}: {failed}; open the app, or run `cf setup` from it")
    })?;
    // A `cf` that ended with no code was ended by a signal: not a success.
    Ok(status.code().unwrap_or(1))
}

/// What `cf-shim` does: runs the `cf` its launch file names with the arguments
/// it was given, and ends as that `cf` ends. A launch file it cannot read, or a
/// `cf` it cannot start, is said on standard error and ends with 1.
pub fn run() -> i32 {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let started = std::env::current_exe()
        .map_err(|failed| format!("cannot tell which program this is: {failed}"))
        .and_then(|exe| start(&exe, &args));
    match started {
        Ok(code) => code,
        Err(why) => {
            let _ = writeln!(std::io::stderr(), "cf: {why}");
            1
        }
    }
}

#[cfg(test)]
mod tests;
