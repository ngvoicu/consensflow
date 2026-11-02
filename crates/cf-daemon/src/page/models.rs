//! `models.get`: the models a harness lists, for the chief's picker and the staff chains'
//! editor (CHF-03), as the home keeps them ([`crate::models::ModelLists`]): the listing
//! (its models, when and how it was read, and why it is stale where it is), the model a harness is started
//! on where nothing names one (never a contributor model, never from a stale list), and
//! the words a picker shows beside a contributor model, and how the harness switches its
//! model in its own window (CHF-07).

use cf_catalog::Catalog;
use cf_proto::agents::Harness;
use cf_proto::models::{ModelChoice, ModelListing, CONTRIBUTOR_WARNING};
use serde_json::{json, Map, Value};

use super::agents::saved;
use super::body::{merged, Body, Fields, Said};
use super::Page;

/// `models.get`: `harness` by its CLI's name (`claude`); `refresh` is the human's Refresh.
pub(super) async fn get(page: &Page, body: Body<'_>) -> Result<Fields, Said> {
    let name = body.text("harness");
    let harness = Harness::from_name(&name).ok_or_else(|| format!("{name:?} is no harness"))?;
    let refresh = body.get("refresh") == Some(&Value::Bool(true));
    let listing = page.models.get(&page.ledger, harness, refresh).await?;
    let default = listing.default_model().map(ModelChoice::model);
    let tiers = preset_tiers(saved(&page.env)?.catalog(), harness, &listing);
    merged(json!({
        "harness": harness.as_str(),
        "listing": listing,
        "default": default,
        "contributorWarning": CONTRIBUTOR_WARNING,
        "switchHelp": page.models.switch_help(harness),
        "presetTiers": tiers,
    }))
}

/// The tier the presets' rules give each listed model at each of its efforts, by the
/// model's name ([`ModelChoice::model`]) and the effort (`""` for a model with none): what
/// Add member and Edit member start their tier select on (T-1143). A model no preset runs
/// on the harness is not said: the human picks its tier. Goes with the presets (CHN-10).
pub(super) fn preset_tiers(catalog: &Catalog, harness: Harness, listing: &ModelListing) -> Value {
    let mut tiers = Map::new();
    for model in &listing.models {
        let name = model.model();
        let efforts: Vec<Option<&str>> = if model.efforts.is_empty() {
            vec![None]
        } else {
            model.efforts.iter().map(|effort| Some(effort.as_str())).collect()
        };
        let mut by_effort = Map::new();
        for effort in efforts {
            if let Some(tier) = catalog.preset_tier(harness.as_str(), &name, effort) {
                by_effort.insert(effort.unwrap_or_default().to_owned(), json!(tier.as_str()));
            }
        }
        if !by_effort.is_empty() {
            tiers.insert(name, Value::Object(by_effort));
        }
    }
    Value::Object(tiers)
}
