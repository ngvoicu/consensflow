//! The suggested staff (T-2405): the one staff ConsensFlow ships and is built with, which New
//! project and Project settings offer for one choice. Its data is `suggested-staff.json`, beside
//! this file, compiled into the daemon (`include_str!`), so each release carries the staff it was
//! made with and no file is read at run time. `harnesses.missing` sends it as `suggestedStaff`.
//!
//! The file is the page's to offer only where it is true: it is read by the rule below, and a file
//! that does not parse or breaks one rule is sent as `null`, which the page reads as "nothing to
//! offer". Whether each model is still listed by the person's installed harness is the page's
//! to check when its dialog opens, from the read it makes then (never from here, never kept).

use cf_ledger::model::{parse_effort, require_model_id};
use cf_ledger::roles;
use cf_proto::agents::Harness;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The file, as shipped.
const SHIPPED: &str = include_str!("suggested-staff.json");

/// The staff as the file writes it, and as the page reads it.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Suggestion {
    /// What the staff is for, in a line or two of plain words.
    about: String,
    members: Vec<Entry>,
    /// A figure for how well the staff does, measured elsewhere. Nothing reads it yet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    quality: Option<f64>,
    /// A figure for what the staff costs, measured elsewhere. Nothing reads it yet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cost: Option<f64>,
}

/// One suggested member: the roles it holds at one tier, or the image designer.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    harness: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    model: Option<String>,
    /// One of the efforts, or none where the model lists none.
    #[serde(default, skip_serializing_if = "effort_is_none")]
    effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tier: Option<String>,
    roles: Vec<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    designer: bool,
}

fn effort_is_none(effort: &Option<String>) -> bool {
    effort.is_none()
}

impl Entry {
    /// What makes two entries one member (one name): the harness, the model, the effort, the tier.
    fn key(&self) -> (&str, &str, &str, &str) {
        (
            &self.harness,
            self.model.as_deref().unwrap_or(""),
            self.effort.as_deref().unwrap_or(""),
            self.tier.as_deref().unwrap_or(""),
        )
    }

    fn check(&self) -> Result<(), String> {
        let harness = Harness::from_kind(&self.harness)
            .ok_or_else(|| format!("{:?} is no harness", self.harness))?;
        if self.roles.is_empty() {
            return Err("a member holds no role".to_owned());
        }
        for (at, id) in self.roles.iter().enumerate() {
            roles::find(id).ok_or_else(|| format!("{id:?} is no role"))?;
            if self.roles[..at].contains(id) {
                return Err(format!("{id} is named twice for one member"));
            }
        }
        let image = |id: &String| roles::find(id).is_some_and(|role| role.image_agent);
        if self.designer {
            let bare = self.model.is_none() && self.effort.is_none() && self.tier.is_none();
            if harness != Harness::Codex || !bare || !self.roles.iter().all(image) {
                return Err("the image designer is Codex's, with no model, effort or tier".into());
            }
            return Ok(());
        }
        if self.roles.iter().any(image) {
            return Err("only the image designer entry holds an image role".to_owned());
        }
        // A model id and an effort are held to the ledger's own rule for one: what Add member
        // would refuse is not offered.
        let model = self.model.as_deref().unwrap_or("");
        require_model_id(model).map_err(|why| why.to_string())?;
        parse_effort(self.effort.as_deref().map(Value::from).as_ref())
            .map_err(|why| why.to_string())?;
        let tier = self.tier.as_deref().unwrap_or("");
        for id in &self.roles {
            if !roles::offers(id, tier) {
                return Err(format!("{id} has no slot at the tier {tier:?}"));
            }
        }
        Ok(())
    }
}

impl Suggestion {
    fn check(&self) -> Result<(), String> {
        if self.about.trim().is_empty() {
            return Err("the staff says nothing of what it is for".to_owned());
        }
        if self.members.is_empty() {
            return Err("the staff has no member".to_owned());
        }
        for figure in [self.quality, self.cost].into_iter().flatten() {
            if !figure.is_finite() {
                return Err("a figure is not a number".to_owned());
            }
        }
        for (at, entry) in self.members.iter().enumerate() {
            entry.check()?;
            for other in &self.members[..at] {
                if other.key() == entry.key() {
                    return Err(format!(
                        "two entries are one member ({} {:?}): one entry holds its roles",
                        entry.harness, entry.model
                    ));
                }
                for id in &entry.roles {
                    if other.roles.contains(id) && other.tier == entry.tier {
                        return Err(format!("{id} has two members at one slot"));
                    }
                }
            }
        }
        Ok(())
    }
}

/// What `text` says, as the page reads it, or why it cannot be offered.
pub(super) fn read(text: &str) -> Result<Value, String> {
    let file: Value = serde_json::from_str(text).map_err(|why| why.to_string())?;
    // Named objects only: the structs below would also take a list in an object's place.
    let named = file.is_object()
        && file["members"]
            .as_array()
            .is_some_and(|members| members.iter().all(Value::is_object));
    if !named {
        return Err("the staff and each member are objects with named fields".to_owned());
    }
    let suggestion: Suggestion = serde_json::from_value(file).map_err(|why| why.to_string())?;
    suggestion.check()?;
    serde_json::to_value(&suggestion).map_err(|why| why.to_string())
}

/// What `harnesses.missing` sends of a file's text: its staff, or `null` where it cannot be
/// offered (the page then offers nothing).
fn offered(text: &str) -> Value {
    read(text).unwrap_or(Value::Null)
}

/// The text of the shipped file: the tests may put another in its place.
fn shipped_text() -> String {
    #[cfg(test)]
    if let Some(text) = seam::text() {
        return text;
    }
    SHIPPED.to_owned()
}

/// What `harnesses.missing` sends: the shipped staff, or `null`.
pub(super) fn shipped() -> Value {
    offered(&shipped_text())
}

/// The tests' way to put another file's text in the shipped one's place, on this thread.
#[cfg(test)]
pub(super) mod seam {
    use std::cell::RefCell;

    thread_local! {
        static TEXT: RefCell<Option<String>> = const { RefCell::new(None) };
    }

    pub(super) fn text() -> Option<String> {
        TEXT.with(|cell| cell.borrow().clone())
    }

    /// `text` as the shipped file on this thread until what this returns is dropped.
    pub(in crate::page) fn put(text: &str) -> Put {
        Put(TEXT.with(|cell| cell.replace(Some(text.to_owned()))))
    }

    pub(in crate::page) struct Put(Option<String>);

    impl Drop for Put {
        fn drop(&mut self) {
            TEXT.with(|cell| *cell.borrow_mut() = self.0.take());
        }
    }
}

#[cfg(test)]
mod tests;
