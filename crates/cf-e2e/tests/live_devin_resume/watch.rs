//! What each place Devin says anything in says of a tool call that did not run and of a
//! permission: the window's screen, Devin's store, and the wire log of the launch (the
//! TUI's words to its agent, and its agent's). A window is watched by looking at the
//! three, each read on from where the last look left it, and what is new is noted as
//! a sighting with the last thing the check did before it ([`Trail`]).

use std::collections::HashSet;
use std::path::PathBuf;

use cf_e2e::devin_store::{self, Message};
use cf_e2e::files;
use cf_e2e::live::members::squash;
use cf_e2e::live::window::Window;
use serde_json::Value;

use crate::tools::in_messages;
use crate::trail::{Sighting, Trail};
use crate::words::find_markers;

/// How much of what was seen a line quotes, and how much of a screen that holds a
/// window still.
const EXCERPT: usize = 170;
const STALLED: usize = 420;

/// What a place said: the words, and what is quoted of them.
pub type Said = (String, String);

/// The text around the byte `at` of `text`: a little before it and after.
fn around(text: &str, at: usize) -> String {
    let (mut from, mut to) = (at.saturating_sub(30), (at + EXCERPT).min(text.len()));
    while !text.is_char_boundary(from) {
        from -= 1;
    }
    while !text.is_char_boundary(to) {
        to += 1;
    }
    squash(&text[from..to], EXCERPT)
}

/// The markers a text holds, each with the words around it.
pub fn in_text(text: &str) -> Vec<Said> {
    find_markers(text)
        .into_iter()
        .map(|(marker, at)| (marker.to_owned(), around(text, at)))
        .collect()
}

/// What a record of the wire says: the markers it holds, a request for a
/// permission (its method names one), and the answer to a request that was asked
/// (a record of the same id with a result or an error). `asked` holds the ids of the
/// requests not answered yet.
pub fn in_wire(line: &str, asked: &mut HashSet<String>) -> Vec<Said> {
    let mut said = in_text(line);
    let Ok(record) = serde_json::from_str::<Value>(line) else {
        return said;
    };
    let id = record.get("id").map(Value::to_string);
    let asks = record["method"]
        .as_str()
        .is_some_and(|method| method.to_ascii_lowercase().contains("permission"));
    if asks {
        asked.extend(id);
        said.push(("permission request".to_owned(), squash(line, EXCERPT)));
    } else if let Some(id) = id.filter(|id| asked.contains(id)) {
        if let Some(answer) = record.get("result").or_else(|| record.get("error")) {
            asked.remove(&id);
            said.push((
                "permission answered".to_owned(),
                squash(&answer.to_string(), EXCERPT),
            ));
        }
    }
    said
}

/// What a window is watched by: its screen as it prints, the messages of its
/// session in Devin's store and the wire log of the launch, each read on from where
/// the last look left it.
pub struct Watch {
    store: PathBuf,
    wire: Option<PathBuf>,
    /// How much of the screen was read, in characters, and of the wire, in bytes.
    mark: usize,
    offset: usize,
    asked: HashSet<String>,
    seen: HashSet<String>,
    pub sightings: Vec<Sighting>,
}

impl Watch {
    /// The watch of `window`, in the store `store` and the wire log `wire` (none
    /// where the window has none), from now on.
    pub fn new(window: &Window, store: PathBuf, wire: Option<PathBuf>) -> Self {
        Self {
            store,
            wire,
            mark: window.mark(),
            offset: 0,
            asked: HashSet::new(),
            seen: HashSet::new(),
            sightings: Vec::new(),
        }
    }

    /// The messages of the window's session as the store has them now: none while
    /// the window has named none, or where the store cannot be read at the moment
    /// (its writer has it).
    pub fn messages(&self, window: &Window) -> Vec<Message> {
        window
            .session()
            .and_then(|session| devin_store::messages_of(&self.store, &session).ok())
            .unwrap_or_default()
    }

    /// Looks at the three places once, and notes what is new in them.
    pub fn look(&mut self, window: &Window, trail: &Trail) {
        let printed = window.screen_after(self.mark);
        self.mark += printed.chars().count();
        for (saw, excerpt) in in_text(&printed) {
            self.note("screen", saw, excerpt, trail);
        }
        for (id, (saw, excerpt)) in in_messages(&self.messages(window)) {
            if self.seen.insert(format!("{id}:{saw}")) {
                self.note("store", saw, excerpt, trail);
            }
        }
        for (saw, excerpt) in self.wired() {
            self.note("wire", saw, excerpt, trail);
        }
    }

    /// What a window shows while a turn that has not ended holds it still (`rows`: the
    /// end of its screen), noted once for each screen it holds: in whatever words a
    /// dialog that waits for an answer has them.
    pub fn stalled(&mut self, rows: &str, trail: &Trail) {
        if !rows.is_empty() && self.seen.insert(format!("stalled:{rows}")) {
            self.note(
                "screen",
                "stalled screen".to_owned(),
                squash(rows, STALLED),
                trail,
            );
        }
    }

