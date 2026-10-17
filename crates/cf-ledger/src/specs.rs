//! A project's own word on specs (`cf_proto::specs`): whether its chief plans
//! big work by them, the folder they are kept in and the one Specmint variant
//! it plans with. The human says all three at once; what they leave out is not
//! said, and the project then follows what the app says for every project, and
//! where the app names no folder or variant, `.specs` and `core`.
//! Kept on the project's row, and logged as `project.specs` when it changes;
//! said as the project opens, it is kept in the write that makes the project.
//! The folder is the project's own, and is checked as the app's is
//! (`cf_proto::specs::check`): relative to the project, with nowhere to go but
//! down.

use cf_proto::ledger::ProjectView;
use cf_proto::specs::{check, SpecsView, Variant};
use rusqlite::params;
use serde_json::{json, Value};

use crate::model::{printed, LedgerError};
use crate::projects::known_project;
use crate::store::Store;

/// What the human says of a project's specs, each part checked: whether they
/// are on, the folder and the variant, each none to follow what the app says.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpecsChange {
    pub on: Option<bool>,
    pub folder: Option<String>,
    pub variant: Option<Variant>,
}

impl SpecsChange {
    /// The request as JSON gives it: `on` is true, false or null, `folder` is
    /// text (blank is none) or null, `variant` is one of the four names or
    /// null. A part the request does not have is as null.
    pub fn from_json(value: &Value) -> Result<Self, LedgerError> {
        Ok(Self {
            on: parse_on(value.get("on"))?,
            folder: parse_folder(value.get("folder"))?,
            variant: parse_variant(value.get("variant"))?,
        })
    }

    /// What a request says of specs as a project opens: none where it says
    /// nothing at all, else the three parts as [`from_json`](Self::from_json)
    /// reads them.
    pub fn from_opening(value: Option<&Value>) -> Result<Option<Self>, LedgerError> {
        match value {
            None | Some(Value::Null) => Ok(None),
            Some(said @ Value::Object(_)) => Self::from_json(said).map(Some),
            Some(other) => Err(LedgerError::refused(
                "invalid-specs",
                format!(
                    "specs are said as on, folder and variant, not {}",
                    printed(Some(other))
                ),
            )),
        }
    }

    /// What the project says once it is set.
    fn view(&self) -> Option<SpecsView> {
        SpecsView {
            on: self.on,
            folder: self.folder.clone(),
            variant: self.variant.map(|variant| variant.as_str().to_owned()),
        }
        .said()
    }
}

/// Whether specs are on: true, false, or none to follow what the app says.
fn parse_on(value: Option<&Value>) -> Result<Option<bool>, LedgerError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(on)) => Ok(Some(*on)),
        Some(_) => Err(LedgerError::refused(
            "invalid-specs",
            "specs are on (true), off (false) or follow the app's setting (null)",
        )),
    }
}

/// The variant: one of the four names, or none for `core`.
fn parse_variant(value: Option<&Value>) -> Result<Option<Variant>, LedgerError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(name)) => check::variant(name)
            .map(Some)
            .map_err(|words| LedgerError::refused("invalid-specs-variant", words)),
        Some(_) => Err(LedgerError::refused(
            "invalid-specs-variant",
            "the specs variant is a name",
        )),
    }
}

/// The folder, or none where it is blank: a folder of the project's, as the
/// ledger keeps it (`cf_proto::specs::check` says which are).
fn parse_folder(value: Option<&Value>) -> Result<Option<String>, LedgerError> {
    let refused = |words: String| LedgerError::refused("invalid-specs-folder", words);
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => check::folder(text).map_err(refused),
        Some(_) => Err(refused(check::folder_refused("it is text"))),
    }
}

/// `view` as an event says it: each part named, none as null.
fn said(view: Option<&SpecsView>) -> Value {
    json!({
        "on": view.and_then(|view| view.on),
        "folder": view.and_then(|view| view.folder.as_deref()),
        "variant": view.and_then(|view| view.variant.as_deref()),
    })
}

/// Sets what the project says of specs, all three parts at once, and logs it
/// where it changed.
pub(crate) fn set_specs(
    store: &mut Store,
    id: i64,
    change: &SpecsChange,
) -> Result<ProjectView, LedgerError> {
    store.write(|store| {
        keep_specs(store, id, change)?;
        known_project(store, id)
    })
}

/// What [`set_specs`] writes, inside the write of a caller that holds more than
/// the specs: the opening of a project keeps them in its own.
pub(crate) fn keep_specs(
    store: &mut Store,
    id: i64,
    change: &SpecsChange,
) -> Result<(), LedgerError> {
    let row = store.project_row(id)?;
    let from = row.specs_view();
    let to = change.view();
    let at = store.at();
    store.db.execute(
        "UPDATE project SET specs = ?, specs_folder = ?, specs_variant = ?, updated_at = ?
         WHERE id = ?",
        params![
            change.on.map(i64::from),
            change.folder,
            change.variant.map(Variant::as_str),
            at,
            id
        ],
    )?;
    if from != to {
        store.log(
            id,
            "project.specs",
            json!({ "from": said(from.as_ref()), "to": said(to.as_ref()) }),
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
