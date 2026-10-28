//! What the home keeps of each harness's models (CHF-03): the last list read, how and
//! when, and what became of the last try; kept across a reopen, one row a harness.

#![allow(clippy::unwrap_used)]

use cf_ledger::{open_ledger, Options};
use cf_proto::models::{ModelChoice, ModelList, Models};

fn models(ids: &[&str]) -> Models {
    Models {
        models: ids.iter().map(|id| ModelChoice::new(*id, *id)).collect(),
        stale: false,
        how: "devin models list --format json".to_owned(),
    }
}

#[test]
fn a_harness_s_list_is_kept_whole_and_a_failed_try_keeps_the_one_before_marked_stale() {
    let folder = tempfile::tempdir().unwrap();
    let file = folder.path().join("consensflow.db");
    let mut ledger = open_ledger(&file, Options::default()).unwrap();
    assert_eq!(ledger.model_list("devin").unwrap(), None, "never tried");

    let read = ModelList::after("devin", None, 10, Ok(models(&["swe-2", "adaptive"])));
    ledger.keep_model_list(&read).unwrap();
    assert_eq!(ledger.model_list("devin").unwrap(), Some(read.clone()));

    let kept = ledger.model_list("devin").unwrap();
    let failed = ModelList::after("devin", kept, 20, Err("Not logged in".to_owned()));
    ledger.keep_model_list(&failed).unwrap();
    ledger.close().unwrap();

    let ledger = open_ledger(&file, Options::default()).unwrap();
    let after = ledger.model_list("devin").unwrap().unwrap();
    assert_eq!(after.models, read.models, "the list read before, kept");
    assert_eq!(
        (
            after.stale,
            after.read_at,
            after.tried_at,
            after.failure.as_deref()
        ),
        (true, Some(10), 20, Some("Not logged in"))
    );
    assert_eq!(after.how, "devin models list --format json");
    assert_eq!(
        ledger.model_list("claude").unwrap(),
        None,
        "one row a harness"
    );
}

#[test]
fn a_list_that_names_no_harness_is_refused() {
    let folder = tempfile::tempdir().unwrap();
    let mut ledger =
        open_ledger(&folder.path().join("consensflow.db"), Options::default()).unwrap();
    let nameless = ModelList::after("", None, 1, Ok(models(&["a"])));
    assert!(ledger.keep_model_list(&nameless).is_err());
}
