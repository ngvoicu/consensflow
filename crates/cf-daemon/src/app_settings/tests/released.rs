//! The writer of the human's choices as the released 3.0.4 has it (`setPreferences` of
//! `crates/cf-catalog/src/roster/preferences.rs` at the tag `v3.0.4`), trimmed to what a
//! test asks of it: a patch of the two switches, each a flag. It sets `preferences` to an
//! object of its two choices, `specs` only when on, and keeps every other key of the file
//! as it is. A fixture for what becomes of a file this build wrote once 3.0.4, which shares
//! it, has saved it.

use std::path::Path;

use serde_json::{Map, Value};

use super::super::{read_file, save};

/// 3.0.4's `Preferences`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Choices {
    pub(super) own_harness_only: bool,
    pub(super) specs: bool,
}

/// The choices the file records: one it does not record, or that is anything but
/// `true`, is off.
pub(super) fn preferences_of(file: &Map<String, Value>) -> Choices {
    let chosen = |key: &str| {
        file.get("preferences")
            .and_then(|preferences| preferences.get(key))
            == Some(&Value::Bool(true))
    };
    Choices {
        own_harness_only: chosen("ownHarnessOnly"),
        specs: chosen("specs"),
    }
}

/// `setPreferences` of a patch of the two switches: the whole set kept in `preferences`.
pub(super) fn set_preferences(path: &Path, patch: &Value) -> Choices {
    let mut file = read_file(path).unwrap();
    let mut next = preferences_of(&file);
    for (key, value) in patch.as_object().unwrap() {
        let on = value.as_bool().unwrap();
        match key.as_str() {
            "specs" => next.specs = on,
            "ownHarnessOnly" => next.own_harness_only = on,
            other => panic!("3.0.4 has no preference named {other}"),
        }
    }
    let mut kept = Map::from_iter([(
        "ownHarnessOnly".to_owned(),
        Value::Bool(next.own_harness_only),
    )]);
    if next.specs {
        kept.insert("specs".to_owned(), Value::Bool(true));
    }
    file.insert("preferences".to_owned(), Value::Object(kept));
    save(path, &file).unwrap();
    next
}
