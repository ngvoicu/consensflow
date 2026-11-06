//! F6: a Refresh gives a package's fresh catalog, or says it could not (the review T-1270).
//!
//! A provider of a package lists what it holds at once, and fetches its catalog after: the
//! installed CLIProxyAPI package answers its cache first, asks its server in the background, and
//! lists the new catalog when it comes (measured on Pi 1.5.0: held 8 s, its cache file was
//! rewritten). `pi update --models` does not load packages, so a read that stops at the first
//! answer gives the old catalog as a fresh one. A Refresh keeps Pi's session open for
//! [`REFRESH_HOLD`], asks again, and takes the second answer.

use cf_proto::models::Freshness;

use super::standin::{answer, StandIn};
use super::*;

const HOLD_MS: i64 = 10_000;

fn ids(listing: &ModelListing) -> Vec<String> {
    listing.models.iter().map(ModelChoice::model).collect()
}

#[test]
fn a_refresh_asks_again_after_the_hold_and_takes_the_second_answer() {
    let pi = StandIn::new(&[], &[]);
    pi.processes
        .run_answer("pi update --models", Ok(String::new()));
    // The package's cache, then the catalog it fetched.
    pi.child(
        &[
            answer("models", &["pkg/cached"]),
            answer("models-again", &["pkg/cached", "pkg/fresh"]),
        ],
        Ends::Asked,
    );
    let mut driver = pi.begin(true);
    assert!(driver.run().is_empty(), "the session is held open");
    assert_eq!(
        pi.processes.take_written().len(),
        1,
        "one question until the hold is up"
    );
    assert!(pi.time.fire_next(HOLD_MS), "the hold is armed");
    assert_eq!(pi.time.wall_ms(), HOLD_MS, "held for REFRESH_HOLD");
    assert_eq!(REFRESH_HOLD.as_millis(), HOLD_MS as u128);
    let listing = driver.run().remove(0).1.unwrap();
    assert_eq!(
        ids(&listing),
        ["pkg/cached", "pkg/fresh"],
        "the second answer"
    );
    assert!(!listing.is_stale());
    assert_eq!(
        pi.processes.take_written().len(),
        1,
        "asked again once, by the other request"
    );
}

#[test]
fn a_refresh_that_cannot_ask_again_keeps_the_first_answer_and_says_so() {
    let pi = StandIn::new(&[], &[]);
    pi.processes
        .run_answer("pi update --models", Ok(String::new()));
    // Pi answers once and goes.
    pi.child(&[answer("models", &["pkg/cached"])], Ends::Itself);
    let mut driver = pi.begin(true);
    assert!(driver.run().is_empty());
    assert!(pi.time.fire_next(HOLD_MS));
    let listing = driver.run().remove(0).1.unwrap();
    assert_eq!(ids(&listing), ["pkg/cached"]);
    let Freshness::Stale { why } = &listing.freshness else {
        panic!("the first answer is no fresh catalog: {listing:?}");
    };
    assert!(
        why.contains("did not list again") && why.contains("ended before it answered"),
        "{why}"
    );
}

#[test]
fn an_ordinary_read_stops_at_the_first_answer_and_holds_nothing() {
    let pi = StandIn::new(&[], &[]);
    pi.child(&[answer("models", &["pkg/cached"])], Ends::Asked);
    let listing = pi.read(false).unwrap();
    assert_eq!(ids(&listing), ["pkg/cached"]);
    assert_eq!(pi.processes.take_written().len(), 1, "asked once");
    assert!(pi.processes.take_ran().is_empty(), "nothing fetched either");
}
