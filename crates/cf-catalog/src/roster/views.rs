//! A roster row as the app sees it (`effortOf` and `toView`,
//! `src/roster.js`): the agent's settings under the app's own words
//! (`harness`, `effort`) and the profile of its model.
//!
//! The view keeps the row beside the [`Settings`] it builds the profile
//! from, and the two read the same row differently: the view says
//! `designer` for `true` alone, the profile for any truthy value; the view
//! names the harness of the row's kind, the profile reads the row's own
//! `harness` first; the view's effort is the one under the key the kind
//! reads, the profile's on Pi is `thinking ?? effort`.

use cf_base::js;
use cf_base::refusal::Refusal;
use cf_proto::agents::AgentView;
use serde_json::Value;

use super::agent_row::{effort_key, AgentRow};
use crate::{validate_work_tier, Catalog, Harness, Settings};

impl Catalog {
    /// `toView`: a row as the page and the CLI list it. A work tier the row
    /// names that is none of the four is refused, as its profile is built.
    pub(crate) fn view(&self, row: &AgentRow) -> Result<AgentView, Refusal> {
        let harness = row.kind().and_then(Harness::from_kind);
        let work_tier = validate_work_tier(row.get("workTier"))?;
        let profile = self.profile(&Settings {
            harness: row.harness(),
            kind: row.kind(),
            model: row.model(),
            effort: row.effort(),
            thinking: row.thinking(),
            designer: row.is_designer(),
            work_tier,
        });
        let description = row.get("description");
        Ok(AgentView {
            name: row.id().map(str::to_owned),
            harness: harness
                .map(|harness| harness.as_str().to_owned())
                .or_else(|| row.kind().map(str::to_owned)),
            designer: row.get("designer") == Some(&Value::Bool(true)),
            model: row.model().map(str::to_owned),
            work_tier,
            effort: effort_of(row).map(str::to_owned),
            description: description.filter(|_| js::truthy(description)).cloned(),
            preset: row
                .preset()
                .filter(|preset| !preset.is_empty())
                .map(str::to_owned),
            custom: js::truthy(row.get("custom")),
            profile,
            unsupported: harness.is_none(),
            hidden: false,
        })
    }
}

/// `effortOf`: the effort under the key the row's kind reads, when it is not
/// empty.
fn effort_of(row: &AgentRow) -> Option<&str> {
    match row.get(effort_key(row.kind())) {
        Some(Value::String(effort)) if !effort.is_empty() => Some(effort),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
