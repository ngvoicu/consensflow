//! A Refresh runs `pi update --models` and asks Pi once, as an ordinary read does. It holds
//! Pi's session for nothing: a hold of 10 s, a second question and the stale list a second
//! refusal dropped were the second reading's F6, and the hold took the Refresh past the app's
//! 30 s bridge deadline (F8). A package that fills its catalog a few seconds after Pi starts
//! shows its new models at the next Refresh.

use super::standin::{answer, StandIn};
use super::*;

fn ids(listing: &ModelListing) -> Vec<String> {
    listing.models.iter().map(ModelChoice::model).collect()
}

#[test]
fn a_refresh_runs_the_update_then_asks_pi_once_and_holds_nothing() {
    let pi = StandIn::new(&[], &[]);
    pi.processes
        .run_answer("pi update --models", Ok(String::new()));
    pi.child(&[answer("models", &["pkg/cached"])], Ends::Asked);
    let mut driver = pi.begin(true);
    let mut settled = driver.run();
    assert_eq!(
        settled.len(),
        1,
        "the Refresh answers at the first answer of Pi"
    );
    let listing = settled.remove(0).1.unwrap();
    assert_eq!(ids(&listing), ["pkg/cached"]);
    assert!(!listing.is_stale());
    assert_eq!(
        pi.processes.take_ran().len(),
        1,
        "the catalogs were fetched first"
    );
    assert_eq!(pi.processes.take_written().len(), 1, "asked once");
    assert!(
        !pi.time.fire_next(i64::MAX),
        "no timer is armed: Pi is not held"
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
