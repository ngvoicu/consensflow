//! Devin's wire log (`CHISEL_PURE_ACP_WIRE_LOG`): a JSON record per line of
//! what the stock TUI and its agent said to each other. It names the
//! conversation the window shows, since the TUI configures each conversation
//! it opens, at start and at every /new or /resume, and the permission mode the
//! conversation is in: the configuration's `mode` option, and each change of it
//! (`current_mode_update`).

use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Read};
use std::path::Path;

use cf_base::js;
use cf_base::json::from_slice_lossy;
use serde_json::Value;

/// A wire log that could not be read for the conversation it shows: not
/// there, or a complete record in it that is no record.
#[derive(Debug)]
pub(crate) struct Unreadable;

impl From<io::Error> for Unreadable {
    fn from(_: io::Error) -> Self {
        Unreadable
    }
}

/// The conversation one record says the window now shows; none when the
/// record says nothing of it.
pub(crate) fn shown_in(record: &Value) -> Result<Option<&str>, Unreadable> {
    if record.is_null() {
        return Err(Unreadable);
    }
    let Some(update) = record.get("update") else {
        return Ok(None);
    };
    if update.get("sessionUpdate").and_then(Value::as_str) != Some("config_option_update") {
        return Ok(None);
    }
    let mut configures_mode = false;
    match update.get("configOptions") {
        None | Some(Value::Null) => {}
        Some(Value::Array(options)) => {
            for option in options {
                if option.is_null() {
                    return Err(Unreadable);
                }
                if option.get("id").and_then(Value::as_str) == Some("mode") {
                    configures_mode = true;
                    break;
                }
            }
        }
        Some(_) => return Err(Unreadable),
    }
    Ok(configures_mode
        .then(|| record.get("sessionId").and_then(Value::as_str))
        .flatten())
}

/// The permission mode one record says the conversation `shown` is in: the
/// configuration's `mode` option where it has a value (`normal`, `bypass`…), or the
/// mode it changed to. None when the record says nothing of it, or is of another
/// conversation (a record that names none is the shown one's).
pub(crate) fn mode_in<'a>(record: &'a Value, shown: Option<&str>) -> Option<&'a str> {
    let of = record.get("sessionId").and_then(Value::as_str);
    if of.is_some() && of != shown {
        return None;
    }
    let update = record.get("update")?;
    match update.get("sessionUpdate").and_then(Value::as_str)? {
        "config_option_update" => update
            .get("configOptions")?
            .as_array()?
            .iter()
            .find(|option| option.get("id").and_then(Value::as_str) == Some("mode"))?
            .get("currentValue")?
            .as_str(),
        "current_mode_update" => update.get("currentModeId")?.as_str(),
        _ => None,
    }
    .filter(|mode| !mode.is_empty())
}

