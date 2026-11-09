//! A chief as the page asks for one, in New project and Switch chief (CHF-08, HMD-04): a
//! harness, a model of it and an effort it lists for that model, `{harness, model, effort}`,
//! never a saved agent. The harness is the word the
//! ledger and the board say it in (`claude-code`), one ConsensFlow has an adapter for and
//! that is installed here; the model is one the harness lists, where the home keeps the
//! last read of it ([`crate::models::ModelLists`]): the picker read it to offer it. Nothing is
//! read from a harness here: a home that keeps no listing of it (none read since the daemon
//! started, or the read failed) takes the model on the harness's word at the launch, where a
//! model it does not have starts the chief on its default (CHF-06).

use cf_engine::{require_chief_model, SwitchTo};
use cf_harness::detect::missing_harnesses;
use cf_proto::agents::Harness;
use serde_json::Value;

use super::body::{Body, Said};
use super::staff::{effort_of, read_of, resolve};
use super::Page;

/// The chief `body` asks for (`harness`, `model`, `effort`): a model is named; where
/// `installed` asks it, its harness is installed here (a Switch chief's: New project offers
/// only those, and Node's never asked it of one); and the model and its effort are checked
/// and held as a member's are ([`resolve`]): one the harness lists and runs, at an effort it
/// lists for it, Devin's as the exact id it lists for the effort, where the home keeps its
/// listing. That ConsensFlow opens the harness's windows at all is the engine's to say, as it
/// opens the project or switches the chief.
pub(super) async fn chief_asked(
    page: &Page,
    body: Body<'_>,
    installed: bool,
) -> Result<SwitchTo, Said> {
    let model = require_chief_model(body.get("model").and_then(Value::as_str))?;
    let effort = effort_of(body)?;
    let kind = body.text("harness").into_owned();
    let Some(harness) = Harness::from_kind(&kind) else {
        // No harness ConsensFlow runs: the engine refuses it in its own words.
        return Ok(SwitchTo {
            harness: kind,
            model: model.to_owned(),
            effort: effort.map(str::to_owned),
        });
    };
    if installed && missing_harnesses(&page.env).contains(&harness) {
        return Err(Said(format!("{kind} is not installed here")));
    }
    let resolved = resolve(page, harness, &kind, model, effort, read_of(body)?).await?;
    Ok(SwitchTo {
        harness: kind,
        model: resolved.model,
        effort: resolved.effort,
    })
}
