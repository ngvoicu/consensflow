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
fn a_harness_s_listing_is_kept_whole_and_a_failed_read_replaces_it_with_no_list() {
    let folder = tempfile::tempdir().unwrap();
    let file = folder.path().join("consensflow.db");
    let mut ledger = open_ledger(&file, Options::default()).unwrap();
    assert_eq!(ledger.model_listing("devin").unwrap(), None, "never tried");

    let read = listing(&["swe-2", "adaptive"]);
    ledger.keep_model_listing("devin", &read).unwrap();
    assert_eq!(ledger.model_listing("devin").unwrap(), Some(read.clone()));

    let failed = ModelListing::failed("Not logged in");
    ledger.keep_model_listing("devin", &failed).unwrap();
    ledger.close().unwrap();

    let mut ledger = open_ledger(&file, Options::default()).unwrap();
    let after = ledger.model_listing("devin").unwrap().unwrap();
    assert!(
        after.models.is_empty(),
        "the list read before is not kept in its place"
    );
    assert_eq!(after.read_at, None);
    assert_eq!(
        after.freshness,
        Freshness::Stale {
            why: "Not logged in".to_owned()
        }
    );
    assert_eq!(
        ledger.model_listing("claude").unwrap(),
        None,
        "one row a harness"
    );

    ledger.keep_model_listing("claude", &read).unwrap();
    ledger.forget_model_listings().unwrap();
    assert_eq!(ledger.model_listing("devin").unwrap(), None, "forgotten");
    assert_eq!(ledger.model_listing("claude").unwrap(), None, "every row");
}

#[test]
fn a_listing_that_names_no_harness_is_refused() {
    let folder = tempfile::tempdir().unwrap();
    let mut ledger =
        open_ledger(&folder.path().join("consensflow.db"), Options::default()).unwrap();
    assert!(ledger.keep_model_listing("", &listing(&["a"])).is_err());
}
