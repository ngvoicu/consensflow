//! What a case looks at of a running rig besides what the daemon and the pane
//! host answer it: a window's screen as text, what the daemon wrote as it went,
//! the keys it pressed into a window and when, and a paste it catches; and the
//! human's typing into the chief's terminal.

use std::time::Duration;

use serde_json::{json, Map, Value};

use super::report::plain;
use super::route::{Held, Pasted};
use super::{Project, Rig, IDLE};
use crate::{files, Error, Result};

impl Rig {
    /// The daemon's next paste that holds `words` is caught: it goes as `pasted`
    /// says, into the window or nowhere, and the pane host's answer to it never
    /// reaches the daemon, which goes on waiting for it as one stopped between the
    /// paste and its receipt did. A catch set before and not yet made is replaced.
    pub fn catch_paste(&self, words: &str, pasted: Pasted) {
        self.shared.catch_paste(words, pasted);
    }

    /// From now on, while `refused`, every look the daemon asks of a window's screen
    /// (`pane.snapshot`) is refused, not ok, as a host that could not read it answers.
    pub fn refuse_snapshots(&self, refused: bool) {
        self.shared.refuse_snapshots(refused);
    }

    /// When the daemon told the page the board moved, in milliseconds since the
    /// epoch, oldest first: what the page reads the board again on.
    pub fn board_moves(&self) -> Vec<i64> {
        self.shared.seen().moves.clone()
    }

    /// Whether the paste the case set out to catch was caught.
    pub fn paste_caught(&self) -> bool {
        self.shared.paste_caught()
    }

    /// The pane host stalls at the daemon's next paste that holds `words`, for
    /// `lasting`: that paste and every frame between the daemon and the host after it,
    /// both ways, wait in the rig, and go on in the order they came once it has lasted,
    /// as with a host that answered nothing for that long and gave its window nothing
    /// (the Mac under a load of 100, 2026-10-22). The daemon's requests meanwhile meet
    /// their deadlines. A second paste of the same words ends it early, held with the
    /// rest: a daemon that took the first for lost has done what the stall was for,
    /// and the case need not wait out the rest. A stall set before and not yet begun
    /// is replaced.
    pub fn stall_host_at_paste(&self, words: &str, lasting: Duration) {
        self.shared.stall_at_paste(words, lasting);
    }

    /// The daemon's next `pane.open` of a window whose id begins with `pane` is held
    /// in the rig as `held` says (the request, before the pane host starts anything;
    /// or the host's answer, once it has started the program), until the case lets it
    /// go ([`Rig::let_open_go`]): an open that took longer than its deadline, as one did
    /// on the Mac under a load of 100 (kluris, 2026-10-26), with every other frame
    /// passing meanwhile. One set before and not yet begun is replaced.
    pub fn hold_open(&self, pane: &str, held: Held) {
        self.shared.hold_open(pane, held);
    }

    /// The window whose open is held (`id`, `generation`), while one is.
    pub fn open_held(&self) -> Option<Value> {
        self.shared.open_held()
    }

    /// The pane host's answer to the open held, once it came: its body (`ok`, the `pid`
    /// it started, or the `error` it refused it with).
    pub fn open_answer(&self) -> Option<Value> {
        self.shared.open_answer()
    }

    /// Lets the open held go on: the request to the pane host, or its answer to the
    /// daemon. Whether one was held.
    pub fn let_open_go(&self) -> bool {
        match (self.host.sink(), self.daemon.sink()) {
            (Some(host), Some(daemon)) => self.shared.let_open_go(&host, &daemon),
            _ => false,
        }
    }

    /// Whether the stall set out held a paste and is over.
    pub fn stall_over(&self) -> bool {
        self.shared.stall_over()
    }

    /// What the window `pane` printed so far, as the text on its screen: control
    /// sequences stripped, and Windows' console host's cursor moves left as the
    /// blanks and the line breaks they stand for, so that text reads the same on
    /// every platform. Nothing of a window that printed nothing, or was never
    /// opened. It is what was printed, not the screen as the terminal now draws it:
    /// what was drawn over is still in it. A window that printed more than the rig
    /// keeps (8 MiB) has its last 8 MiB or so in it, from the start of a line
    /// ([`Rig::screen_and_dropped`] counts what went before).
    pub fn screen(&self, pane: &str) -> String {
        let printed = self.printed(pane);
        plain(&printed)
    }

