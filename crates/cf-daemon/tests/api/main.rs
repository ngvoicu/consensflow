//! The agents' API held to what Node answers (`tests/goldens/daemon/FORMAT.md`,
//! `decision-36`): every trace Node recorded of it, replayed against the
//! daemon's own routes in the daemon's own server, each step's answer compared
//! as bytes (key order, absent against null), and the ledger left as Node left
//! it; and every run of `cf` Node's `cf-board.test.mjs` made against the API,
//! made again with the native `cf` built from this workspace, against this
//! one: its arguments, its input, its output and its exit as Node's recorded
//! them.
//!
//! - `core-api-*`, `core-daemon-*`: the suites that held the API in Node.
//! - `corners-api-*`: what no suite looked at (the order of the checks, how a
//!   target, a body and a number are read, `HEAD`, a window whose project is
//!   gone).
//! - `cf-board-*`: `cf` against the API.
//!
//! The ledger's calls the traces make are replayed here from step 3.1's replay
//! (`crates/cf-ledger/tests/replay.rs`): that is a copy, because a test of one
//! crate cannot be used by another's. The traces of the other two surfaces of
//! the daemon (the page operations, the screens) read the same files and need
//! the same.

// The player's own scaffolding: a failure in it is the test's, and so is what
// it says of how many traces it played. The tests start `cf` themselves, and
// read the environment it is to be run without.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::print_stdout,
    clippy::disallowed_methods
)]

mod checks;
mod dump;
mod player;
mod replay;
mod rig;
mod runs;
mod trace;
mod wire;

use std::panic::{catch_unwind, AssertUnwindSafe};

/// Plays every trace of `suites`, each against an API of its own: what each
/// that was not answered as Node answered it said.
fn play_all(suites: &[&str], expected: usize) {
    let names = trace::names(suites);
    assert_eq!(
        names.len(),
        expected,
        "the traces of {suites:?}: npm run goldens:daemon"
    );
    let mut failures = Vec::new();
    for name in &names {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("a runtime");
        let local = tokio::task::LocalSet::new();
        let played = catch_unwind(AssertUnwindSafe(|| {
            local.block_on(&runtime, player::play(name))
        }));
        match played {
            Ok(Ok(())) => {}
            Ok(Err(why)) => failures.push(why),
            Err(_) => failures.push(format!("{name}: the player panicked")),
        }
    }
    println!(
        "{} traces played, {} failed of {}",
        names.len() - failures.len(),
        failures.len(),
        names.len()
    );
    assert!(
        failures.is_empty(),
        "{} traces were not answered as Node answered them:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn every_trace_of_the_api_is_answered_as_node_answered_it() {
    play_all(&["core-api", "core-daemon", "corners-api"], 18);
}

#[test]
fn every_run_of_cf_against_the_api_prints_what_it_printed_against_node() {
    play_all(&["cf-board"], 29);
}
