//! What Devin's wire log says of the notes, as the check's looks find it: the first
//! record that holds each note's header, what the record is, and when a look first
//! read it. The wire is what the TUI and its agent say to each other, in
//! `integrations/devin/<launch>/wire.jsonl` of the rig's home. Whether the TUI puts a
//! message typed in a turn to its agent at once (a request that holds its words) or
//! only when its agent runs it (the step that names it) is not recorded anywhere, and
//! is what the daemon's evidence of Devin's queue rests on: the run says.

use std::path::{Path, PathBuf};

use cf_e2e::live::seconds;
use serde_json::Value;

use crate::verdict::Note;

/// How a note was first named on the wire.
struct Named {
    note: i64,
    kind: String,
    at_ms: i64,
}

/// The wire's words about the notes, as far as the looks have read them.
#[derive(Default)]
pub struct Watch {
    named: Vec<Named>,
}

impl Watch {
    /// A look at the newest wire log of the Devin launches in `home`, at `now_ms`:
    /// each note it names that no look has found named is named, as its first record
    /// of it is.
    pub fn look(&mut self, home: &Path, notes: &[Note], now_ms: i64) {
        let Some(log) = newest(home).and_then(|file| std::fs::read(file).ok()) else {
            return;
        };
        let log = String::from_utf8_lossy(&log);
        for note in notes {
            if self.named.iter().any(|named| named.note == note.id) {
                continue;
            }
            let marker = note.marker();
            if let Some(line) = log.lines().find(|line| line.contains(&marker)) {
                self.named.push(Named {
                    note: note.id,
                    kind: kind_of(line),
                    at_ms: now_ms,
                });
            }
        }
    }

    /// What the wire said of each of `notes`, in words, by how long after its first
    /// paste (`pasted`: the milliseconds the daemon wrote, by the note's id) a look
    /// first read the record that names it.
    pub fn said(&self, notes: &[Note], pasted: impl Fn(i64) -> Option<i64>) -> Vec<String> {
        notes
            .iter()
            .map(|note| {
                let id = note.id;
                match self.named.iter().find(|named| named.note == id) {
                    Some(named) => match pasted(id) {
                        Some(first) => format!(
                            "m-{id} in the wire: first as {}, read {} s after its first paste",
                            named.kind,
                            seconds(named.at_ms - first)
                        ),
                        None => format!("m-{id} in the wire as {}, and never pasted", named.kind),
                    },
                    None => format!("m-{id} is named by no record of the wire"),
                }
            })
            .collect()
    }
}

/// The wire log of the newest launch of Devin in `home`.
fn newest(home: &Path) -> Option<PathBuf> {
    std::fs::read_dir(home.join("integrations").join("devin"))
        .ok()?
        .filter_map(std::result::Result::ok)
        .map(|launch| launch.path().join("wire.jsonl"))
        .filter_map(|file| {
            let modified = std::fs::metadata(&file).ok()?.modified().ok()?;
            Some((modified, file))
        })
        .max_by_key(|(modified, _)| *modified)
        .map(|(_, file)| file)
}

/// What a record of the wire is: the method a request makes, the kind of update it
/// carries, the steps the agent lists, or another.
fn kind_of(line: &str) -> String {
    let Ok(record) = serde_json::from_str::<Value>(line) else {
        return "unreadable".to_owned();
    };
    if let Some(method) = record["method"].as_str() {
        return format!("a request ({method})");
    }
    if let Some(update) = record["update"]["sessionUpdate"].as_str() {
        return format!("an update ({update})");
    }
    if record.get("steps").is_some() {
        return "the agent's steps".to_owned();
    }
    "another record".to_owned()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn notes() -> Vec<Note> {
        vec![
            Note {
                id: 2,
                what: "the cancel's note",
            },
            Note {
                id: 3,
                what: "the closed window's note",
            },
        ]
    }

    /// A home with the wire of a launch of Devin holding `lines`.
    fn home_with(lines: &[Value]) -> tempfile::TempDir {
        let home = tempfile::tempdir().unwrap();
        let launch = home.path().join("integrations").join("devin").join("one");
        std::fs::create_dir_all(&launch).unwrap();
        let text: String = lines.iter().map(|line| format!("{line}\n")).collect();
        std::fs::write(launch.join("wire.jsonl"), text).unwrap();
        home
    }

    #[test]
    fn a_note_is_named_by_the_first_record_that_holds_its_header_and_by_the_look_that_read_it() {
        let home = home_with(&[
            json!({ "sessionId": "s", "update": { "sessionUpdate": "config_option_update" } }),
            json!({ "jsonrpc": "2.0", "id": 2, "method": "session/prompt",
                "params": { "sessionId": "s", "prompt": [{ "text": "[ConsensFlow m-2 · T-1 · note]\nwords" }] } }),
        ]);
        let mut watch = Watch::default();
        watch.look(home.path(), &notes(), 10_000);
        // The next look finds m-3 named, and m-2 as it was.
        let launch = home.path().join("integrations").join("devin").join("one");
        let mut text = std::fs::read_to_string(launch.join("wire.jsonl")).unwrap();
        text.push_str(&format!(
            "{}\n",
            json!({ "sessionId": "s", "steps": [{ "summary": "[ConsensFlow m-3 · T-1 · note]" }] })
        ));
        text.push_str(&format!(
            "{}\n",
            json!({ "method": "session/prompt", "params": { "prompt": "[ConsensFlow m-2 · again]" } })
        ));
        std::fs::write(launch.join("wire.jsonl"), text).unwrap();
        watch.look(home.path(), &notes(), 70_000);
        let said = watch.said(&notes(), |id| Some(9_000 + id));
        assert_eq!(
            said,
            [
                "m-2 in the wire: first as a request (session/prompt), read 1.0 s after its first paste",
                "m-3 in the wire: first as the agent's steps, read 61.0 s after its first paste",
            ]
        );
    }

    #[test]
    fn a_note_the_wire_does_not_name_is_told_so_and_a_home_with_no_wire_names_none() {
        let home = home_with(&[json!({ "sessionId": "s", "update": { "sessionUpdate": "x" } })]);
        let mut watch = Watch::default();
        watch.look(home.path(), &notes(), 1);
        let said = watch.said(&notes(), |_| Some(0));
        assert_eq!(
            said,
            [
                "m-2 is named by no record of the wire",
                "m-3 is named by no record of the wire"
            ]
        );
        let nowhere = tempfile::tempdir().unwrap();
        let mut none = Watch::default();
        none.look(nowhere.path(), &notes(), 1);
        assert!(none.named.is_empty());
        assert_eq!(kind_of("not json"), "unreadable");
        assert_eq!(kind_of("{\"id\":4}"), "another record");
    }

    #[test]
    fn a_note_named_by_the_wire_that_the_daemon_never_pasted_is_told_so() {
        let home = home_with(&[json!({ "method": "x", "text": "[ConsensFlow m-2 · note]" })]);
        let mut watch = Watch::default();
        watch.look(home.path(), &notes()[..1], 5);
        assert_eq!(
            watch.said(&notes()[..1], |_| None),
            ["m-2 in the wire as a request (x), and never pasted"]
        );
    }
}
