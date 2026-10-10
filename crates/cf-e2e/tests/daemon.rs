//! The daemon's black-box suite: `cf ui --json --no-open` run as a process, as
//! the app starts it, on a home of each case's own, and judged by what it says
//! (its handle line, its bridge's frames, its API, its log) and by how it
//! ends. The cases are ported one for one from the JavaScript suites
//! `tests/core-daemon.test.mjs`, `tests/bridge.test.mjs` and
//! `tests/agents-proof.test.mjs` (gone with the port); the module of a case is
//! the JavaScript file it came from, and its name the JavaScript test's.
//!
//! The cases take turns ([`cf_e2e::serial`]): the daemon is timed (it must stop
//! in ten seconds) and each case starts one.
//!
//!     cargo test -p cf-e2e --test daemon        (`npm run test:daemons`, with the rig's)
//!     cargo test -p cf-e2e --test daemon agents_proof::   (`npm run test:agents`)

// The cases are in `tests/daemon/`: cargo looks for the modules of a test's root
// file beside it, where each would be a suite of its own.
#[path = "daemon/agents_proof.rs"]
mod agents_proof;
#[path = "daemon/bridge.rs"]
mod bridge;
#[path = "daemon/core_daemon.rs"]
mod core_daemon;
#[path = "daemon/over_bridge.rs"]
mod over_bridge;

use std::time::Duration;

/// What a case ends with: nothing, or why it could not be run. A check that
/// does not hold is a panic, which is the case's failure.
type Outcome = Result<(), Box<dyn std::error::Error>>;

/// `n` seconds.
fn secs(n: u64) -> Duration {
    Duration::from_secs(n)
}
