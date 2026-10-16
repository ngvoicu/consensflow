//! What the human says of specs for every project that does not say otherwise
//! (`cf_proto::specs::AppSpecs`): whether they are on, and the folder and the
//! Specmint variant a project uses when it names none. Kept in the agents file's
//! `preferences`, beside the human's choice about the roster, as `specs` (only
//! when on), `specsFolder` and `specsVariant` (each only when named), so a file
//! that never knew of them reads, and is written, as it always did. Each chief's
//! launch reads them, and the page alone sets them: they are no choice about the
//! roster, which the Agents screen sets.

use std::path::Path;

use cf_base::refusal::Refusal;
use cf_proto::specs::{check, AppSpecs, Variant};
use serde_json::Value;

use super::document::{load_document, Document};
use super::preferences::{keep, kept, preferences_of};

/// What the file says of specs for every project. Off unless it says `true`
/// itself, and no folder or variant unless it names a folder a project may keep
/// its specs in and a variant Specmint has: a file edited by hand to say anything
/// else reads as one that says nothing, so no launch is ever given a folder that
/// leaves its project.
pub(crate) fn app_specs_of(document: &Document) -> AppSpecs {
    let text = |key: &str| kept(document, key).and_then(Value::as_str);
    AppSpecs {
        on: kept(document, "specs") == Some(&Value::Bool(true)),
        folder: text("specsFolder").and_then(|folder| check::folder(folder).ok().flatten()),
        variant: text("specsVariant").and_then(Variant::from_name),
    }
}

/// Sets what `said` says of specs for every project, as the page says it
/// (`{on, folder, variant}`): `on` is true or false, `folder` is text (blank is
/// none) or `null` to name none again, as a project's own folder is checked, and
/// `variant` is one of the four names or `null`. A part it leaves out is as it
/// was. Every part is checked, in that order, before anything is written; then
/// the whole of `preferences` is written, the human's choice about the roster as
/// it was, and a file is made where there is none.
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
