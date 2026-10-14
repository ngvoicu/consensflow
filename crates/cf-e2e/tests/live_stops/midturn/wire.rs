//! Devin's wire log of a window's launch: what its TUI and its agent say to each
//! other, a JSON record a line, in `integrations/devin/<launch>/wire.jsonl` of the
//! rig's home. It is where a stop that did not take shows what the keys did:
//! `method:session/cancel` is the TUI asking its agent to cancel, `cause:cancelled`
//! is the agent's turn ended by it, and a log that goes on with message chunks is a
//! turn that went on.

use std::path::{Path, PathBuf};

use cf_e2e::live::members::{plural, squash};
use cf_e2e::live::wire::{kind_of, newest, runs};
use serde_json::Value;

/// A wire log, and how many records it held at the pause.
pub struct Wire {
    file: PathBuf,
    before: usize,
}

impl Wire {
    /// The newest wire log of Devin's launches in the home, as it is now: none
    /// where there is no Devin launch.
    pub fn open(home: &Path) -> Option<Self> {
        let mut wire = Self {
            file: newest(home)?,
            before: 0,
        };
        wire.before = wire.records().len();
        Some(wire)
    }

    fn records(&self) -> Vec<String> {
        std::fs::read(&self.file)
            .map(|bytes| {
                String::from_utf8_lossy(&bytes)
                    .lines()
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// What it says since the pause, in runs of what its records are.
    pub fn since(&self) -> String {
        let records = self.records();
        let added = records.get(self.before..).unwrap_or_default();
        format!(
            "{} since the pause: {}",
            plural(added.len(), "wire record"),
            runs(added)
        )
    }

    /// What it says from its first record, in runs of what its records are.
    pub fn all(&self) -> String {
        let records = self.records();
        format!(
            "{}: {}",
            plural(records.len(), "wire record"),
            runs(&records)
        )
    }

    /// Whether a record of a tool call (the call, or its update) holds `text`: a prompt,
    /// which quotes what the member was asked, does not make a call.
    pub fn called(&self, text: &str) -> bool {
        self.records().iter().any(|record| {
            record.contains(text)
                && matches!(kind_of(record).as_str(), "tool_call" | "tool_call_update")
        })
    }

    /// Its first `most` records, each cut to `width` characters: what Devin said
    /// when it started, which a log that names no conversation is read by.
    pub fn head(&self, most: usize, width: usize) -> Vec<String> {
        self.records()
            .iter()
            .take(most)
            .map(|record| squash(record, width))
            .collect()
    }

    /// The conversation the window shows, as the log says it last: the session of the
    /// last record that configures the conversation's mode, which is how Devin names it.
    pub fn session(&self) -> Option<String> {
        self.records()
            .iter()
            .filter_map(|record| serde_json::from_str::<Value>(record).ok())
            .filter(|record| record["update"]["sessionUpdate"] == "config_option_update")
            .filter(|record| {
                record["update"]["configOptions"]
                    .as_array()
                    .is_some_and(|options| options.iter().any(|option| option["id"] == "mode"))
            })
            .filter_map(|record| record["sessionId"].as_str().map(str::to_owned))
            .next_back()
    }
}

/// Why there is no wire log to read in `home`, from what the folder of Devin's
/// launches holds: Devin was launched with none, or has written nothing of it.
pub fn missing(home: &Path) -> String {
    let folder = home.join("integrations").join("devin");
    let Ok(launches) = std::fs::read_dir(&folder) else {
        return "no Devin was launched: the home has no integrations/devin folder".to_owned();
    };
    let launches: Vec<_> = launches.filter_map(std::result::Result::ok).collect();
    if launches.is_empty() {
        return "no Devin was launched: integrations/devin holds no launch".to_owned();
    }
    format!(
        "{} of Devin, none with a wire.jsonl: it was told where to write one and wrote none",
        plural(launches.len(), "launch folder"),
    )
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn record(value: Value) -> String {
        value.to_string()
    }

    #[test]
    fn a_wire_is_told_of_since_the_pause_by_the_records_it_gained() {
        let folder = tempfile::tempdir().unwrap();
        let launch = folder
            .path()
            .join("integrations")
            .join("devin")
            .join("launch-1");
        std::fs::create_dir_all(&launch).unwrap();
        let file = launch.join("wire.jsonl");
        let chunk = record(json!({ "update": { "sessionUpdate": "agent_message_chunk" } }));
        std::fs::write(&file, format!("{chunk}\n{chunk}\n")).unwrap();
        let wire = Wire::open(folder.path()).unwrap();
        assert_eq!(wire.since(), "0 wire records since the pause: nothing");
        let cancel = record(json!({ "method": "session/cancel" }));
        std::fs::write(&file, format!("{chunk}\n{chunk}\n{chunk}\n{cancel}\n")).unwrap();
        assert_eq!(
            wire.since(),
            "2 wire records since the pause: agent_message_chunk, method:session/cancel"
        );
        // No Devin, no wire.
        assert!(Wire::open(&folder.path().join("nowhere")).is_none());
    }

    #[test]
    fn a_wire_is_told_of_from_its_start_by_its_runs_and_its_first_records_cut() {
        let folder = tempfile::tempdir().unwrap();
        let launch = folder
            .path()
            .join("integrations")
            .join("devin")
            .join("launch-1");
        std::fs::create_dir_all(&launch).unwrap();
        let chunk = record(json!({ "update": { "sessionUpdate": "agent_message_chunk" } }));
        let hello = record(json!({ "method": "initialize", "params": { "protocolVersion": 1 } }));
        std::fs::write(
            launch.join("wire.jsonl"),
            format!("{hello}\n{chunk}\n{chunk}\n"),
        )
        .unwrap();
        let wire = Wire::open(folder.path()).unwrap();
        assert_eq!(
            wire.all(),
            "3 wire records: method:initialize, agent_message_chunk ×2"
        );
        assert_eq!(wire.head(1, 200), std::slice::from_ref(&hello));
        assert_eq!(
            wire.head(1, 12),
            [hello.chars().take(12).collect::<String>()]
        );
        assert_eq!(wire.head(9, 200).len(), 3);
    }

    #[test]
    fn a_tool_is_called_when_a_record_of_a_call_holds_its_command_and_a_prompt_that_quotes_it_is_none(
    ) {
        let folder = tempfile::tempdir().unwrap();
        let launch = folder
            .path()
            .join("integrations")
            .join("devin")
            .join("launch-1");
        std::fs::create_dir_all(&launch).unwrap();
        let prompt = record(json!({
            "method": "session/prompt",
            "params": { "prompt": [{ "text": "Run exactly: ping -c 61 127.0.0.1" }] },
        }));
        let file = launch.join("wire.jsonl");
        std::fs::write(&file, format!("{prompt}\n")).unwrap();
        let wire = Wire::open(folder.path()).unwrap();
        assert!(!wire.called("-c 61 127.0.0.1"), "a prompt makes no call");
        let call = record(json!({
            "update": {
                "sessionUpdate": "tool_call",
                "rawInput": { "command": "ping -c 61 127.0.0.1" },
            },
        }));
        std::fs::write(&file, format!("{prompt}\n{call}\n")).unwrap();
        assert!(wire.called("-c 61 127.0.0.1"));
        assert!(!wire.called("sleep 119"));
        let update = record(json!({
            "update": { "sessionUpdate": "tool_call_update", "content": "sleep 119 ran" },
        }));
        std::fs::write(&file, format!("{update}\n")).unwrap();
        assert!(wire.called("sleep 119"));
    }

    #[test]
    fn a_home_with_no_wire_log_says_what_its_devin_folder_holds() {
        let folder = tempfile::tempdir().unwrap();
        assert_eq!(
            missing(folder.path()),
            "no Devin was launched: the home has no integrations/devin folder"
        );
        let launches = folder.path().join("integrations").join("devin");
        std::fs::create_dir_all(&launches).unwrap();
        assert_eq!(
            missing(folder.path()),
            "no Devin was launched: integrations/devin holds no launch"
        );
        std::fs::create_dir_all(launches.join("launch-1")).unwrap();
        std::fs::create_dir_all(launches.join("launch-2")).unwrap();
        assert_eq!(
            missing(folder.path()),
            "2 launch folders of Devin, none with a wire.jsonl: it was told where to write one and wrote none"
        );
    }

    #[test]
    fn the_conversation_a_window_shows_is_the_last_the_wire_configures_a_mode_for() {
        let folder = tempfile::tempdir().unwrap();
        let launch = folder
            .path()
            .join("integrations")
            .join("devin")
            .join("launch-1");
        std::fs::create_dir_all(&launch).unwrap();
        let configured = |session: &str| {
            record(json!({
                "sessionId": session,
                "update": {
                    "sessionUpdate": "config_option_update",
                    "configOptions": [{ "id": "model" }, { "id": "mode" }],
                },
            }))
        };
        let other = record(json!({
            "sessionId": "not-this",
            "update": { "sessionUpdate": "config_option_update", "configOptions": [{ "id": "model" }] },
        }));
        let chunk = record(json!({ "update": { "sessionUpdate": "agent_message_chunk" } }));
        let file = launch.join("wire.jsonl");
        std::fs::write(
            &file,
            format!("{}\n{other}\n{chunk}\n", configured("calm-river")),
        )
        .unwrap();
        let wire = Wire::open(folder.path()).unwrap();
        assert_eq!(wire.session().as_deref(), Some("calm-river"));
        // A /new or a /resume names another, and it is the one the window shows.
        std::fs::write(
            &file,
            format!(
                "{}\n{chunk}\n{}\n",
                configured("calm-river"),
                configured("brisk-ember")
            ),
        )
        .unwrap();
        assert_eq!(wire.session().as_deref(), Some("brisk-ember"));
        std::fs::write(&file, format!("{chunk}\n")).unwrap();
        assert_eq!(wire.session(), None);
    }
}
