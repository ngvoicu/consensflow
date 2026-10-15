//! A window of a case with what it is watched by, and the wait for a turn: the
//! daemon's own way of telling that a message arrived in a window and that its turn
//! ended is its harness's record (the header of the message in a user's item, then
//! an assistant's item that is the turn's end, with the record at rest), and the
//! window is at rest once it has printed nothing for a while. Whatever the check
//! presses or pastes into the window is on its trail.

use std::fmt::Display;
use std::path::PathBuf;
use std::thread::sleep;
use std::time::{Duration, Instant};

use cf_e2e::live::ask::Record;
use cf_e2e::live::last_lines;
use cf_e2e::live::members::squash;
use cf_e2e::live::window::Window;
use cf_e2e::rig::Rig;
use cf_e2e::Result;
use serde_json::Value;

use crate::trail::Trail;
use crate::watch::Watch;

/// How often the window, its record and Devin's store are looked at while a turn is
/// waited for.
pub const LOOK: Duration = Duration::from_millis(500);

/// How long a window must have printed nothing to be at rest, in milliseconds: the
/// daemon's own (`ENTER_AGAIN_QUIET_MS` of its arrival watch).
pub const QUIET_MS: f64 = 3_000.0;

/// How long a window that prints nothing, in a turn that has not ended, is held by
/// something (a dialog that waits for an answer), in milliseconds; and how many rows
/// of its screen are looked at then.
const STALL_MS: f64 = 8_000.0;
const ROWS: usize = 16;

/// How long after its end a turn is over though its window goes on printing: a
/// status that moves at rest would keep a window from ever being quiet.
const OVER: Duration = Duration::from_secs(10);

/// A window of the case, and what it is watched by.
pub struct Scene<'a> {
    pub rig: &'a Rig,
    pub window: &'a Window<'a>,
    pub trail: Trail,
    pub watch: Watch,
}

/// How the wait for a turn ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Turn {
    /// The message showed in the window's record.
    pub arrived: bool,
    /// The turn it began ended: the record has the turn's end, and is at rest.
    pub ended: bool,
    /// The window ended before.
    pub closed: bool,
    pub took: Duration,
    /// Where the turn did not end, what the window showed.
    pub held: Option<String>,
}

/// Whether `header`, the start of a message's header, is in the record, and whether
/// the turn it began has ended: the record is at rest, and the last item that is not
/// a note of the harness's own is an assistant's, after the message. A record at rest
/// whose last item is an assistant's is complete, cancelled or failed: the daemon reads
/// it as over, and a window that has said nothing of a turn yet reads at rest with the
/// message as its last item, which is no end. The way the daemon confirms that a
/// message arrived, and reads a window at rest.
pub fn told(record: Option<&Record>, header: &str) -> (bool, bool) {
    let Some(record) = record else {
        return (false, false);
    };
    let Some(at) = record
        .items
        .iter()
        .rposition(|item| item.role == "user" && item.text.contains(header))
    else {
        return (false, false);
    };
    let last = record
        .items
        .iter()
        .rposition(|item| item.role != "custom")
        .unwrap_or_default();
    let ended = record.settled && last > at && record.items[last].role == "assistant";
    (true, ended)
}

/// How the daemon's own reader reads the record, as a line.
pub fn reading(record: Option<&Record>) -> String {
    let Some(record) = record else {
        return "the daemon's reader: no record".to_owned();
    };
    let last = record.items.last().map_or_else(
        || "nothing".to_owned(),
        |item| {
            let said = squash(&item.text, 60);
            format!(
                "{}: {}",
                item.role,
                if said.is_empty() { "(no text)" } else { &said }
            )
        },
    );
    format!(
        "the daemon's reader: {}, {}, {} items, last {last}",
        record.settlement,
        if record.settled {
            "at rest"
        } else {
            "not at rest"
        },
        record.items.len()
    )
}

/// The last `last` items of the record, as a line: who each is from and what it says
/// (an item that says nothing, a tool's call, is there as that).
pub fn tail(record: Option<&Record>, last: usize) -> String {
    let Some(record) = record else {
        return "no record".to_owned();
    };
    let items = &record.items;
    let said: Vec<String> = items[items.len().saturating_sub(last)..]
        .iter()
        .map(|item| {
            let words = squash(&item.text, 100);
            format!(
                "{}: {}",
                item.role,
                if words.is_empty() {
                    "(no text)"
                } else {
                    &words
                }
            )
        })
        .collect();
    format!("{} items; the last: {}", items.len(), said.join(" | "))
}

impl<'a> Scene<'a> {
    /// A window of the rig, watched from now on: Devin's store is `store`, and the
    /// wire log of the window's launch `wire`.
    pub fn new(
        rig: &'a Rig,
        window: &'a Window<'a>,
        store: PathBuf,
        wire: Option<PathBuf>,
    ) -> Self {
        Self {
            rig,
            window,
            trail: Trail::new(),
            watch: Watch::new(window, store, wire),
        }
    }

