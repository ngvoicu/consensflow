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

// What the players share, taken whole.
#[path = "../support/departed.rs"]
mod departed;
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
use departed::Departed;
use hyper::Method;
use support::trace::{self, Tally};

/// The suites of the API's traces, and of `cf` against it.
const SUITES: [&str; 4] = ["core-api", "core-daemon", "corners-api", "cf-board"];

/// A result the chief had not been given when it accepted the task is
/// withdrawn, never pasted after the decision it asks for.
const RESULT_ACCEPTED: &str = "the chief accepts the task while its result is still queued for it: the result is withdrawn (`cancelled`, with the reason `T-n was accepted`), where Node leaves it `queued` to be pasted after the decision";

/// The same when the chief sent the task back.
const RESULT_SENT_BACK: &str = "the chief sends the task back while its result is still queued for it: the result is withdrawn (`cancelled`, with the reason `T-n was sent back`), where Node leaves it `queued` to be pasted after the decision";

/// The traces the receipt and stop redesign moved on purpose: a choice answer
/// is received, not read at its creation, a resume takes in what its window
/// kept, a command written wrong asks the board nothing, and a brief that
/// waits at the gate is not given to an agent; and those the decision on a
/// result moved: the result is withdrawn, not pasted after it. Found by
/// playing them against the daemon.
const DEPARTED: &[Departed] = &[
    ("core-api-003", RESULT_ACCEPTED),
    (
        "core-api-006",
        "the answer to a question with options lands `queued`: it is read when received, not when written",
    ),
    ("corners-api-008", RESULT_SENT_BACK),
    (
        "cf-board-001",
        "`cf task get T-1 --transcript --last 0` says its usage failure before it asks the board anything: Node asked for the task first, and the board took the answers in its thread as read, though nothing of them was printed",
    ),
    ("cf-board-002", RESULT_ACCEPTED),
    (
        "cf-board-003",
        "a resume carries the brief that never arrived and logs `message.carried`: one more clock reading, so the task's `updatedAt` is a second later",
    ),
    (
        "cf-board-010",
        "the task an agent reads while its brief waits at the gate has no `body`: the brief is for its window once the human passes it on",
    ),
    (
        "cf-board-023",
        "the answer to a question with options lands `queued`: it is read when received, not when written",
    ),
    (
        "cf-board-024",
        "the answer to a question with options lands `queued`: it is read when received, not when written",
    ),
    ("cf-board-027", RESULT_ACCEPTED),
];

/// Plays every trace of `suites` but the departed, each against an API of its
/// own: what each that was not answered as Node answered it said.
fn play_all(suites: &[&str], expected: usize) {
    let names = trace::names(suites);
    assert_eq!(
        names.len(),
        expected,
        "the traces of {suites:?}: npm run goldens:daemon"
    );
    let to_hold = departed::held(&names, DEPARTED);
    let mut failures = Vec::new();
    let mut held = Tally::default();
    for name in &to_hold {
        let played = catch_unwind(AssertUnwindSafe(|| trace::locally(player::play(name))));
        match played {
            Ok(Ok(tally)) => held += tally,
            Ok(Err(why)) => failures.push(why),
            Err(_) => failures.push(format!("{name}: the player panicked")),
        }
    }
    println!(
        "{} traces played, {} departed, {} failed of {}",
        to_hold.len() - failures.len(),
        names.len() - to_hold.len(),
        failures.len(),
        names.len()
    );
    for line in departed::said(&names, DEPARTED) {
        println!("{line}");
    }
    println!("held: {held}");
    assert!(
        failures.is_empty(),
        "{} traces were not answered as Node answered them:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// A trace is named in [`DEPARTED`] because it differs from Node's, and for no
/// other reason.
#[test]
fn every_departed_trace_is_there_and_still_departs() {
    let wrong = departed::wrong(DEPARTED, &SUITES, |name| {
        matches!(
            catch_unwind(AssertUnwindSafe(|| trace::locally(player::play(name)))),
            Ok(Ok(_))
        )
    });
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
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