    /// What [`Rig::screen`] says of the window `pane`, and how many characters of its
    /// screen the rig let go of before it, its oldest: none until it printed more than
    /// the rig keeps. Read together, where a count of a screen's characters marks a
    /// place in it.
    pub fn screen_and_dropped(&self, pane: &str) -> (String, usize) {
        let (printed, dropped) = self
            .shared
            .seen()
            .printed
            .get(pane)
            .map_or_else(Default::default, |printed| {
                (printed.kept.clone(), printed.dropped)
            });
        (plain(&printed), dropped)
    }

    /// What the window `pane` printed so far, as the bytes the pane host gave and
    /// before anything is taken out of them: [`Rig::screen`] is these with the
    /// control sequences cleaned away. What tells a window that printed nothing
    /// from one that printed only what Windows' console host prints of its own
    /// accord (a jump of the cursor, a title), which `screen` reads as blank.
    /// Nothing of a window that printed nothing, or was never opened. The last 8
    /// MiB or so of it, as `screen`.
    pub fn printed(&self, pane: &str) -> Vec<u8> {
        self.shared
            .seen()
            .printed
            .get(pane)
            .map(|printed| printed.kept.clone())
            .unwrap_or_default()
    }

    /// What the pane host says of the window `pane` (`{ id, generation }`, as
    /// the board has it) now: its answer to `pane.snapshot`, which has how long
    /// the window printed nothing (`outputQuietMs`, null for a window that never
    /// did), whether a paste is on its way in and what was typed and not sent, and
    /// the last `tail` lines of its screen as the host draws it (`tail`). The host
    /// keeps the cells the program's text, cursor moves and erasures leave, so
    /// that is the screen a person sees, however the program or the console host
    /// repaints it. None where the host does not answer or says no: the pane is
    /// gone.
    pub fn snapshot(&self, pane: &Value, tail: usize) -> Option<Value> {
        let answer = self
            .host(
                "pane.snapshot",
                json!({ "id": pane["id"], "generation": pane["generation"], "tail": tail }),
            )
            .ok()?;
        (answer["ok"] == true).then_some(answer)
    }

    /// What the daemon wrote as it went, from its event file (`events.jsonl` in
    /// its home): one object for each event the ledger logged (`at`, `project`,
    /// `kind`, `data`) and each change of a window's activity (`kind`
    /// `window.activity`, with the `participant` and its `state`). The daemon
    /// holds the ledger itself while it runs, so this is its running copy, read
    /// again at each call, oldest first, each numbered (`n`) in the order it was
    /// written, the first being 1. A line the daemon is still writing is left
    /// for the next read.
    pub fn events(&self) -> Vec<Value> {
        let text = files::read_finished_lines(&self.home().join("events.jsonl"));
        events_of(&text.unwrap_or_default())
    }

    /// What the rig's stand-in `opencode` ([`Config::opencode`](super::Config::opencode),
    /// [`Config::opencode_2`](super::Config::opencode_2)) was asked so far, the
    /// arguments of each time it was started, oldest first: nothing where it never
    /// was, or the rig has none.
    pub fn opencode_asked(&self) -> Vec<Vec<String>> {
        self.opencode_lines()
            .into_iter()
            .filter_map(|line| serde_json::from_value(line).ok())
            .collect()
    }

    /// The requests the rig's stand-in OpenCode 1.x ([`Config::opencode`](super::Config::opencode))
    /// was sent so far on the servers it ran, oldest first: each its `method`, its
    /// `target` (the path and query) and its `body`, as text.
    pub fn opencode_requests(&self) -> Vec<Value> {
        self.opencode_lines()
            .into_iter()
            .filter(Value::is_object)
            .collect()
    }

    /// The lines of what the stand-in `opencode` wrote down, each as the JSON it is.
    fn opencode_lines(&self) -> Vec<Value> {
        let text = files::read_finished_lines(&self.root.join("opencode.jsonl"));
        text.unwrap_or_default()
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect()
    }

    /// When the daemon pressed the key `bytes` into the window `id`, in
    /// milliseconds since the epoch (the clock of the daemon's events and of a
    /// harness's status file), in order: the `pane.input` requests it made of the
    /// pane host that are exactly that.
    pub fn presses(&self, id: &str, bytes: &[u8]) -> Vec<i64> {
        self.shared
            .seen()
            .presses
            .iter()
            .filter(|press| press.pane == id && press.bytes == bytes)
            .map(|press| press.at_ms)
            .collect()
    }

