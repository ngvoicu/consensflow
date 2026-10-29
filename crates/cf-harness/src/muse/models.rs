//! The models Muse offers and the efforts each takes, as its host lists them
//! (`model/list`, MUS-15): read over a bounded one-shot of `muse serve` (`msp`), with no
//! model turn and no allowance. This is the Muse half of the harnesses' `models()`
//! (CHF-03, CHN-10).
//!
//! `source` says where the list came from. `providerCatalog` is the account's own,
//! fetched with its login (5 models on 2026-10-27); `bundledCatalog` is what Muse ships,
//! one stale model (`muse-spark-1.2`), which a host answers where it has no login or no
//! cached catalog yet: such a list is [`Models::stale`], and is not taken for the
//! account's.
//!
//! **A contributor model is never a default** (Meta may train on what it sees: its own
//! description says "your content … may be used for product improvement"). Muse marks one
//! `isDefault` (`muse-spark-1.3-contributor` on a fresh config, which runs at `max`):
//! [`Models::default_model`] never answers it, and [`warning`] says what a picker shows
//! beside one.

use std::path::PathBuf;

use cf_base::env::Env;
use serde_json::{json, Value};

use super::msp;
use crate::seams::processes::{Processes, Program};
use crate::seams::Time;

/// What a picker says beside a contributor model.
pub const CONTRIBUTOR_WARNING: &str =
    "Contributor model: Meta may train on what it sees, including your code.";

/// A model Muse offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelChoice {
    pub id: String,
    pub label: String,
    /// The efforts the model takes, from the lowest, as Muse orders them (`variants`).
    pub efforts: Vec<String>,
    /// The effort Muse runs it at where none is given.
    pub default_effort: Option<String>,
    /// Muse's own mark of the model it starts on: never ConsensFlow's choice where this is a
    /// contributor model.
    pub marked_default: bool,
    /// Meta may train on what it sees.
    pub contributor: bool,
}

/// What `model/list` answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Models {
    pub models: Vec<ModelChoice>,
    /// The list is Muse's bundled one, not the account's: it names what Muse ships, and the
    /// models of the account are not known.
    pub stale: bool,
}

/// Whether `id` names a contributor model: `-contributor` ends it.
fn contributor(id: &str, description: Option<&str>) -> bool {
    id.ends_with("-contributor")
        || description.is_some_and(|said| said.contains("may be used for product improvement"))
}

impl Models {
    /// The model a member is given where its agent names none: the one Muse marks, unless it
    /// is a contributor model, then its twin that is not (`muse-spark-1.3` for
    /// `muse-spark-1.3-contributor`), else the first that is not one. None where every model
    /// is: no default is better than one Meta trains on. None too where the list is
    /// [`Models::stale`], Muse's bundled one: it names what Muse ships, which the account may
    /// not offer, and a member launched on it would fail at its first turn (T-816).
    pub fn default_model(&self) -> Option<&ModelChoice> {
        if self.stale {
            return None;
        }
        let marked = self.models.iter().find(|model| model.marked_default);
        let twin = marked
            .filter(|model| model.contributor)
            .and_then(|model| model.id.strip_suffix("-contributor"))
            .and_then(|twin| self.models.iter().find(|model| model.id == twin));
        marked
            .filter(|model| !model.contributor)
            .or(twin)
            .or_else(|| self.models.iter().find(|model| !model.contributor))
    }

    /// The model `id`, if listed.
    pub fn model(&self, id: &str) -> Option<&ModelChoice> {
        self.models.iter().find(|model| model.id == id)
    }
}

/// What a picker shows beside `model`: a warning for a contributor model, none else.
pub fn warning(model: &ModelChoice) -> Option<&'static str> {
    model.contributor.then_some(CONTRIBUTOR_WARNING)
}

/// What a `model/list` result says. Fails where it names no model: a host that lists none
/// has nothing to offer, and says so.
pub fn parse(result: &Value) -> Result<Models, String> {
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
            Some(ModelChoice {
                label: text(row, "displayLabel").unwrap_or_else(|| id.clone()),
                contributor: contributor(&id, description.as_deref()),
                efforts,
                default_effort: text(row, "defaultReasoningEffort"),
                marked_default: row.get("isDefault") == Some(&Value::Bool(true)),
                id,
            })
        })
        .collect();
    if models.is_empty() {
        return Err("Muse's model list names no model".to_owned());
    }
    Ok(Models {
        models,
        stale: result.get("source").and_then(Value::as_str) != Some("providerCatalog"),
    })
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
) -> Result<Models, String> {
    let answers = msp::conversation(
        processes,
        time,
        program,
        &[("model/list", json!({}))],
        msp::BOUND_MS,
    )
    .await?;
    parse(&answers[0])
}

#[cfg(test)]
mod tests;
