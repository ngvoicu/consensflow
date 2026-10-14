//! Devin's wire log said in runs of what its records are: what its TUI and its agent say
//! to each other, a JSON record a line, in `integrations/devin/<launch>/wire.jsonl` of a
//! run's home. A report of a window that did not do what it should ends with the last of
//! it: a log that goes on with message chunks is a turn that went on, and one that ends
//! with a tool's updates after `cause:complete` is a turn that ended with a command still
//! reporting.

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::members::plural;

/// What a record of the wire is: the method a request makes, the cause that ends a
/// turn, the kind of update it carries, or an error.
pub fn kind_of(record: &str) -> String {
    let Ok(record) = serde_json::from_str::<Value>(record) else {
        return "unreadable".to_owned();
    };
    if let Some(method) = record["method"].as_str() {
        return format!("method:{method}");
    }
    if let Some(cause) = record["cause"].as_str() {
        return format!("cause:{cause}");
    }
    if let Some(update) = record["update"]["sessionUpdate"].as_str() {
        return update.to_owned();
    }
    if record.get("error").is_some() {
        return "error".to_owned();
    }
    "other".to_owned()
}

/// How many runs of records a line of a report quotes, the last.
const RUNS: usize = 8;

/// The records as runs of their kinds: `agent_message_chunk ×412, cause:cancelled`.
pub fn runs(records: &[String]) -> String {
    let mut found: Vec<(String, usize)> = Vec::new();
    for record in records {
        let kind = kind_of(record);
        match found.last_mut() {
            Some((last, count)) if *last == kind => *count += 1,
            _ => found.push((kind, 1)),
        }
    }
    if found.is_empty() {
        return "nothing".to_owned();
    }
    let cut = found.len().saturating_sub(RUNS);
    let said: Vec<String> = found[cut..]
        .iter()
        .map(|(kind, count)| {
            if *count == 1 {
                kind.clone()
            } else {
                format!("{kind} ×{count}")
            }
        })
        .collect();
    format!("{}{}", if cut > 0 { "… " } else { "" }, said.join(", "))
}

/// The newest wire log of the Devin launches in `home`, as it is now: none where there
/// is no Devin launch.
pub fn newest(home: &Path) -> Option<PathBuf> {
    let launches = std::fs::read_dir(home.join("integrations").join("devin")).ok()?;
    launches
        .filter_map(std::result::Result::ok)
        .map(|launch| launch.path().join("wire.jsonl"))
        .filter_map(|file| {
            let modified = std::fs::metadata(&file).ok()?.modified().ok()?;
            Some((modified, file))
        })
        .max_by_key(|(modified, _)| *modified)
        .map(|(_, file)| file)
}

/// How the newest wire log of `home` ends: how many records it holds, and the runs of its
/// last. None where there is no Devin launch with a log.
pub fn tail(home: &Path) -> Option<String> {
    let bytes = std::fs::read(newest(home)?).ok()?;
    let records: Vec<String> = String::from_utf8_lossy(&bytes)
        .lines()
        .map(str::to_owned)
        .collect();
    Some(format!(
        "{}: {}",
        plural(records.len(), "wire record"),
        runs(&records)
    ))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn record(value: Value) -> String {
        value.to_string()
    }

    #[test]
    fn the_wire_is_said_in_runs_of_its_records_and_a_cancel_shows_as_one() {
        let chunk = record(json!({ "update": { "sessionUpdate": "agent_message_chunk" } }));
        let cancel = record(json!({ "method": "session/cancel", "sessionId": "s" }));
        let ended = record(json!({ "sessionId": "s", "cause": "cancelled" }));
        let wire = [
            chunk.clone(),
            chunk.clone(),
            chunk.clone(),
            cancel,
            chunk,
            ended,
            "not json".to_owned(),
            record(json!({ "error": { "code": 1 } })),
            record(json!({ "id": 3 })),
        ];
        assert_eq!(
            runs(&wire),
            "agent_message_chunk ×3, method:session/cancel, agent_message_chunk, cause:cancelled, unreadable, error, other"
        );
        assert_eq!(runs(&[]), "nothing");
    }

    #[test]
    fn a_long_wire_is_said_by_its_last_runs() {
        let kinds = ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"];
        let wire: Vec<String> = kinds
            .iter()
            .map(|kind| record(json!({ "update": { "sessionUpdate": kind } })))
            .collect();
        assert_eq!(runs(&wire), "… c, d, e, f, g, h, i, j");
    }

    #[test]
    fn the_wire_of_a_run_ends_with_what_a_command_left_reporting_after_its_turn_ended() {
        let home = tempfile::tempdir().unwrap();
        // No Devin launch, no wire.
        assert_eq!(tail(home.path()), None);
        let launch = home.path().join("integrations/devin/launch-1");
        std::fs::create_dir_all(&launch).unwrap();
        assert_eq!(tail(home.path()), None, "a launch that wrote no log");
        let chunk = record(json!({ "update": { "sessionUpdate": "agent_message_chunk" } }));
        let ended = record(json!({ "sessionId": "s", "cause": "complete" }));
        let update = record(json!({ "update": { "sessionUpdate": "tool_call_update" } }));
        let lines = [&chunk, &chunk, &ended, &update, &update, &update];
        let text: String = lines.iter().map(|line| format!("{line}\n")).collect();
        std::fs::write(launch.join("wire.jsonl"), text).unwrap();
        assert_eq!(
            tail(home.path()).as_deref(),
            Some("6 wire records: agent_message_chunk ×2, cause:complete, tool_call_update ×3")
        );
    }
}