    /// What the pane host says of the window now: how long it printed nothing, what
    /// was typed and not sent.
    pub fn snapshot(&self) -> Option<Value> {
        self.rig.snapshot(&self.window.pane(), 1)
    }

    /// How long the window has printed nothing, in milliseconds: none for one that
    /// never printed.
    pub fn quiet_ms(&self) -> Option<f64> {
        self.snapshot()?["outputQuietMs"].as_f64()
    }

    /// Whether the window has printed nothing for [`QUIET_MS`].
    pub fn quiet(&self) -> bool {
        self.quiet_ms().is_some_and(|quiet| quiet >= QUIET_MS)
    }

    /// Looks at the window, Devin's store and the wire once.
    pub fn look(&mut self) {
        self.watch.look(self.window, &self.trail);
    }

    /// Pastes `text` as the daemon does, noting `what` on the trail first.
    pub fn paste(&mut self, text: &str, what: impl Display) -> Result<()> {
        self.trail.note(what);
        self.window.paste(text)?;
        Ok(())
    }

    /// Presses the key `bytes` as the daemon does, noting `what` on the trail first.
    pub fn press(&mut self, bytes: &[u8], what: impl Display) -> Result<()> {
        self.trail.note(what);
        self.window.press(bytes)?;
        Ok(())
    }

    /// What the window showed when a turn did not end: whether it printed, what was
    /// typed in it and not sent, what the daemon's reader says of the record, and the
    /// end of its screen.
    fn held(&self, record: Option<&Record>) -> String {
        let snapshot = self.snapshot();
        let quiet = snapshot
            .as_ref()
            .and_then(|snapshot| snapshot["outputQuietMs"].as_f64())
            .map_or_else(
                || "has printed nothing".to_owned(),
                |ms| format!("last printed {:.0} s ago", ms / 1000.0),
            );
        let unsent = snapshot
            .as_ref()
            .and_then(|snapshot| snapshot["unsent"].as_str())
            .filter(|unsent| !unsent.is_empty())
            .map_or_else(
                || "nothing typed and unsent".to_owned(),
                |unsent| format!("typed and unsent: {}", squash(unsent, 100)),
            );
        format!(
            "the window {quiet}; {unsent}; {}; its screen: {}",
            reading(record),
            self.window.tail()
        )
    }

    /// The end of the window's screen as the pane host draws it, as a line: what holds
    /// a window that waits for an answer (a dialog) is on it, in whatever words.
    fn rows(&self) -> String {
        let shown = self.rig.snapshot(&self.window.pane(), ROWS);
        let rows: Vec<&str> = shown
            .as_ref()
            .and_then(|shown| shown["tail"].as_array())
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        last_lines(&rows.join("\n"))
    }

    /// How the wait for a turn ended, as it is told by what was seen of the window.
    fn turned(&self, since: Instant, seen: (bool, bool, bool), record: Option<&Record>) -> Turn {
        let (arrived, ended, closed) = seen;
        Turn {
            arrived,
            ended,
            closed,
            took: since.elapsed(),
            held: (!ended).then(|| self.held(record)),
        }
    }

    /// Waits up to `within` for the message whose header starts with `header` to
    /// arrive in the window's record and its turn to end, looking at the window every
    /// [`LOOK`]; `keys` is asked after each look, with how long the wait has been and
    /// whether the message arrived, what the daemon does while it waits (press Enter
    /// once more, paste again) and what the case does to the window besides. A turn
    /// that has not ended while the window prints nothing for [`STALL_MS`] is held by
    /// something, and the screen it holds is noted. A turn that ended is over once the
    /// window is quiet, or [`OVER`] later whatever it prints.
    pub fn wait_turn(
        &mut self,
        header: &str,
        within: Duration,
        mut keys: impl FnMut(&mut Self, Duration, bool) -> Result<()>,
    ) -> Result<Turn> {
        let started = Instant::now();
        let mut ended_since: Option<Instant> = None;
        loop {
            sleep(LOOK);
            self.look();
            let record = self.window.record();
            let (arrived, ended) = told(record.as_ref(), header);
            let closed = self.window.closed();
            let quiet = self.quiet_ms();
            let seen = (arrived, ended, closed);
            if ended {
                let since = *ended_since.get_or_insert_with(Instant::now);
                if quiet.is_some_and(|ms| ms >= QUIET_MS) || since.elapsed() >= OVER {
                    return Ok(self.turned(started, seen, record.as_ref()));
                }
            } else {
                ended_since = None;
                if quiet.is_some_and(|ms| ms >= STALL_MS) {
                    let rows = self.rows();
                    self.watch.stalled(&rows, &self.trail);
                }
            }
            if closed || started.elapsed() >= within {
                return Ok(self.turned(started, seen, record.as_ref()));
            }
            keys(self, started.elapsed(), arrived)?;
        }
    }
}

