//! A project's own word on specs (`cf_proto::specs`): whether its chief plans
//! big work by them, the folder they are kept in and the one Specmint variant
//! it plans with. The human says all three at once; what they leave out is not
//! said, and the project then follows the app's default, `.specs` and `core`.
//! Kept on the project's row, and logged as `project.specs` when it changes;
//! said as the project opens, it is kept in the write that makes the project.
//! The folder is the project's own, and is checked here: relative to the
//! project, with nowhere to go but down.

use cf_base::text::utf16_len;
use cf_proto::ledger::ProjectView;
use cf_proto::specs::{SpecsView, Variant};
use rusqlite::params;
use serde_json::{json, Value};

use crate::model::{printed, LedgerError};
use crate::projects::known_project;
use crate::store::Store;

/// The most characters a folder's name may have: far past any a person types.
const FOLDER_MAX: usize = 200;

/// What the human says of a project's specs, each part checked: whether they
/// are on (none follows the app's default), the folder (none is `.specs`) and
/// the variant (none is `core`).
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

/// Whether specs are on: true, false, or none to follow the app's default.
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
        Some(Value::String(name)) => Variant::from_name(name).map(Some).ok_or_else(|| {
            LedgerError::refused(
                "invalid-specs-variant",
                format!(
                    "the specs variant is one of {}, not {name:?}",
                    Variant::ALL.map(Variant::as_str).join(", ")
                ),
            )
        }),
        Some(_) => Err(LedgerError::refused(
            "invalid-specs-variant",
            "the specs variant is a name",
        )),
    }
}

/// The folder, or none where it is blank: a folder of the project's, as the
/// ledger keeps it.
fn parse_folder(value: Option<&Value>) -> Result<Option<String>, LedgerError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => folder_of(text),
        Some(_) => Err(folder_refused("it is text")),
    }
}

fn folder_refused(why: &str) -> LedgerError {
    LedgerError::refused(
        "invalid-specs-folder",
        format!("the specs folder is a folder inside the project: {why}"),
    )
}

/// `text` as the folder it names: its parts in slashes, with the `.` of a
/// `./` gone. It is a folder under the project's own (a path that is
/// absolute, on a drive or climbs out with `..` is refused, and so is the
/// project's folder itself), and none where it is blank.
fn folder_of(text: &str) -> Result<Option<String>, LedgerError> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    if utf16_len(text) > FOLDER_MAX {
        return Err(folder_refused(&format!(
            "its name is at most {FOLDER_MAX} characters"
        )));
    }
    if text.chars().any(char::is_control) {
        return Err(folder_refused("its name has no control characters"));
    }
    let slashed = text.replace('\\', "/");
    let bytes = slashed.as_bytes();
    let on_a_drive =
        bytes.first().is_some_and(u8::is_ascii_alphabetic) && bytes.get(1) == Some(&b':');
    if slashed.starts_with('/') || on_a_drive {
        return Err(folder_refused(
            "it is relative to the project, not absolute",
        ));
    }
    if slashed.contains(':') {
        return Err(folder_refused("its name has no `:`"));
    }
    let mut parts = Vec::new();
    for part in slashed.split('/').filter(|part| !matches!(*part, "" | ".")) {
        if part == ".." {
            return Err(folder_refused("it stays inside it, with no `..`"));
        }
        parts.push(part);
    }
    if parts.is_empty() {
        return Err(folder_refused(
            "it is a folder of the project's, not its own",
        ));
    }
    Ok(Some(parts.join("/")))
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