/// The conversation the window shows, as the log has it so far: the last
/// complete record that names one. The log is read a record at a time, as
/// long a conversation's log grows; what follows its last newline is a
/// record still being written, and is not read.
pub(crate) fn selected_session(file: &Path) -> Result<Option<String>, Unreadable> {
    let size = fs::metadata(file)?.len();
    let mut log = BufReader::new(File::open(file)?.take(size));
    let mut session = None;
    let mut line = Vec::new();
    loop {
        line.clear();
        log.read_until(b'\n', &mut line)?;
        let Some(record) = line.strip_suffix(b"\n") else {
            return Ok(session);
        };
        if js::trim(&String::from_utf8_lossy(record)).is_empty() {
            continue;
        }
        let record = from_slice_lossy(record).map_err(|_| Unreadable)?;
        if let Some(shown) = shown_in(&record)? {
            session = Some(shown.to_string());
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use serde_json::json;

    /// The line the wire log gains when its window configures a conversation it opens.
    pub(crate) fn shows(session: &str) -> String {
        let record = json!({
            "sessionId": session,
            "update": { "sessionUpdate": "config_option_update", "configOptions": [{ "id": "mode" }] },
        });
        format!("{record}\n")
    }

    #[test]
    fn a_record_names_the_conversation_only_when_it_configures_its_mode() {
        let record = json!({ "sessionId": "native-a",
            "update": { "sessionUpdate": "config_option_update", "configOptions": [{ "id": "model" }, { "id": "mode" }] } });
        assert_eq!(shown_in(&record).unwrap(), Some("native-a"));
        for silent in [
            json!({ "sessionId": "native-a", "update": { "sessionUpdate": "agent_message_chunk" } }),
            json!({ "sessionId": "native-a",
                "update": { "sessionUpdate": "config_option_update", "configOptions": [{ "id": "model" }] } }),
            json!({ "sessionId": 7,
                "update": { "sessionUpdate": "config_option_update", "configOptions": [{ "id": "mode" }] } }),
            json!({ "update": null }),
            json!(5),
        ] {
            assert_eq!(shown_in(&silent).unwrap(), None, "{silent}");
        }
        for unreadable in [
            json!(null),
            json!({ "update": { "sessionUpdate": "config_option_update", "configOptions": "mode" } }),
            json!({ "update": { "sessionUpdate": "config_option_update", "configOptions": [null] } }),
        ] {
            assert!(shown_in(&unreadable).is_err(), "{unreadable}");
        }
    }

    #[test]
    fn a_record_says_the_mode_its_conversation_is_in_by_its_configuration_or_its_change() {
        // As Devin 3000.11.92 wrote them (round 2 of the live suite of a resume,
        // 2026-10-15): the configuration whole at load, a change on its own after.
        let configured = json!({ "sessionId": "mild-coin", "update": {
            "sessionUpdate": "config_option_update",
            "configOptions": [
                { "id": "mode", "currentValue": "normal" },
                { "id": "model", "currentValue": "swe-2-high" },
            ] } });
        assert_eq!(mode_in(&configured, Some("mild-coin")), Some("normal"));
        let changed = json!({ "sessionId": "mild-coin",
            "update": { "sessionUpdate": "current_mode_update", "currentModeId": "bypass" } });
        assert_eq!(mode_in(&changed, Some("mild-coin")), Some("bypass"));
        // A record that names no conversation is the shown one's.
        let bare = json!({ "update": { "sessionUpdate": "current_mode_update", "currentModeId": "bypass" } });
        assert_eq!(mode_in(&bare, Some("mild-coin")), Some("bypass"));
        // Another conversation's mode is none of this one's.
        assert_eq!(mode_in(&changed, Some("fresh-leaf")), None);
        for silent in [
            // A configuration that names the mode and gives no value of it.
            json!({ "sessionId": "mild-coin",
                "update": { "sessionUpdate": "config_option_update", "configOptions": [{ "id": "mode" }] } }),
            json!({ "sessionId": "mild-coin",
                "update": { "sessionUpdate": "config_option_update",
                    "configOptions": [{ "id": "model", "currentValue": "swe-2-high" }] } }),
            json!({ "sessionId": "mild-coin",
                "update": { "sessionUpdate": "current_mode_update", "currentModeId": "" } }),
            json!({ "sessionId": "mild-coin", "update": { "sessionUpdate": "agent_message_chunk" } }),
            json!({ "sessionId": "mild-coin", "update": null }),
            json!(null),
        ] {
            assert_eq!(mode_in(&silent, Some("mild-coin")), None, "{silent}");
        }
    }

    #[test]
    fn selection_uses_complete_records_and_ignores_an_unfinished_last_one() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("wire.jsonl");
        fs::write(
            &file,
            format!("{}{}{{\"sessionId\":", shows("native-a"), shows("native-b")),
        )
        .unwrap();
        assert_eq!(
            selected_session(&file).unwrap().as_deref(),
            Some("native-b")
        );
        fs::write(
            &file,
            format!("{}\r\n\n{}", shows("native-a"), shows("native-b")),
        )
        .unwrap();
        assert_eq!(
            selected_session(&file).unwrap().as_deref(),
            Some("native-b")
        );
        fs::write(&file, "").unwrap();
        assert_eq!(selected_session(&file).unwrap(), None);
    }

    #[test]
    fn a_complete_record_that_is_no_json_or_no_file_is_unreadable() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("wire.jsonl");
        fs::write(
            &file,
            format!("{}{{invalid}}\n{}", shows("native-a"), shows("native-b")),
        )
        .unwrap();
        assert!(selected_session(&file).is_err());
        assert!(selected_session(&dir.path().join("gone.jsonl")).is_err());
    }
}
