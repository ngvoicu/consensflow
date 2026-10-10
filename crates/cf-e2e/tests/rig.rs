//! The rig's black-box suites: the native daemon and the real pane host, run as
//! the app runs them and connected by their production JSON-lines pipes, with a
//! stand-in `claude` (the `fake-agent` binary) in the windows they open. The
//! cases ask the daemon what the page asks and the pane host what the desktop
//! app asks, type to a chief as a human does, and judge what comes of it: the
//! board, the inboxes, the windows' own records, the daemon's log and event
//! file, the processes left behind. They are ported one for one from the
//! JavaScript suites `tests/integration/*.test.mjs` (gone with the port); the
//! module of a case is the JavaScript file it came from, and its name the
//! JavaScript test's. (`stand_ins` is new: the stand-in Codex's own transport
//! on Windows, which the supervisor's cases here do not reach. So is
//! `live_support`: what the opt-in live tests stand on.)
//!
//! The cases take turns ([`cf_e2e::serial`]): each starts a daemon and a pane
//! host of its own, with windows in real terminals, and the waits are timed.
//!
//!     cargo test -p cf-e2e --test rig        (`npm run test:integration`)

// The cases are in `tests/rig/`: cargo looks for the modules of a test's root
// file beside it, where each would be a suite of its own.
#[path = "rig/cf_native.rs"]
mod cf_native;
#[path = "rig/codex_session.rs"]
mod codex_session;
#[path = "rig/core_board.rs"]
mod core_board;
#[path = "rig/core_questions.rs"]
mod core_questions;
#[path = "rig/core_slice.rs"]
mod core_slice;
#[path = "rig/core_tells.rs"]
mod core_tells;
#[path = "rig/core_tiered.rs"]
mod core_tiered;
#[path = "rig/daemon_seam.rs"]
mod daemon_seam;
#[path = "rig/decided_result.rs"]
mod decided_result;
#[path = "rig/help.rs"]
mod help;
#[path = "rig/live_support.rs"]
mod live_support;
#[path = "rig/stand_ins.rs"]
mod stand_ins;
#[path = "rig/waiting_notes.rs"]
mod waiting_notes;
#[path = "rig/window_says_why.rs"]
mod window_says_why;

use std::time::Duration;

use cf_e2e::rig::{Config, Rig};

/// What a case ends with: nothing, or why it could not be run. A check that
/// does not hold is a panic, which is the case's failure.
type Outcome = Result<(), Box<dyn std::error::Error>>;

/// The stand-in for a window's harness program, built for these tests.
const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");

/// A rig whose windows run the stand-in agent.
fn rig() -> cf_e2e::Result<Rig> {
    Rig::start(Config::new(FAKE_AGENT))
}

/// The configuration of a rig whose windows run the stand-in agent, to add
/// variables to.
fn config() -> Config {
    Config::new(FAKE_AGENT)
}

/// `n` seconds.
fn secs(n: u64) -> Duration {
    Duration::from_secs(n)
}

/// The conversation a window was launched with: the word after `--session-id`
/// in its command line.
fn session_of(frame: &serde_json::Value) -> String {
    after(frame, "--session-id")
}

/// The word that follows `flag` in the command line of the window `frame`
/// opens.
fn after(frame: &serde_json::Value, flag: &str) -> String {
    let argv = frame["argv"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    argv.iter()
        .position(|word| word == flag)
        .and_then(|at| argv.get(at + 1))
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_owned()
}