#[cfg(test)]
mod tests {
    use cf_e2e::live::ask::Item;

    use super::*;

    fn item(role: &str, text: &str, complete: bool) -> Item {
        Item {
            id: String::new(),
            role: role.to_owned(),
            text: text.to_owned(),
            complete,
        }
    }

    fn record(items: Vec<Item>, settled: bool) -> Record {
        Record {
            items,
            settled,
            settlement: if settled { "settled" } else { "in-flight" }.to_owned(),
            failed: Value::Null,
            quota: Value::Null,
        }
    }

    #[test]
    fn a_message_has_arrived_when_its_header_is_in_a_users_item_and_its_turn_has_ended_after_it() {
        let header = "[ConsensFlow m-10 ";
        let asked = item(
            "user",
            "[ConsensFlow m-10 · T-1 · note from @chief]\nrun it",
            true,
        );
        assert_eq!(told(None, header), (false, false));
        assert_eq!(told(Some(&record(vec![], true)), header), (false, false));
        // Not in the record yet, or in the words of another role: not arrived.
        let other = [item("assistant", "[ConsensFlow m-10 ", true)];
        assert_eq!(
            told(Some(&record(other.to_vec(), true)), header),
            (false, false)
        );
        // In the record with nothing after it, though the record reads at rest: begun.
        assert_eq!(
            told(Some(&record(vec![asked.clone()], true)), header),
            (true, false)
        );
        // A record not at rest, whatever it ends in: going on.
        let going = vec![asked.clone(), item("assistant", "", false)];
        assert_eq!(told(Some(&record(going, false)), header), (true, false));
        let ended = vec![asked.clone(), item("assistant", "done", true)];
        assert_eq!(
            told(Some(&record(ended.clone(), false)), header),
            (true, false)
        );
        assert_eq!(told(Some(&record(ended, true)), header), (true, true));
        // At rest with an assistant's words that end no turn: a turn cancelled or failed,
        // which the daemon reads as over. A tool's call still waiting for its answer is not.
        let cut = vec![asked.clone(), item("assistant", "", false)];
        assert_eq!(told(Some(&record(cut, true)), header), (true, true));
        let waiting = vec![
            asked.clone(),
            item("assistant", "", false),
            item("tool", "", true),
        ];
        assert_eq!(told(Some(&record(waiting, true)), header), (true, false));
        // The harness's own notes after the end of a turn, or after the message alone,
        // are no part of either.
        let noted = vec![
            asked.clone(),
            item("assistant", "done", true),
            item("custom", "a note", true),
        ];
        assert_eq!(told(Some(&record(noted, true)), header), (true, true));
        let only_notes = vec![asked.clone(), item("custom", "a note", true)];
        assert_eq!(told(Some(&record(only_notes, true)), header), (true, false));
        // The last asking is the one that counts: a turn before it ended, this one has not.
        let again = vec![asked.clone(), item("assistant", "done", true), asked];
        assert_eq!(told(Some(&record(again, true)), header), (true, false));
        // m-1 is not found in m-10's header.
        let first = vec![
            item("user", "[ConsensFlow m-10 · x]", true),
            item("assistant", "ok", true),
        ];
        assert_eq!(
            told(Some(&record(first, true)), "[ConsensFlow m-1 "),
            (false, false)
        );
    }

    #[test]
    fn the_end_of_a_record_is_its_last_items_and_a_tools_call_that_says_nothing_is_one() {
        assert_eq!(tail(None, 3), "no record");
        let read = record(
            vec![
                item("user", "go", true),
                item("assistant", "", false),
                item("tool", "exit 0", true),
                item("assistant", "done  now", true),
            ],
            true,
        );
        assert_eq!(
            tail(Some(&read), 3),
            "4 items; the last: assistant: (no text) | tool: exit 0 | assistant: done now"
        );
        assert_eq!(tail(Some(&record(vec![], true)), 3), "0 items; the last: ");
    }

    #[test]
    fn the_daemons_reader_says_what_it_read_the_turn_as_and_the_last_item() {
        assert_eq!(reading(None), "the daemon's reader: no record");
        let read = record(
            vec![
                item("user", "hi", true),
                item("assistant", "hello  there", false),
            ],
            false,
        );
        assert_eq!(
            reading(Some(&read)),
            "the daemon's reader: in-flight, not at rest, 2 items, last assistant: hello there"
        );
        assert_eq!(
            reading(Some(&record(vec![], true))),
            "the daemon's reader: settled, at rest, 0 items, last nothing"
        );
        assert!(reading(Some(&record(vec![item("tool", "", true)], true)))
            .ends_with("last tool: (no text)"));
    }
}
