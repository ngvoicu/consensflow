//! An agent defined by hand, edited in place or removed (`applyPatch`,
//! `refuseEffortEdit`, `editAgent` and `removeAgent`, `src/roster.js`). A
//! catalog agent is the catalog's: it is neither edited nor removed.

use std::path::Path;

use cf_base::js;
use cf_base::json::js_order_fields;
use cf_base::refusal::Refusal;
use cf_base::time::{iso, Clock};
use cf_proto::agents::AgentView;
use serde_json::{Map, Value};

use super::add::{model_needed, refuse_an_effort_that_is_no_text};
use super::agent_row::{effort_key, AgentRow};
use super::document::{load_document, Document};
use super::save::save_document;
use crate::presets::Preset;
use crate::{harness_for_kind, validate_work_tier, Catalog};

impl Catalog {
    /// Edits the agent named `name` in the file at `path` by `patch`, and
    /// answers it as the page lists it. The time is read after every
    /// refusal, once. The patch is read as `JSON.parse` handed it to Node,
    /// its keys in JavaScript's order.
    pub(crate) fn edit(
        &self,
        path: &Path,
        name: &str,
        patch: &Map<String, Value>,
        clock: &mut dyn Clock,
    ) -> Result<AgentView, Refusal> {
        let patch = &js_order_fields(patch.clone());
        validate_work_tier(patch.get("workTier"))?;
        let mut document = load_document(path)?;
        let at = self.own_row(&document, name, || {
            format!(
                "{name} is a catalog agent and stays as the catalog has it: define your own with the settings you want"
            )
        })?;
        {
            let row = &mut document.agents_mut()[at];
            if patch.contains_key("effort")
                && (row.kind().and_then(harness_for_kind).is_none()
                    || row.get("designer") == Some(&Value::Bool(true)))
            {
                return Err(refuse_effort_edit(name, row));
            }
            apply_patch(row, patch)?;
            row.set("updatedAt", Value::from(iso(clock.now_ms())));
        }
        save_document(path, &mut document)?;
        // The row as the save left it, its stale fields gone.
        let mut shown = document.agents()[at].clone();
        shown.set("custom", Value::Bool(true));
        self.view(&shown)
    }

    /// Removes the agent named `name` from the file at `path`: the one row
    /// found, though another has its id too.
    pub(crate) fn remove(&self, path: &Path, name: &str) -> Result<(), Refusal> {
        let mut document = load_document(path)?;
        let at = self.own_row(&document, name, || {
            format!("{name} is a catalog agent: it is not yours to remove")
        })?;
        document.agents_mut().remove(at);
        save_document(path, &mut document)
    }

    /// Where the human's own agent `name` is among the rows: the first row
    /// with its id. A catalog agent of that name is refused (`catalog_says`)
    /// when no row has the name, or the first that does is that entry's
    /// copy; a name nothing has, with Node's sentence.
    fn own_row(
        &self,
        document: &Document,
        name: &str,
        catalog_says: impl FnOnce() -> String,
    ) -> Result<usize, Refusal> {
        let at = document
            .agents()
            .iter()
            .position(|row| row.id() == Some(name));
        if let Some(entry) = self.preset_with_id(name) {
            let copy = at.is_some_and(|at| {
                self.entry_of(&document.agents()[at])
                    .is_some_and(|found| std::ptr::eq(found, entry))
            });
            if at.is_none() || copy {
                return Err(Refusal::new("agent-catalog", catalog_says()));
            }
        }
        at.ok_or_else(|| Refusal::new("agent-unknown", format!("no agent named {name}")))
    }

    /// The preset with the id `id` (`CATALOG_BY_ID`): of two the later, as a
    /// JavaScript `Map` keeps the last value of a key.
    fn preset_with_id(&self, id: &str) -> Option<&Preset> {
        self.presets().iter().rfind(|preset| preset.id == id)
    }
}

/// `applyPatch`: the patch on a row of the file's shape, each field Node
/// applied in Node's order, the effort under the key its kind reads.
fn apply_patch(row: &mut AgentRow, patch: &Map<String, Value>) -> Result<(), Refusal> {
    if let Some(model) = patch.get("model") {
        match model {
            Value::String(text) if !text.is_empty() => row.set("model", model.clone()),
            _ => return Err(model_needed()),
        }
    }
    if let Some(description) = patch.get("description") {
        row.set("description", description.clone());
    }
    if let Some(tier) = patch.get("workTier") {
        if tier.is_null() {
            row.remove("workTier");
        } else {
            row.set("workTier", tier.clone());
        }
    }
    if let Some(effort) = patch.get("effort") {
        let key = effort_key(row.kind());
        match effort {
            Value::Null => row.remove(key),
            Value::String(text) if text.is_empty() => row.remove(key),
            _ => {
                refuse_an_effort_that_is_no_text(Some(effort))?;
                row.set(key, effort.clone());
            }
        }
        // Never leave a stale value in the key this kind does not read.
        row.remove(if key == "thinking" {
            "effort"
        } else {
            "thinking"
        });
    }
    Ok(())
}

/// Two refusals that used to be one: a kind this build cannot run at all,
/// and an image agent, which it runs but which has no effort to set: its
/// window is Codex on its own default model, whose image tool draws.
fn refuse_effort_edit(name: &str, row: &AgentRow) -> Refusal {
    let message = if row.get("designer") == Some(&Value::Bool(true)) {
        format!(
            "{name} is an image agent: it has no effort level \u{2014} only its model and description can be edited"
        )
    } else {
        format!(
            "{name} is a {} agent, which this build does not run; only its model and description can be edited here",
            js::text(row.get("kind"))
        )
    };
    Refusal::new("agent-effort-fixed", message)
}

#[cfg(test)]
mod tests;
