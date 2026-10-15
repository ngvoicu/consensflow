//! Which OpenCode ConsensFlow opens windows on, judged by what it says to
//! `--version`: the 1.x its launch was made for, and not yet 2. OpenCode 2
//! replaced what a window is launched with (`--port`, `--hostname` and `--model`
//! are gone, and a shared `opencode service` stands where each window's server
//! was): every window opened on it was seen to wait for ever, and none read
//! idle. It is refused before anything is made, in words that say what to do.
//!
//! Only a version that is read to be 2 or more is refused. An OpenCode that
//! cannot be asked, or says no version in what it prints, is let through as it
//! always was: the refusal is for the one case known to hang, and a guess would
//! shut out a 1.x that works.

use std::path::Path;
use std::rc::Rc;
use std::sync::LazyLock;

use cf_base::env::Env;
use regex::Regex;

use crate::seams::processes::{probe, Probes, Processes};
use crate::shared::pattern::compile;

/// The first major version of OpenCode that no window is opened on yet.
const FIRST_UNSUPPORTED: u64 = 2;

/// The major version in what OpenCode says: the first three dotted numbers
/// that follow the start of the text, white space or a `v`, as 1.x prints it
/// (`1.19.0`) and 2 does (`opencode v2.0.29`). The `v` is no word boundary
/// from the number after it, so a pattern that asks for one would read 2 as no
/// version at all.
static VERSION: LazyLock<Regex> = LazyLock::new(|| compile(r"(?:^|\s|v)([0-9]+)\.[0-9]+\.[0-9]+"));

/// Whether the OpenCode that said `output` to `--version`, or whose version
/// alone it is, is one ConsensFlow does not support yet: its major version is
/// 2 or more, a number too long for one as much as any. What names no version
/// of three numbers is no such OpenCode.
pub(crate) fn unsupported(output: &str) -> bool {
    VERSION.captures(output).is_some_and(|found| {
        !found[1]
            .parse::<u64>()
            .is_ok_and(|major| major < FIRST_UNSUPPORTED)
    })
}

/// What a launch is refused with, and the Harnesses screen says, of an
/// OpenCode that is one. It has no full stop at its end, as a refusal has none
/// in this crate: the board goes on after it with sentences of its own.
pub(crate) fn not_supported() -> String {
    format!(
        "OpenCode {FIRST_UNSUPPORTED} or newer is not supported yet. Install the last OpenCode 1.x release"
    )
}

/// Asks the OpenCode at `executable` its version, once as it is on disk
/// ([`probe`]), and refuses one that is not supported yet. One that did not
/// answer, or answered no version, goes on.
pub(super) async fn check(
    probes: &Probes,
    processes: &Rc<dyn Processes>,
    env: &Env,
    executable: &str,
) -> Result<(), String> {
    match probe(
        probes,
        processes,
        Path::new(executable),
        &["--version"],
        env,
    )
    .await
    {
        Ok(answered) if unsupported(&answered.stdout) => Err(not_supported()),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests;
