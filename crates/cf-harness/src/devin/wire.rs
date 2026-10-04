//! Devin's wire log (`CHISEL_PURE_ACP_WIRE_LOG`): a JSON record per line of
//! what the stock TUI and its agent said to each other. It names the
//! conversation the window shows, since the TUI configures each conversation
//! it opens, at start and at every /new or /resume.

use std::fs::{self, File};
use std::io::{self, Read};
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

/// The conversation the window shows, as the log has it so far: the last
/// complete record that names one. What follows the last newline is a
/// record still being written, and is not read.
pub(crate) fn selected_session(file: &Path) -> Result<Option<String>, Unreadable> {
    let size = fs::metadata(file)?.len();
    let mut bytes = Vec::new();
    File::open(file)?.take(size).read_to_end(&mut bytes)?;
    let mut lines = bytes.split(|byte| *byte == b'\n');
    lines.next_back();
    let mut session = None;
    for line in lines {
        if js::trim(&String::from_utf8_lossy(line)).is_empty() {
            continue;
        }
        let record = from_slice_lossy(line).map_err(|_| Unreadable)?;
        if let Some(shown) = shown_in(&record)? {
            session = Some(shown.to_string());
        }
    }
    Ok(session)
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
