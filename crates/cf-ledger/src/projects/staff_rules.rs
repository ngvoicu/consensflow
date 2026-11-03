//! The rules the human writes for a project's staff, in Project settings: plain
//! text every member window of the project is given after its role text, and the
//! chief's after its own, as what every member already has (the engine's
//! `roles::staff_rules`). They are kept on the project's row and logged when they
//! change (`project.staff_rules`, with what they were and what they became). Only
//! the page writes them: no route of the agents' API reaches them, so no `cf`
//! command, the chief's or a member's, changes them.
//!
//! The text is trimmed, and blank text is none: saving it takes the rules away.
//! It holds line breaks and tabs, and no other control character. It is at most
//! [`STAFF_RULES_MAX_BYTES`] bytes and [`STAFF_RULES_MAX_LINES`] lines, because
//! it goes into every window of the project and into every text that says a role
//! again (the engine's `role_change`), whose bounds it is sized to:
//!
//! - a member's role with the most rules goes into a window in one message (the
//!   16,000 units a paste is given whole), with room to spare;
//! - a chief's with specs on, a staff of 25 and the most rules is longer than one
//!   message already without them, and is printed by `cf inbox read` within what
//!   its window reads whole (`cf_base::role_read`: 29,000 bytes, 500 lines), so
//!   it is told once read.
//!
//! The engine's `role_change` tests hold both, with the most rules in the
//! characters that take the most bytes and in the most lines. The bound is in
//! bytes, not characters, because the read bound is: a letter of English is one
//! byte, most others two or three.

use cf_base::js;
use cf_proto::ledger::ProjectView;
use rusqlite::params;
use serde_json::{json, Value};

use super::known_project;
use crate::model::LedgerError;
use crate::store::Store;

/// The most bytes (of UTF-8) the staff's rules may have.
pub const STAFF_RULES_MAX_BYTES: usize = 8_000;

/// The most lines the staff's rules may have.
pub const STAFF_RULES_MAX_LINES: usize = 150;

fn rules_refused(why: &str) -> LedgerError {
    LedgerError::refused(
        "invalid-staff-rules",
        format!("the rules for the staff are plain text: {why}"),
    )
}

/// Whether `character` has no place in plain text: a control character other
/// than a line break or a tab.
fn not_plain(character: char) -> bool {
    character.is_control() && !matches!(character, '\n' | '\t')
}

/// `text` as the rules it makes: trimmed, and none where it is blank.
fn rules_of(text: &str) -> Result<Option<String>, LedgerError> {
    let text = js::trim(text);
    if text.is_empty() {
        return Ok(None);
    }
    if text.chars().any(not_plain) {
        return Err(rules_refused(
            "they have no control characters but line breaks and tabs",
        ));
    }
    let bytes = text.len();
    if bytes > STAFF_RULES_MAX_BYTES {
        return Err(rules_refused(&format!(
            "they are at most {STAFF_RULES_MAX_BYTES} bytes, and these are {bytes}"
        )));
    }
    let lines = text.split('\n').count();
    if lines > STAFF_RULES_MAX_LINES {
        return Err(rules_refused(&format!(
            "they are at most {STAFF_RULES_MAX_LINES} lines, and these are {lines}"
        )));
    }
    Ok(Some(text.to_owned()))
}

/// `value` read as the staff's rules: text, or none (a missing field, `null` or
/// blank text) to take them away.
pub fn parse_staff_rules(value: Option<&Value>) -> Result<Option<String>, LedgerError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => rules_of(text),
        Some(_) => Err(rules_refused("they are text, or none to take them away")),
    }
}

/// Writes the project's rules for its staff, or takes them away, and logs it
/// where they changed. The text is checked before the project is looked for.
pub(crate) fn set_staff_rules(
    store: &mut Store,
    id: i64,
    rules: Option<&str>,
) -> Result<ProjectView, LedgerError> {
    let to = rules.map(rules_of).transpose()?.flatten();
    store.write(|store| {
        let from = store.project_row(id)?.staff_rules;
        if from != to {
            let at = store.at();
            store.db.execute(
                "UPDATE project SET staff_rules = ?, updated_at = ? WHERE id = ?",
                params![to, at, id],
            )?;
            store.log(id, "project.staff_rules", json!({ "from": from, "to": to }))?;
        }
        known_project(store, id)
    })
}

#[cfg(test)]
mod tests;
