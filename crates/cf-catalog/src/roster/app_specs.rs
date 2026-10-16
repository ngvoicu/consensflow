//! What the human says of specs for every project that does not say otherwise
//! (`cf_proto::specs::AppSpecs`): whether they are on, and the folder and the
//! Specmint variant a project uses when it names none. Each chief's launch reads
//! them, and the page alone sets them: they are no choice about the roster,
//! which the Agents screen sets.
//!
//! They are kept in the agents file where the released 3.0.4, which shares the
//! file, leaves them as they are. Whether they are on is `preferences.specs`
//! (only when on), which 3.0.4 reads and writes again. The folder and the variant
//! are a key of the file's own, `specsDefaults: {folder, variant}` (each only
//! where named, and the key only where one is): 3.0.4 sets `preferences` to an
//! object of its two choices at each save, and keeps every other key of the file
//! as it is. An earlier build of this one kept them in `preferences`
//! (`specsFolder`, `specsVariant`): they are read there where `specsDefaults`
//! does not say them, and moved at its next write. A file that never knew of them
//! reads, and is written, as it always did.

use std::path::Path;

use cf_base::refusal::Refusal;
use cf_proto::specs::{check, AppSpecs, Variant};
use serde_json::{Map, Value};

use super::document::{load_document, Document};
use super::preferences::{keep, kept, preferences_of};

/// The key of the file the folder and the variant are kept under.
const DEFAULTS: &str = "specsDefaults";

/// Where an earlier build of this one kept the folder and the variant, in
/// `preferences`: read where `specsDefaults` says nothing, never written.
pub(super) const EARLIER: [&str; 2] = ["specsFolder", "specsVariant"];

/// What the file says of specs for every project. Off unless it says `true`
/// itself, and no folder or variant unless it names a folder a project may keep
/// its specs in and a variant Specmint has: a file edited by hand to say anything
/// else reads as one that says nothing, so no launch is ever given a folder that
/// leaves its project.
pub(crate) fn app_specs_of(document: &Document) -> AppSpecs {
    let defaults = document.get(DEFAULTS);
    let text = |key: &str, earlier: &str| {
        defaults
            .and_then(|defaults| defaults.get(key))
            .or_else(|| kept(document, earlier))
            .and_then(Value::as_str)
    };
    AppSpecs {
        on: kept(document, "specs") == Some(&Value::Bool(true)),
        folder: text("folder", EARLIER[0]).and_then(|folder| check::folder(folder).ok().flatten()),
        variant: text("variant", EARLIER[1]).and_then(Variant::from_name),
    }
}

/// `app`'s folder and variant set as the file keeps them, under `specsDefaults`:
/// each only where it is named, beside what a later build keeps there, and the
/// key taken out where it holds nothing.
pub(super) fn keep_defaults(document: &mut Document, app: &AppSpecs) {
    let mut defaults = Map::new();
    if let Some(folder) = &app.folder {
        defaults.insert("folder".to_owned(), Value::String(folder.clone()));
    }
    if let Some(variant) = app.variant {
        defaults.insert(
            "variant".to_owned(),
            Value::String(variant.as_str().to_owned()),
        );
    }
    if let Some(Value::Object(before)) = document.get(DEFAULTS) {
        let later = before
            .iter()
            .filter(|(key, _)| !matches!(key.as_str(), "folder" | "variant"));
        defaults.extend(later.map(|(key, value)| (key.clone(), value.clone())));
    }
    if defaults.is_empty() {
        document.remove(DEFAULTS);
    } else {
        document.set(DEFAULTS, Value::Object(defaults));
    }
}

/// Sets what `said` says of specs for every project, as the page says it
/// (`{on, folder, variant}`): `on` is true or false, `folder` is text (blank is
/// none) or `null` to name none again, as a project's own folder is checked, and
/// `variant` is one of the four names or `null`. A part it leaves out is as it
/// was. Every part is checked, in that order, before anything is written; then
/// they are written, the human's choice about the roster as it was, and a file
/// is made where there is none.
pub(crate) fn set_app_specs(path: &Path, said: &Value) -> Result<AppSpecs, Refusal> {
    let mut document = load_document(path)?;
    let mut next = app_specs_of(&document);
    if let Some(on) = said.get("on") {
        next.on = on.as_bool().ok_or_else(|| {
            Refusal::new(
                "preference-specs",
                "specs for every project are on (true) or off (false)",
            )
        })?;
    }
    if let Some(folder) = said.get("folder") {
        next.folder = folder_said(folder)?;
    }
    if let Some(variant) = said.get("variant") {
        next.variant = variant_said(variant)?;
    }
    let preferences = preferences_of(&document);
    keep(path, &mut document, preferences, &next)?;
    Ok(next)
}

/// The folder every project keeps its specs in unless it names its own: text,
/// which is none where it is blank, or `null` to name none again.
fn folder_said(value: &Value) -> Result<Option<String>, Refusal> {
    let refused = |words: String| Refusal::new("preference-specs-folder", words);
    match value {
        Value::Null => Ok(None),
        Value::String(text) => check::folder(text).map_err(refused),
        _ => Err(refused(check::folder_refused("it is text"))),
    }
}

/// The variant every project plans with unless it names its own: one of the
/// four names, or `null` to name none again.
fn variant_said(value: &Value) -> Result<Option<Variant>, Refusal> {
    let refused = |words: String| Refusal::new("preference-specs-variant", words);
    match value {
        Value::Null => Ok(None),
        Value::String(name) => check::variant(name).map(Some).map_err(refused),
        _ => Err(refused("the specs variant is a name".to_owned())),
    }
}

#[cfg(test)]
mod tests;
