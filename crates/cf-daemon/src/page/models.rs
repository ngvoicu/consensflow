//! `models.get`: the models a harness lists, for the chief's picker and the staff chains'
//! editor (CHF-03), as the home keeps them ([`crate::models::ModelLists`]): the listing
//! (its models, when and how it was read, and why it is stale where it is), the model a harness is started
//! on where nothing names one (never a contributor model, never from a stale list), and
//! the words a picker shows beside a contributor model.

use cf_proto::agents::Harness;
use cf_proto::models::{ModelChoice, CONTRIBUTOR_WARNING};
use serde_json::{json, Value};

use super::body::{merged, Body, Fields, Said};
use super::Page;

/// `models.get`: `harness` by its CLI's name (`claude`); `refresh` is the human's Refresh.
pub(super) async fn get(page: &Page, body: Body<'_>) -> Result<Fields, Said> {
    let name = body.text("harness");
    let harness = Harness::from_name(&name).ok_or_else(|| format!("{name:?} is no harness"))?;
    let refresh = body.get("refresh") == Some(&Value::Bool(true));
    let listing = page.models.get(&page.ledger, harness, refresh).await?;
    let default = listing.default_model().map(ModelChoice::model);
    merged(json!({
        "harness": harness.as_str(),
        "listing": listing,
        "default": default,
        "contributorWarning": CONTRIBUTOR_WARNING,
    }))
}
