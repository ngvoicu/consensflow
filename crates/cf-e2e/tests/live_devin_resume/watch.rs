//! What each place Devin says anything in says of a tool call that did not run, of a
//! permission, and of the mode its session is in: the window's screen (with the marks
//! Devin's terminal puts on its output, [`Marks`]), Devin's store, and the wire log of
//! the launch (what its agent tells the TUI: [`crate::wire`]). A window is watched by
//! looking at the three, each read on from where the last look left it, and what is new
//! is noted as a sighting with the last thing the check did before it ([`Trail`]).

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

use cf_e2e::devin_store::{self, Message};
use cf_e2e::files;
use cf_e2e::live::members::squash;
use cf_e2e::live::window::Window;

use crate::states::Marks;
use crate::tools::in_messages;
use crate::trail::{Sighting, Trail};
use crate::wire::{in_wire, Modes, WireState};
use crate::words::find_markers;

/// How much of what was seen a line quotes, and how much of a screen that holds a
/// window still.
pub const EXCERPT: usize = 170;
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

/// What a window is watched by: its screen as it prints, the messages of its
/// session in Devin's store and the wire log of the launch, each read on from where
/// the last look left it.
pub struct Watch {
    store: PathBuf,
    wire: Option<PathBuf>,
    /// How much of the screen was read, in characters, and of the wire, in bytes.
    mark: usize,
    offset: usize,
    state: WireState,
    seen: HashSet<String>,
    pub sightings: Vec<Sighting>,
    /// What Devin's terminal said it was doing, as it changed.
    pub marks: Marks,
    /// What the wire said of the session's config and mode, as it changed.
    pub modes: Modes,
    /// The mode the wire last said the session is in, and the first it said.
    pub mode: Option<String>,
    pub first_mode: Option<String>,
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
            state: WireState::default(),
            seen: HashSet::new(),
            sightings: Vec::new(),
            marks: Marks::default(),
            modes: Modes::default(),
            mode: None,
            first_mode: None,
        }
    }

    /// How many prompts the TUI put to its agent so far, as its wire says: each a turn
    /// begun.
    pub fn prompts(&self) -> usize {
        self.state.prompts
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
        self.marks.feed(&printed, trail.at());
        for (saw, excerpt) in in_text(&printed) {
            self.note("screen", saw, excerpt, trail);
        }
        for (id, (saw, excerpt)) in in_messages(&self.messages(window)) {
            if self.seen.insert(format!("{id}:{saw}")) {
                self.note("store", saw, excerpt, trail);
            }
        }
        for (saw, excerpt) in self.wired(trail.at()) {
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

    /// What the wire log says that is new, at `at`: the records written whole since the
    /// last look. What it says of the config and the mode is kept, and the rest is given.
    fn wired(&mut self, at: Duration) -> Vec<Said> {
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
        let mut said = Vec::new();
        for line in String::from_utf8_lossy(&fresh[..end]).lines() {
            let wired = in_wire(line, &mut self.state);
            if let Some(config) = wired.config {
                self.modes.note(at, config);
            }
            if let Some(mode) = wired.mode {
                self.first_mode.get_or_insert_with(|| mode.clone());
                self.mode = Some(mode);
            }
            said.extend(wired.said);
        }
        said
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
    use serde_json::{json, Value};

    use super::*;

    /// A watch of no window: only what its places have said is read.
    fn bare_watch(folder: &std::path::Path) -> Watch {
        Watch {
            store: folder.join("sessions.db"),
            wire: Some(folder.join("wire.jsonl")),
            mark: 0,
            offset: 0,
            state: WireState::default(),
            seen: HashSet::new(),
            sightings: Vec::new(),
            marks: Marks::default(),
            modes: Modes::default(),
            mode: None,
            first_mode: None,
        }
    }

    fn at(seconds: u64) -> Duration {
        Duration::from_secs(seconds)
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
        assert!(watch.wired(at(1)).is_empty(), "no wire yet");
        let ask = json!({ "id": 1, "method": "session/request_permission" }).to_string();
        files::write(&file, format!("{ask}\n{{\"id\":1,\"res")).unwrap();
        assert_eq!(watch.wired(at(2)).len(), 1);
        // The half record is finished and read at the next look, once.
        files::write(&file, format!("{ask}\n{{\"id\":1,\"result\":{{}}}}\n")).unwrap();
        let said = watch.wired(at(3));
        assert_eq!(said.len(), 1);
        assert_eq!(said[0].0, "permission answered");
        assert!(watch.wired(at(4)).is_empty());
    }

    #[test]
    fn the_modes_the_wire_said_are_kept_with_when_and_the_last_one_is_the_mode() {
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join("wire.jsonl");
        let mut watch = bare_watch(folder.path());
        let line = |value: Value| format!("{value}\n");
        let config = |mode: &str| {
            line(json!({ "update": { "sessionUpdate": "config_option_update",
                "configOptions": [{ "id": "mode", "currentValue": mode }] } }))
        };
        files::write(&file, format!("{}{}", config("normal"), config("normal"))).unwrap();
        assert!(watch.wired(at(1)).is_empty());
        assert_eq!(watch.mode.as_deref(), Some("normal"));
        assert_eq!(watch.first_mode.as_deref(), Some("normal"));
        assert_eq!(watch.modes.len(), 1);
        files::write(
            &file,
            format!(
                "{}{}{}",
                config("normal"),
                config("normal"),
                config("bypass")
            ),
        )
        .unwrap();
        assert!(watch.wired(at(5)).is_empty());
        assert_eq!(watch.mode.as_deref(), Some("bypass"));
        assert_eq!(
            watch.first_mode.as_deref(),
            Some("normal"),
            "the first stays"
        );
        assert_eq!(
            watch.modes.since(0),
            "mode=normal +1.0 s, mode=bypass +5.0 s"
        );
        assert_eq!(watch.modes.since(1), "mode=bypass +5.0 s");
        assert_eq!(watch.modes.since(7), "no word of a config or a mode");
    }
}
