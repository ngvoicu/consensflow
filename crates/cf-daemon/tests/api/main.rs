//! The agents' API held to what Node answers (`tests/goldens/daemon/FORMAT.md`,
//! `decision-36`): every trace Node recorded of it, replayed against the
//! daemon's own dispatch (`api::handle`, the screens mounted in front of the
//! routes) in the daemon's own server, each step's answer compared as bytes
//! (key order, absent against null), and the ledger left as Node left it; and
//! every run of `cf` Node's `cf-board.test.mjs` made against the API, made
//! again with the native `cf` built from this workspace, against this one: its
//! arguments, its input, its output and its exit as Node's recorded them, and
//! every request it wrote, whole, and no other, and each answer it was given as
//! the bytes it got: its status, its type and its body.
//!
//! - `core-api-*`, `core-daemon-*`: the suites that held the API in Node.
//! - `corners-api-*`: what no suite looked at (the order of the checks, how a
//!   target, a body and a number are read, `HEAD`, a window whose project is
//!   gone).
//! - `cf-board-*`: `cf` against the API.
//!
//! The traces are read, the ledger's calls made again, the database compared
//! and the front reached as the other two surfaces of the daemon (the page
//! operations, the screens) do: that is `tests/support/`, shared.

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
mod frames;
mod names;
mod player;
mod relay;
mod rig;
mod runs;

// What the three players share, taken whole.
#[path = "../support/front.rs"]
mod front;
#[path = "../support/ledger.rs"]
mod ledger;
#[path = "../support/mod.rs"]
mod support;

use std::collections::BTreeSet;
use std::panic::{catch_unwind, AssertUnwindSafe};

use cf_daemon::api::body::Body;
use cf_daemon::api::request::Request;
use cf_daemon::screens::recognize;
use hyper::Method;
use support::trace::{self, Tally};

/// The suites of the API's traces, and of `cf` against it.
const SUITES: [&str; 4] = ["core-api", "core-daemon", "corners-api", "cf-board"];

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
    let mut held = Tally::default();
    for name in &names {
        let played = catch_unwind(AssertUnwindSafe(|| trace::locally(player::play(name))));
        match played {
            Ok(Ok(tally)) => held += tally,
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
    println!("held: {held}");
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

/// Every exchange of the traces whose path the screens answer, which they do
/// before the API's own checks: those are the ones held to the screens' answer.
#[test]
fn the_exchanges_that_a_screen_answers_are_exactly_the_departures() {
    let mut screens = BTreeSet::new();
    for name in trace::names(&SUITES) {
        for step in trace::load(&name)["steps"].as_array().unwrap() {
            if step["kind"] != "exchange" {
                continue;
            }
            let request = &step["request"];
            let method = Method::from_bytes(request["method"].as_str().unwrap().as_bytes());
            let target = request["target"].as_str().unwrap();
            // A target the standard does not read fails the request before any route.
            let Ok(request) = Request::new(method.unwrap(), target, None, Body::empty()) else {
                continue;
            };
            if recognize(&request.path).is_some() {
                screens.insert((name.clone(), step["id"].as_u64().unwrap()));
            }
        }
    }
    let departures: BTreeSet<(String, u64)> = player::DEPARTURES
        .iter()
        .map(|(name, id)| ((*name).to_owned(), *id))
        .collect();
    assert_eq!(screens, departures);
}

/// The screens open to their token alone, which no request of a trace carries.
#[test]
fn no_trace_of_the_api_carries_the_token_of_the_screens_it_is_mounted_under() {
    for name in trace::names(&SUITES) {
        let text = trace::load(&name).to_string();
        assert!(!text.contains(rig::UI_TOKEN), "{name}");
    }
}