    /// What the wire log says that is new: the records written whole since the last
    /// look.
    fn wired(&mut self) -> Vec<Said> {
        let Some(file) = &self.wire else {
            return Vec::new();
        };
        let Ok(bytes) = files::read(file) else {
            return Vec::new();
        };
        let fresh = bytes.get(self.offset..).unwrap_or_default();
        // A record still being written has no line end yet: it is read at the next look.
        let Some(end) = fresh.iter().rposition(|byte| *byte == b'\n') else {
            return Vec::new();
        };
        self.offset += end + 1;
        String::from_utf8_lossy(&fresh[..end])
            .lines()
            .flat_map(|line| in_wire(line, &mut self.asked))
            .collect()
    }

    fn note(&mut self, source: &'static str, saw: String, excerpt: String, trail: &Trail) {
        let at = trail.at();
        self.sightings.push(Sighting {
            source,
            saw,
            at,
            after: trail.before(at),
            excerpt,
        });
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// A watch of no window: only what its places have said is read.
    fn bare_watch(folder: &std::path::Path) -> Watch {
        Watch {
            store: folder.join("sessions.db"),
            wire: Some(folder.join("wire.jsonl")),
            mark: 0,
            offset: 0,
            asked: HashSet::new(),
            seen: HashSet::new(),
            sightings: Vec::new(),
        }
    }

    #[test]
    fn a_text_gives_each_marker_it_holds_with_the_words_around_it() {
        assert!(in_text("all well").is_empty());
        let screen = "output\nTool execution was rejected: User skipped this tool call\nmore";
        let said = in_text(screen);
        assert_eq!(said.len(), 2);
        assert_eq!(said[0].0, "User skipped this tool call");
        assert_eq!(
            said[0].1,
            "Tool execution was rejected: User skipped this tool call more"
        );
        // Words with a multi-byte character where the quote would be cut are no failure.
        let long = format!(
            "{}é User skipped this tool call é{}",
            "x".repeat(29),
            "é".repeat(300)
        );
        assert_eq!(in_text(&long).len(), 1);
    }

    #[test]
    fn the_wire_says_a_permission_was_asked_and_how_it_was_answered_by_the_id_of_the_request() {
        let mut asked = HashSet::new();
        let request = json!({
            "id": 7, "method": "session/request_permission",
            "params": { "toolCall": { "title": "run_command" },
                        "options": [{ "optionId": "allow" }, { "optionId": "reject" }] },
        })
        .to_string();
        let said = in_wire(&request, &mut asked);
        assert_eq!(said.len(), 1);
        assert_eq!(said[0].0, "permission request");
        assert!(said[0].1.contains("run_command"), "{said:?}");
        assert_eq!(asked.len(), 1);
        // Another record is no answer; the record of the same id is.
        let other = json!({ "id": 8, "result": { "ok": true } }).to_string();
        assert!(in_wire(&other, &mut asked).is_empty());
        let answer = json!({
            "id": 7, "result": { "outcome": { "outcome": "selected", "optionId": "reject" } },
        })
        .to_string();
        let said = in_wire(&answer, &mut asked);
        assert_eq!(said.len(), 1);
        assert_eq!(said[0].0, "permission answered");
        assert!(said[0].1.contains("reject"), "{said:?}");
        assert!(asked.is_empty());
        // A wire with no word of a permission in it says nothing, and a line that is
        // no JSON is read for the markers alone.
        let chunk = json!({ "update": { "sessionUpdate": "agent_message_chunk" } }).to_string();
        assert!(in_wire(&chunk, &mut asked).is_empty());
        assert_eq!(
            in_wire("not json: User skipped this tool call", &mut asked).len(),
            1
        );
    }

    #[test]
    fn a_screen_that_holds_a_window_still_is_noted_once_in_whatever_words_it_has() {
        let folder = tempfile::tempdir().unwrap();
        let mut watch = bare_watch(folder.path());
        let trail = Trail::new();
        watch.stalled("", &trail);
        assert!(watch.sightings.is_empty(), "no screen, nothing to note");
        let dialog = "Run command? ⏎ echo ran > shell.txt ⏎ > Yes No";
        watch.stalled(dialog, &trail);
        watch.stalled(dialog, &trail);
        assert_eq!(
            watch.sightings.len(),
            1,
            "the same screen is not noted again"
        );
        let noted = &watch.sightings[0];
        assert_eq!(
            (noted.source, noted.saw.as_str()),
            ("screen", "stalled screen")
        );
        assert_eq!(noted.excerpt, dialog);
        assert!(!noted.is_skip());
        // Another screen is another note.
        watch.stalled("Tool execution was rejected", &trail);
        assert_eq!(watch.sightings.len(), 2);
    }

    #[test]
    fn the_wire_is_read_on_from_where_it_was_and_a_record_being_written_waits_for_its_line_end() {
        // The part of the watch that needs no window: what the wire adds, a look at a time.
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join("wire.jsonl");
        let mut watch = bare_watch(folder.path());
        assert!(watch.wired().is_empty(), "no wire yet");
        let ask = json!({ "id": 1, "method": "session/request_permission" }).to_string();
        files::write(&file, format!("{ask}\n{{\"id\":1,\"res")).unwrap();
        assert_eq!(watch.wired().len(), 1);
        // The half record is finished and read at the next look, once.
        files::write(&file, format!("{ask}\n{{\"id\":1,\"result\":{{}}}}\n")).unwrap();
        let said = watch.wired();
        assert_eq!(said.len(), 1);
        assert_eq!(said[0].0, "permission answered");
        assert!(watch.wired().is_empty());
    }
}
