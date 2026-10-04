//! An agent defined by hand (`validateAdd` and `addAgent`, `src/roster.js`):
//! the catalog's agents are there already. Each request is checked in the
//! order Node checked it, so the sentence a request hears is Node's.

use std::path::Path;

use cf_base::js;
use cf_base::json::as_parsed_fields;
use cf_base::refusal::Refusal;
use cf_base::time::{iso, Clock};
use cf_proto::agents::{AgentView, Harness};
use serde_json::{Map, Value};

use super::agent_row::{effort_key, AgentRow};
use super::document::load_document;
use super::save::save_document;
use crate::{validate_work_tier, Catalog, HARNESSES};

impl Catalog {
    /// Adds the agent `input` defines to the file at `path`, and answers it
    /// as the page lists it. The time is read once, for both of its stamps.
    /// The request is read as `JSON.parse` handed it to Node, its keys in
    /// JavaScript's order and its numbers JavaScript's: a description is
    /// stored and answered, and a name refused, as Node wrote them.
    pub(crate) fn add(
        &self,
        path: &Path,
        input: &Map<String, Value>,
        clock: &mut dyn Clock,
    ) -> Result<AgentView, Refusal> {
        let mut input = input.clone();
        as_parsed_fields(&mut input);
        let input = &input;
        let (name, harness, model) = self.checked_new(input)?;
        validate_work_tier(input.get("workTier"))?;
        let mut document = load_document(path)?;
        if document.agents().iter().any(|row| row.id() == Some(name)) {
            return Err(Refusal::new(
                "agent-exists",
                format!("an agent named {name} already exists"),
            ));
        }
        let effort = input
            .get("effort")
            .filter(|effort| js::truthy(Some(effort)));
        refuse_an_effort_that_is_no_text(effort)?;
        let now = Value::from(iso(clock.now_ms()));
        let mut row = Map::new();
        row.insert("id".to_owned(), Value::from(name));
        // The display name cc shows; capitalized to match its convention.
        row.insert("name".to_owned(), Value::from(capitalized(name)));
        row.insert("kind".to_owned(), Value::from(harness.kind()));
        if input.get("designer") == Some(&Value::Bool(true)) {
            row.insert("designer".to_owned(), Value::Bool(true));
        }
        row.insert("createdAt".to_owned(), now.clone());
        row.insert("updatedAt".to_owned(), now);
        row.insert("model".to_owned(), Value::from(model));
        if let Some(tier) = input.get("workTier").filter(|tier| !tier.is_null()) {
            row.insert("workTier".to_owned(), tier.clone());
        }
        if let Some(effort) = effort {
            row.insert(effort_key(Some(harness.kind())).to_owned(), effort.clone());
        }
        if let Some(description) = input
            .get("description")
            .filter(|description| js::truthy(Some(description)))
        {
            row.insert("description".to_owned(), description.clone());
        }
        let row = AgentRow::new(row);
        document.agents_mut().push(row.clone());
        save_document(path, &mut document)?;
        let mut shown = row;
        shown.set("custom", Value::Bool(true));
        self.view(&shown)
    }

    /// `validateAdd`: the name, the harness and the model a new agent needs,
    /// each refused in the order Node refused it.
    fn checked_new<'a>(
        &self,
        input: &'a Map<String, Value>,
    ) -> Result<(&'a str, Harness, &'a str), Refusal> {
        let name = match input.get("name") {
            Some(Value::String(name)) if is_agent_name(name) => name.as_str(),
            other => {
                return Err(Refusal::new(
                    "agent-name",
                    format!(
                        "agent names are lowercase [a-z0-9-] starting with a letter; got {}",
                        stringified(other)
                    ),
                ))
            }
        };
        if self.presets().iter().any(|preset| preset.id == name) {
            return Err(Refusal::new(
                "agent-name-taken",
                format!("{name} is a catalog agent: pick another name for your own"),
            ));
        }
        let harness = match input.get("harness") {
            Some(Value::String(word)) => Harness::from_name(word),
            _ => None,
        };
        let Some(harness) = harness else {
            let expected: Vec<&str> = HARNESSES.into_iter().map(Harness::as_str).collect();
            return Err(Refusal::new(
                "agent-harness",
                format!(
                    "unknown harness {}; expected {}",
                    stringified(input.get("harness")),
                    expected.join(", ")
                ),
            ));
        };
        let designer = input.get("designer");
        if designer.is_some_and(|designer| !designer.is_boolean()) {
            return Err(Refusal::new(
                "agent-designer",
                "an agent is an image agent (designer true) or not (false)",
            ));
        }
        if designer == Some(&Value::Bool(true)) && harness != Harness::Codex {
            return Err(Refusal::new(
                "agent-designer",
                "an image agent is a Codex agent: Codex's image tool draws",
            ));
        }
        match input.get("model") {
            Some(Value::String(model)) if !model.is_empty() => Ok((name, harness, model)),
            _ => Err(model_needed()),
        }
    }
}

/// The refusal of an agent with no model it could run.
pub(super) fn model_needed() -> Refusal {
    Refusal::new(
        "agent-model",
        "an agent needs a model (any identifier its harness accepts)",
    )
}

/// An effort the file would keep as something other than text, refused:
/// Node kept it, and a later read refused the file as no agents file (the
/// roster's rows hold their effort as text or not at all), so the request
/// is refused instead. Stricter than Node, decided with that rule.
pub(super) fn refuse_an_effort_that_is_no_text(effort: Option<&Value>) -> Result<(), Refusal> {
    match effort {
        None | Some(Value::Null | Value::String(_)) => Ok(()),
        Some(_) => Err(Refusal::new(
            "agent-effort",
            "an agent's effort is the name of a level, as text",
        )),
    }
}

/// `/^[a-z][a-z0-9-]*$/`.
fn is_agent_name(name: &str) -> bool {
    let mut characters = name.chars();
    characters
        .next()
        .is_some_and(|first| first.is_ascii_lowercase())
        && characters.all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        })
}

/// `name.charAt(0).toUpperCase() + name.slice(1)`, for a name that starts
/// with an ASCII letter.
fn capitalized(name: &str) -> String {
    let mut characters = name.chars();
    characters
        .next()
        .map(|first| first.to_ascii_uppercase().to_string() + characters.as_str())
        .unwrap_or_default()
}

/// `JSON.stringify(value)` in a sentence: `undefined` for a field not there.
fn stringified(value: Option<&Value>) -> String {
    value.map_or_else(|| "undefined".to_owned(), js::stringify)
}

#[cfg(test)]
mod tests;
