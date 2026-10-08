//! The page's operations held to Node's recordings: every page trace of
//! `tests/goldens/` (`core-page-*` and `corners-page-*`) played as
//! `tests/goldens/FORMAT.md` says. Each operation is asked over a bridge as the
//! app asks it, with the dispatcher of Node's tests as the engine; its reply is
//! compared as the bytes the bridge carried, its kicks, the files it wrote
//! (none, where the trace says none), the events its ledger calls logged and
//! every call it made on a stand-in, with its arguments, in its place among the
//! ledger's own; and the ledger is left as Node left it.

// The player's own scaffolding: a failure in it is the test's.
#![allow(clippy::expect_used, clippy::unwrap_used)]

mod notes;
mod player;
mod standin;
mod wire;

// What the players share, taken whole.
#[path = "../support/departed.rs"]
mod departed;
#[path = "../support/ledger.rs"]
mod ledger;
#[path = "../support/mod.rs"]
mod support;
#[path = "../support/world.rs"]
mod world;
#[path = "../support/wrote.rs"]
mod wrote;

use cf_proto::page::PageOperation;
use departed::Departed;
use support::trace::{self, Tally};

/// The suites of the page's traces: the 28 tests of Node's page suite and the 8
/// of its corners.
const SUITES: [&str; 2] = ["core-page", "corners-page"];

/// The traces the receipt and stop redesign moved on purpose, found by playing
/// them against the daemon.
const DEPARTED: &[Departed] = &[(
    "core-page-013",
    "a release cancels the old window's kept rows with the reason `carried into T-n's brief for its next window`; Node gave none",
)];

/// Every operation of the page is asked by some trace: what the traces hold is
/// the whole of what the page can ask.
#[test]
fn the_traces_ask_every_operation_of_the_page() {
    let mut asked = std::collections::BTreeSet::new();
    let (mut operations, mut calls) = (0, 0);
    for name in trace::names(&SUITES) {
        let played = trace::load(&name);
        for step in played["steps"].as_array().unwrap() {
            if step["kind"] == "operation" {
                operations += 1;
                calls += step["seams"].as_array().map_or(0, Vec::len);
                asked.insert(step["name"].as_str().unwrap().to_owned());
            }
        }
    }
    let every: std::collections::BTreeSet<String> = PageOperation::ALL
        .map(|operation| operation.as_str().to_owned())
        .into();
    assert_eq!(asked, every);
    println!("{operations} operations and {calls} calls on the stand-ins in the traces");
}

/// Every page trace of Node's but the departed answers here as it answered there.
#[test]
fn every_page_trace_is_answered_as_node_answered() {
    let names = trace::names(&SUITES);
    assert_eq!(
        names.len(),
        36,
        "{names:?}: the traces are fixed recordings (tests/goldens/README.md)"
    );
    let to_hold = departed::held(&names, DEPARTED);
    let mut held = Tally::default();
    let failed: Vec<String> = to_hold
        .iter()
        .filter_map(|name| match player::play(name) {
            Ok(tally) => {
                held += tally;
                None
            }
            Err(problems) => Some(format!("{name}:\n  {}", problems.join("\n  "))),
        })
        .collect();
    println!(
        "{} page traces played, {} departed, {} not answered as Node's",
        to_hold.len(),
        names.len() - to_hold.len(),
        failed.len()
    );
    for line in departed::said(&names, DEPARTED) {
        println!("{line}");
    }
    println!("held: {held}");
    assert!(
        failed.is_empty(),
        "{} of {} traces answered otherwise:\n{}",
        failed.len(),
        to_hold.len(),
        failed.join("\n")
    );
}

/// A trace is named in [`DEPARTED`] because it differs from Node's, and for no
/// other reason.
#[test]
fn every_departed_trace_is_there_and_still_departs() {
    let wrong = departed::wrong(DEPARTED, &SUITES, |name| player::play(name).is_ok());
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