    /// The environment of the daemon, the pane host and every window, as the
    /// rig made it: what a harness that is asked where a window's records are
    /// is told it has.
    pub fn environment(&self) -> &[(String, String)] {
        &self.env
    }

    /// The human types `bytes` into the window `pane` (`{ id, generation }`, as
    /// the board names it) as the app's page types them: the pane host takes
    /// them as the human's (`pane.typed`), and counts what they leave unsent in
    /// its input box. [`Rig::tell`] and [`Rig::host`]'s `pane.input` are keys the
    /// host takes as the daemon's.
    pub fn types(&self, pane: &Value, bytes: &[u8]) -> Result<()> {
        let answer = self.host(
            "pane.typed",
            json!({ "id": pane["id"], "generation": pane["generation"], "bytes": bytes }),
        )?;
        if answer["ok"] == true {
            Ok(())
        } else {
            Err(Error::Daemon(format!("pane.typed was refused: {answer}")))
        }
    }

    /// The human types into the chief's terminal, the one way work reaches the
    /// chief: a bracketed paste of `text`, then Enter, once the chief is idle (it
    /// is waited for a minute).
    pub fn tell(&self, project: i64, text: &str) -> Result<()> {
        self.tell_within(project, text, IDLE)
    }

    /// [`Rig::tell`], waiting up to `idle` for the chief to be idle: a chief on
    /// a model that takes its time to come up, or to finish what it was at.
    pub fn tell_within(&self, project: i64, text: &str, idle: Duration) -> Result<()> {
        // The first lane of the chief's, as the page draws it.
        let chief = || -> Result<Value> {
            let board = Project::new(self, project).board()?;
            Ok(board["lanes"]
                .as_array()
                .and_then(|lanes| {
                    lanes
                        .iter()
                        .find(|lane| lane["participant"]["handle"] == "chief")
                })
                .cloned()
                .unwrap_or(Value::Null))
        };
        self.wait_for("the chief to be idle", idle, || {
            Ok(chief()?["activity"]["state"] == "idle")
        })?;
        let pane = chief()?["pane"].clone();
        let typed = format!("\u{1b}[200~{text}\u{1b}[201~\r");
        let answer = self.host(
            "pane.input",
            json!({
                "id": pane["id"],
                "generation": pane["generation"],
                "bytes": typed.as_bytes(),
            }),
        )?;
        if answer["ok"] == true {
            Ok(())
        } else {
            Err(Error::Daemon(format!("pane.input was refused: {answer}")))
        }
    }
}

/// The events of the text of an event file: an object to a line, each with the
/// number `n` it has among them (the first is 1) before its own fields. A line
/// that is no JSON object is none.
pub(crate) fn events_of(text: &str) -> Vec<Value> {
    let mut events = Vec::new();
    for line in text.lines() {
        let Ok(Value::Object(fields)) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let mut numbered = Map::new();
        numbered.insert("n".to_owned(), json!(events.len() + 1));
        numbered.extend(fields);
        events.push(Value::Object(numbered));
    }
    events
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_events_of_a_file_are_numbered_in_the_order_written_and_a_torn_line_is_not_one() {
        let text = concat!(
            r#"{"at":"t","kind":"window.activity","participant":"chief","state":"idle"}"#,
            "\n",
            "not an event\n",
            "\n",
            r#"[1,2]"#,
            "\n",
            r#"{"at":"t","kind":"message.sent","project":1,"data":{"message":7}}"#,
            "\n",
            "{\"at\":\"t\",\"kind\":\"delivery.begun\",\"pro",
        );
        let events = events_of(text);
        let kinds: Vec<(&Value, &str)> = events
            .iter()
            .map(|event| (&event["n"], event["kind"].as_str().unwrap()))
            .collect();
        assert_eq!(
            kinds,
            [(&json!(1), "window.activity"), (&json!(2), "message.sent")]
        );
        // The number comes first, as `{ n, ...event }` puts it, and the event's own fields follow.
        assert!(events[0].to_string().starts_with(r#"{"n":1,"at":"t""#));
        assert!(events_of("").is_empty());
    }

    #[test]
    fn an_event_that_has_a_number_of_its_own_keeps_it() {
        // As `{ n, ...event }` leaves it: what the daemon wrote is the daemon's.
        let events = events_of("{\"n\":99,\"kind\":\"a\"}\n{\"kind\":\"b\"}\n");
        let numbers: Vec<Value> = events.iter().map(|event| event["n"].clone()).collect();
        assert_eq!(numbers, [json!(99), json!(2)]);
    }
}
