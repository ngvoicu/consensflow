//! What the human calls a project and says of it: the name, which they may
//! change (it has the rules it had when the project was opened), and a line of
//! plain text under it that is theirs alone: no agent is told it. Both are kept
//! on the project's row and logged when they change (`project.renamed`,
//! `project.described`, each with what it was and what it became); the
//! project's folder never changes.
//!
//! The line is one line: a line break, or any other control character, is
//! refused rather than turned into a space, so what is kept is what was asked
//! for, and a caller that sends one hears of it. The page's own field cannot
//! hold one. It is trimmed and at most [`DESCRIPTION_MAX`] characters (counted
//! as JavaScript counts them, as the page's field does), and blank text is none:
//! saving it takes the line away.

use cf_base::js;
use cf_base::text::utf16_len;
use cf_proto::ledger::ProjectView;
use rusqlite::params;
use serde_json::{json, Value};

use super::{known_project, require_name};
use crate::model::LedgerError;
use crate::store::Store;

/// The most characters a description may have.
const DESCRIPTION_MAX: usize = 200;

fn description_refused(why: &str) -> LedgerError {
    LedgerError::refused(
        "invalid-description",
        format!("a project's description is one line of plain text: {why}"),
    )
}

/// Whether `character` ends a line, or is otherwise no part of a line of text.
fn breaks_the_line(character: char) -> bool {
    character.is_control() || matches!(character, '\u{2028}' | '\u{2029}')
}

/// `text` as the description it makes: trimmed, and none where it is blank.
fn description_of(text: &str) -> Result<Option<String>, LedgerError> {
    let text = js::trim(text);
    if text.is_empty() {
        return Ok(None);
    }
    if utf16_len(text) > DESCRIPTION_MAX {
        return Err(description_refused(&format!(
            "it is at most {DESCRIPTION_MAX} characters"
        )));
    }
    if text.chars().any(breaks_the_line) {
        return Err(description_refused(
            "it has no line breaks or other control characters",
        ));
    }
    Ok(Some(text.to_owned()))
}

/// `value` read as a description: text, or none (a missing field, `null` or
/// blank text) to take it away.
pub fn parse_description(value: Option<&Value>) -> Result<Option<String>, LedgerError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => description_of(text),
        Some(_) => Err(description_refused("it is text, or none to take it away")),
    }
}

/// Gives the project another name, and logs it where it is another. The name
/// is checked before the project is looked for.
pub(crate) fn rename_project(
    store: &mut Store,
    id: i64,
    name: &str,
) -> Result<ProjectView, LedgerError> {
    require_name(name)?;
    store.write(|store| {
        let from = store.project_row(id)?.name;
        if from != name {
            let at = store.at();
            store.db.execute(
                "UPDATE project SET name = ?, updated_at = ? WHERE id = ?",
                params![name, at, id],
            )?;
            store.log(id, "project.renamed", json!({ "from": from, "to": name }))?;
        }
        known_project(store, id)
    })
}

/// Writes the line under the project's name, or takes it away, and logs it where
/// it changed. The text is checked before the project is looked for.
pub(crate) fn describe_project(
    store: &mut Store,
    id: i64,
    description: Option<&str>,
) -> Result<ProjectView, LedgerError> {
    let to = description.map(description_of).transpose()?.flatten();
    store.write(|store| {
        let from = store.project_row(id)?.description;
        if from != to {
            let at = store.at();
            store.db.execute(
                "UPDATE project SET description = ?, updated_at = ? WHERE id = ?",
                params![to, at, id],
            )?;
            store.log(id, "project.described", json!({ "from": from, "to": to }))?;
        }
        known_project(store, id)
    })
}

#[cfg(test)]
mod tests;
