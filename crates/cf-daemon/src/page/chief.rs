//! A chief as the page asks for one, in New project and Switch chief (CHF-08, HMD-04): a
//! harness, a model of it and an effort it lists for that model, `{harness, model, effort}`,
//! never a saved agent. The harness is the word the
//! ledger and the board say it in (`claude-code`), one ConsensFlow has an adapter for and
//! that is installed here; the model is one the harness lists, where the home keeps its
//! listing ([`crate::models::ModelLists`]): the picker read it to offer it. Nothing is
//! read from a harness here: a home that keeps no listing of it yet takes the model on
//! the harness's word at the launch, where a model it does not have starts the chief on
//! its default (CHF-06).

use cf_proto::agents::Harness;
use cf_engine::{require_chief_model, SwitchTo};
use cf_harness::detect::missing_harnesses;
use serde_json::Value;

use super::body::{Body, Said};
use super::Page;

/// The chief `body` asks for (`harness`, `model`): a model is named; where `installed`
/// asks it, its harness is installed here (a Switch chief's: New project offers only
/// those, and Node's never asked it of one); and the model is one the harness lists and
/// runs, where the home keeps its listing. That ConsensFlow opens the harness's windows at
/// all is the engine's to say, as it opens the project or switches the chief.
pub(super) fn chief_asked(page: &Page, body: Body<'_>, installed: bool) -> Result<SwitchTo, Said> {
    let model = require_chief_model(body.get("model").and_then(Value::as_str))?;
    let kind = body.text("harness").into_owned();
    if let Some(harness) = Harness::from_kind(&kind) {
        if installed && missing_harnesses(&page.env).contains(&harness) {
            return Err(Said(format!("{kind} is not installed here")));
        }
        listed(page, harness, &kind, model)?;
    }
    Ok(SwitchTo {
        harness: kind,
        model: model.to_owned(),
        effort: None,
    })
}

/// Whether `harness`, as the home keeps its listing, lists `model` and runs it: refused
/// where it keeps one that does not, and taken on the harness's word where it keeps none.
fn listed(page: &Page, harness: Harness, kind: &str, model: &str) -> Result<(), Said> {
    cf_ledger::model::require_model_id(model)?;
    let kept = page
        .ledger
        .borrow()
        .model_listing(harness.as_str())
        .map_err(|failed| failed.to_string())?;
    let Some(listing) = kept.filter(|listing| !listing.models.is_empty()) else {
        return Ok(());
    };
    match listing.model(model) {
        None => Err(Said(format!(
            "{kind} does not list {model}: pick one of its models"
        ))),
        Some(listed) if listed.disabled => Err(Said(format!(
            "{kind} lists {model} but will not run it: pick another of its models"
        ))),
        Some(_) => Ok(()),
    }
}
