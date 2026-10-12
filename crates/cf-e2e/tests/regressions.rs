//! The regressions: one rig test for each bug the owner met in the app. The
//! daemon and the pane host are the real ones, run as the app runs them, with the
//! stand-ins of the harnesses in the windows they open ([`cf_e2e::rig`]), and each
//! test is the bug in the shape the owner met it. The doc of a test says when it
//! was met and in which version, what the owner saw, and what should have
//! happened. A test of a bug that was in code must fail on the code that had it,
//! which a plant of the daemon's plants shows (`tests/plants/daemon/devin-stop.toml`
//! is the first); where the cause was in the owner's machine alone, its doc says
//! so and the test is the closest shape of it.
//!
//! The cases take turns, as the rig's do ([`cf_e2e::serial`]), and run wherever the
//! rig's do (`cargo xtask test integration`).
//!
//!     cargo test -p cf-e2e --test regressions

// The cases are in `regressions/`: cargo looks for the modules of a test's root
// file beside it, where each would be a suite of its own.
#[path = "regressions/devin_pause.rs"]
mod devin_pause;

use std::time::Duration;

/// What a case ends with: nothing, or why it could not be run. A check that does
/// not hold is a panic, which is the case's failure.
type Outcome = Result<(), Box<dyn std::error::Error>>;

/// The stand-in for the window of a Claude agent, and of a Devin agent.
const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");
const FAKE_DEVIN: &str = env!("CARGO_BIN_EXE_fake-devin");

/// `n` seconds.
fn secs(n: u64) -> Duration {
    Duration::from_secs(n)
}
