//! The models Muse offers and the efforts each takes, as its host lists them
//! (`model/list`, MUS-15): read over a bounded one-shot of `muse serve` (`msp`), with no
//! model turn and no allowance. This is the Muse half of the harnesses' `models()`
//! (CHF-03, CHN-10).
//!
//! `source` says where the list came from. `providerCatalog` is the account's own,
//! fetched with its login (5 models on 2026-10-27); `bundledCatalog` is what Muse ships,
//! one stale model (`muse-spark-1.2`), which a host answers where it has no login or no
//! cached catalog yet: such a listing is stale ([`BUNDLED`]), and is not taken for the
//! account's: it gives no default.
//!
//! The listing is the one every harness answers (`cf_proto::models::ModelListing`,
//! CHF-03): **a contributor model is never its default** (Meta may train on what it sees:
//! its own description says "your content … may be used for product improvement"). Muse marks
//! one `isDefault` (`muse-spark-1.3-contributor` on a fresh config, which runs at `max`):
//! `ModelListing::default_model` answers its twin, and `warning` is what a picker shows
//! beside one.

use std::path::PathBuf;

use cf_base::env::Env;
use cf_proto::models::{ModelChoice, ModelListing};
use serde_json::{json, Value};

use super::msp;
use crate::contract::NoDefault;
use crate::seams::processes::{Processes, Program};
use crate::seams::Time;

/// How a listing is read, as the human could run it.
pub const HOW: &str = "muse serve --provider meta --no-session-log: model/list";

/// Why a list a host answered from what Muse ships is stale.
pub const BUNDLED: &str =
    "Muse listed the models it ships, not the account's: it has no login here, or has not fetched its catalog yet";

/// Whether `id` names a contributor model: `-contributor` ends it, or its description says
/// what Meta does with what it sees.
fn contributor(id: &str, description: Option<&str>) -> bool {
    cf_proto::models::is_contributor(id)
        || description.is_some_and(|said| said.contains("may be used for product improvement"))
}

/// What a `model/list` result read at `at` says. Fails where it names no model: a host that
/// lists none has nothing to offer, and says so.
pub fn parse(result: &Value, at: i64) -> Result<ModelListing, String> {
    let rows = result
        .get("models")
        .and_then(Value::as_array)
        .ok_or("Muse's model list has no models")?;
    let text = |row: &Value, name: &str| row.get(name).and_then(Value::as_str).map(str::to_owned);
    let models: Vec<ModelChoice> = rows
        .iter()
        // A model Muse hides from its pickers (the image model) is no one's choice.
        .filter(|row| row.get("visibility").and_then(Value::as_str) != Some("hidden"))
        .filter_map(|row| {
            let id = text(row, "modelId").filter(|id| !id.is_empty())?;
            let mut efforts: Vec<String> = row
                .get("variants")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|effort| effort.as_str().map(str::to_owned))
                .collect();
            if efforts.is_empty() {
                efforts = row
                    .get("reasoningEffortVariants")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|variant| variant.get("tier").and_then(Value::as_str))
                    .map(str::to_owned)
                    .collect();
            }
            let description = text(row, "description");
            let mut choice =
                ModelChoice::new(id.clone(), text(row, "displayLabel").unwrap_or(id.clone()));
            choice.contributor = contributor(&id, description.as_deref());
            choice.efforts = efforts;
            choice.default_effort = text(row, "defaultReasoningEffort");
            choice.marked_default = row.get("isDefault") == Some(&Value::Bool(true));
            Some(choice)
        })
        .collect();
    if models.is_empty() {
        return Err("Muse's model list names no model".to_owned());
    }
    let listing = ModelListing::read(models, at, HOW);
    Ok(
        if result.get("source").and_then(Value::as_str) == Some("providerCatalog") {
            listing
        } else {
            listing.stale(BUNDLED)
        },
    )
}

/// The host a list is read from: `muse serve` at `executable`, with no update check (`muse`
/// is its launcher, which runs one an hour unless told not to) and the login of `env` as the
/// human's own `muse` reads it.
pub fn program(executable: PathBuf, env: &Env) -> Program {
    Program {
        executable,
        args: msp::serve_args(),
        cwd: None,
        env: env.clone().with_var("MUSE_NO_AUTO_UPDATE", "1"),
    }
}

/// The models the host `program` (`muse serve`, [`msp::serve_args`]) lists: one bounded
/// conversation, no model turn, no allowance.
pub async fn read(
    processes: &dyn Processes,
    time: &dyn Time,
    program: Program,
) -> Result<ModelListing, String> {
    let at = time.wall_ms();
    let answers = msp::conversation(
        processes,
        time,
        program,
        &[("model/list", json!({}))],
        msp::BOUND_MS,
    )
    .await?;
    parse(&answers[0], at)
}

/// The models the machine's Muse lists, read as the daemon reads every harness's (CHF-03):
/// `muse serve`'s `model/list` over its own login, one bounded conversation.
pub(crate) async fn listed(
    env: &Env,
    processes: &dyn Processes,
    time: &dyn Time,
) -> Result<ModelListing, String> {
    let executable = crate::detect::executable(cf_proto::agents::Harness::Muse, env)?;
    read(processes, time, program(PathBuf::from(executable), env)).await
}

/// Whether `words` are Muse's provider refusing `asked` as a model the account does not
/// have: the window opens on any id, and its first turn fails with the provider's 404,
/// "model `X` does not exist or you lack access" (1.4.14, CHF-02), the whole id named,
/// which the turn's record keeps as its failure.
pub(crate) fn gone(words: &str, asked: &str) -> bool {
    words.contains("does not exist or you lack access") && words.contains(asked)
}

/// The model Muse starts a chief on where its model is gone: the account's default, never a
/// contributor model (`ModelListing::default_model`). Never Muse's own choice, as other
/// harnesses' may be: Muse marks a contributor model its default on a fresh config, so a
/// list that cannot be read, a stale one (what Muse ships, which the account may not offer)
/// or one with no other model is an error, and the chief stays held (CHF-06, T-955).
pub(crate) async fn default_model(
    env: &Env,
    processes: &dyn Processes,
    time: &dyn Time,
) -> Result<Option<String>, NoDefault> {
    default_of(listed(env, processes, time).await)
}

/// [`default_model`] of what a read of the list answered. The words of a read that failed, or
/// of a stale list's reason, are Muse's and the page's: the chief's note says only that the list
/// could not be read (T-1328).
fn default_of(listed: Result<ModelListing, String>) -> Result<Option<String>, NoDefault> {
    let listing = listed.map_err(|_| NoDefault::Unread(None))?;
    if matches!(listing.freshness, cf_proto::models::Freshness::Stale { .. }) {
        return Err(NoDefault::Unread(None));
    }
    listing
        .default_model()
        .map(|model| Some(model.model()))
        .ok_or(NoDefault::NoneOffered(
            "Muse Code offers no model that is not a contributor model",
        ))
}

#[cfg(test)]
mod tests;
