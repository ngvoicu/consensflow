//! What the home keeps of each harness's models (CHF-03): its last listing, when and how
//! it was read, and why it is stale where it is; kept across a reopen, one row a harness.

#![allow(clippy::unwrap_used)]

use cf_ledger::{open_ledger, Options};
use cf_proto::models::{Freshness, ModelChoice, ModelListing};

fn listing(ids: &[&str]) -> ModelListing {
    ModelListing::read(
        ids.iter().map(|id| ModelChoice::new(*id, *id)).collect(),
        10,
        "devin models list --format json",
    )
}

#[test]
fn a_harness_s_listing_is_kept_whole_and_a_failed_read_keeps_the_one_before_stale() {
    let folder = tempfile::tempdir().unwrap();
    let file = folder.path().join("consensflow.db");
    let mut ledger = open_ledger(&file, Options::default()).unwrap();
    assert_eq!(ledger.model_listing("devin").unwrap(), None, "never tried");

    let read = ModelListing::after(None, Ok(listing(&["swe-2", "adaptive"])));
    ledger.keep_model_listing("devin", &read).unwrap();
    assert_eq!(ledger.model_listing("devin").unwrap(), Some(read.clone()));

    let kept = ledger.model_listing("devin").unwrap();
    let failed = ModelListing::after(kept, Err("Not logged in".to_owned()));
    ledger.keep_model_listing("devin", &failed).unwrap();
    ledger.close().unwrap();

    let ledger = open_ledger(&file, Options::default()).unwrap();
    let after = ledger.model_listing("devin").unwrap().unwrap();
    assert_eq!(after.models, read.models, "the listing read before, kept");
    assert_eq!(after.read_at, Some(10));
    assert_eq!(
        after.freshness,
        Freshness::Stale {
            why: "Not logged in".to_owned()
        }
    );
    assert_eq!(after.how, "devin models list --format json");
    assert_eq!(
        ledger.model_listing("claude").unwrap(),
        None,
        "one row a harness"
    );
}

#[test]
fn a_listing_that_names_no_harness_is_refused() {
    let folder = tempfile::tempdir().unwrap();
    let mut ledger =
        open_ledger(&folder.path().join("consensflow.db"), Options::default()).unwrap();
    assert!(ledger.keep_model_listing("", &listing(&["a"])).is_err());
}
