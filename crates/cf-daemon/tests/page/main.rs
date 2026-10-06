//! The page's operations held to Node's recordings: every page trace of
//! `tests/goldens/` (`core-page-*` and `corners-page-*`) played as
//! `tests/goldens/daemon/FORMAT.md` says. Each operation is asked over a bridge
//! as the app asks it, with the dispatcher of Node's tests as the engine; its
//! reply is compared as the bytes the bridge carried, its kicks, the files it
//! wrote (none, where the trace says none), the events its ledger calls logged
//! and every call it made on a stand-in, with its arguments, in its place
//! among the ledger's own; and the ledger is left as Node left it.

// The player's own scaffolding: a failure in it is the test's.
#![allow(clippy::expect_used, clippy::unwrap_used)]

mod notes;
mod player;
mod standin;
mod wire;

// What the three players share, taken whole.
#[path = "../support/ledger.rs"]
mod ledger;
#[path = "../support/mod.rs"]
mod support;
#[path = "../support/world.rs"]
mod world;
#[path = "../support/wrote.rs"]
mod wrote;

use cf_proto::page::PageOperation;
use support::trace::{self, Tally};

/// The suites of the page's traces: the 28 tests of `core-page.test.mjs` and the
/// 8 of `corners-page.test.mjs`.
const SUITES: [&str; 2] = ["core-page", "corners-page"];

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

/// Every page trace of Node's answers here as it answered there.
#[test]
fn every_page_trace_is_answered_as_node_answered() {
    let names = trace::names(&SUITES);
    assert_eq!(names.len(), 36, "{names:?}: npm run goldens:daemon");
    let mut held = Tally::default();
    let failed: Vec<String> = names
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
        "{} page traces played, {} not answered as Node's",
        names.len(),
        failed.len()
    );
    println!("held: {held}");
    assert!(
        failed.is_empty(),
        "{} of {} traces answered otherwise:\n{}",
        failed.len(),
        names.len(),
        failed.join("\n")
    );
}
