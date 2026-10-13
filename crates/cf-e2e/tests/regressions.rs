//! The regressions: one case for each bug the owner hit in real use, reproduced
//! as close to its real shape as the stand-ins allow, on the rig (the native
//! daemon and the real pane host, with stand-in agents in real terminals), and
//! asserting what the owner should have seen.
//!
//! The mechanism tests (the rig's, and each crate's own) say how a fix works. These
//! say what the owner saw, so a fix that passes those and fails these has missed the
//! bug. Each case is shown to fail on the version it was seen on, by the plant that
//! removes the fix (`tests/plants/daemon/`); where the cause was in the owner's
//! machine alone, its doc says so and the case is the closest shape of it.
//!
//! A bug found in real use is added here the same way, as a module of its own in
//! `regressions/` named for the report it answers, whose doc says:
//!
//! - the date and the version the owner saw it on;
//! - what the owner saw;
//! - the issue in the brain (consensflow-sme) that holds the report;
//!
//! and whose cases each assert what the owner should have seen. The cases take
//! turns, as the rig's do ([`cf_e2e::serial`]), and run wherever the rig's do; a
//! case whose bug Windows cannot have is Unix's.
//!
//!     cargo test -p cf-e2e --test regressions        (`npm run test:regressions`)

// The cases are in `tests/regressions/`: cargo looks for the modules of a test's
// root file beside it, where each would be a suite of its own.
#[path = "regressions/a_command_that_never_exits_held_a_workers_result.rs"]
mod a_command_that_never_exits_held_a_workers_result;
#[path = "regressions/devin_pause.rs"]
mod devin_pause;
// Windows keeps the limit the report documents (a job object has no process
// groups or parents to look at), so there is nothing there to assert.
#[cfg(unix)]
#[path = "regressions/mcp_servers_held_a_workers_result.rs"]
mod mcp_servers_held_a_workers_result;

use std::time::Duration;

use cf_e2e::rig::Config;

/// What a case ends with: nothing, or why it could not be run. A check that does
/// not hold is a panic, which is the case's failure.
type Outcome = Result<(), Box<dyn std::error::Error>>;

/// The stand-in for a window's harness program, built for these tests.
const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");

/// The stand-in for Devin, which the windows of Devin's agents run.
const FAKE_DEVIN: &str = env!("CARGO_BIN_EXE_fake-devin");

/// The stand-in for Codex, which the windows of Codex's agents run.
#[cfg(unix)]
const FAKE_CODEX: &str = env!("CARGO_BIN_EXE_fake-codex");

/// The configuration of a rig whose windows run the stand-in agent, to add
/// variables to.
fn config() -> Config {
    Config::new(FAKE_AGENT)
}

/// [`config`], and the stand-in Codex for the windows of Codex's agents.
#[cfg(unix)]
fn config_with_codex() -> Config {
    config().codex(FAKE_CODEX)
}

/// `n` seconds.
fn secs(n: u64) -> Duration {
    Duration::from_secs(n)
}
