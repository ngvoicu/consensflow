//! What the pane host says of a window's end and of its screen, beyond which
//! pane it is: the exit code a program ended with and the last lines its
//! screen showed. The host sends them with `pane.exit`, and answers them to a
//! `pane.snapshot` that asks; the daemon keeps them for the failure note of a
//! window that did not come up.
//!
//! Every field is optional on the wire, and a body without one is a host that
//! does not say (an older one, or a test's): "does not say" is not "said
//! nothing". A screen that showed nothing is an empty `tail`.
//!
//! The host also says what a window's program started and left running: the
//! processes of the window's tree that began after the turn did, which
//! `pane.mark` fixes the start of and `pane.background` answers
//! ([`BackgroundProcess`]), and which `pane.exit` lists for a window that
//! closed with some of it running.

use serde::{Deserialize, Serialize};

/// The most lines of a screen the host sends as its tail, and so the most a
/// request for it gets. Empty lines are not counted: they are not sent.
pub const TAIL_LINES: usize = 12;

/// The most processes the host names when it says what a window left running
/// (it says how many there are besides): a note quotes a few, and a frame is
/// no place for a build's hundred processes.
pub const BACKGROUND_PROCESSES: usize = 16;
/// The most characters of a process's command line the host sends, a mark of
/// where it was cut at the end: a frame is no place for a build's whole one.
pub const COMMAND_CHARS: usize = 512;

/// A process the window's program started and left running past its turn: its
/// number, and the command line it was started with, as far as the host could
/// read it (`COMMAND_CHARS` at most; the program alone where it could read no more).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackgroundProcess {
    pub pid: u32,
    pub command: String,
}

/// `pane.background`'s answer: what the window's tree has running that began
/// after the window's last `pane.mark`, `total` of it and the first `processes`
/// of it by how long ago they began. A window with no mark has none: nothing
/// says where its turn began.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Background {
    pub total: usize,
    pub processes: Vec<BackgroundProcess>,
}

/// `pane.exit`'s body: the pane that ended, and what the host kept of its end.
/// The pane must be named; a field of the rest that is not what it should be
/// is a field the host did not say, and the exit is the exit all the same.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaneExit {
    pub id: String,
    pub generation: u64,
    /// The program's exit code, when the host could read it before it sent
    /// this. A program ended by a signal has the code 1 and its `signal`.
    #[serde(
        default,
        deserialize_with = "or_unsaid",
        skip_serializing_if = "Option::is_none"
    )]
    pub exit_code: Option<u32>,
    /// The signal that ended the program, by the name the system gives it.
    #[serde(
        default,
        deserialize_with = "or_unsaid",
        skip_serializing_if = "Option::is_none"
    )]
    pub signal: Option<String>,
    /// The last lines the window's screen showed, the empty ones left out, the
    /// oldest first.
    #[serde(
        default,
        deserialize_with = "or_unsaid",
        skip_serializing_if = "Option::is_none"
    )]
    pub tail: Option<Vec<String>>,
    /// What the window's program started and left running when the window
    /// closed, which the close ended: the processes of its tree that began after
    /// its last `pane.mark`. Empty for a window with none, and for a host that
    /// does not say.
    #[serde(
        default,
        deserialize_with = "or_none_said",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub background: Vec<BackgroundProcess>,
}

/// A list as it should be, or an empty one: what is not a list of `T` was not said.
fn or_none_said<'de, D, T>(field: D) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::de::DeserializeOwned,
{
    Ok(or_unsaid(field)?.unwrap_or_default())
}

/// A field as it should be, or none: what is not a `T` was not said.
fn or_unsaid<'de, D, T>(field: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::de::DeserializeOwned,
{
    let value = serde_json::Value::deserialize(field)?;
    Ok(serde_json::from_value(value).ok())
}

/// A `pane.snapshot` request. Without `tail` the answer is what it has always
/// been; with it, the answer also holds the screen's last `tail` lines, which
/// a snapshot polled every few moments does not carry unasked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotRequest {
    pub id: String,
    pub generation: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tail: Option<usize>,
}

/// The part of a `pane.snapshot` answer a request for the tail adds.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotTail {
    #[serde(
        default,
        deserialize_with = "or_unsaid",
        skip_serializing_if = "Option::is_none"
    )]
    pub tail: Option<Vec<String>>,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn an_exit_of_an_older_host_names_its_pane_and_says_nothing_else() {
        let exit: PaneExit =
            serde_json::from_value(json!({ "id": "p1-zeus", "generation": 4 })).unwrap();
        assert_eq!(
            exit,
            PaneExit {
                id: "p1-zeus".to_owned(),
                generation: 4,
                exit_code: None,
                signal: None,
                tail: None,
                background: Vec::new(),
            }
        );
        assert_eq!(
            serde_json::to_value(&exit).unwrap(),
            json!({ "id": "p1-zeus", "generation": 4 }),
            "what is not said is not written"
        );
    }

    #[test]
    fn an_exit_carries_its_code_its_signal_and_the_lines_its_screen_ended_with() {
        let body = json!({
            "id": "p1-zeus",
            "generation": 4,
            "exitCode": 3,
            "signal": "Hangup: 1",
            "tail": ["No API key found", "Use /login"],
        });
        let exit: PaneExit = serde_json::from_value(body.clone()).unwrap();
        assert_eq!(exit.exit_code, Some(3));
        assert_eq!(exit.signal.as_deref(), Some("Hangup: 1"));
        assert_eq!(
            exit.tail,
            Some(vec!["No API key found".to_owned(), "Use /login".to_owned()])
        );
        assert_eq!(serde_json::to_value(&exit).unwrap(), body);
    }

    #[test]
    fn an_exit_lists_what_the_window_left_running_and_an_older_host_lists_nothing() {
        let body = json!({
            "id": "p1-zeus",
            "generation": 4,
            "background": [{ "pid": 4242, "command": "python3 lock.py record --run 7" }],
        });
        let exit: PaneExit = serde_json::from_value(body.clone()).unwrap();
        assert_eq!(
            exit.background,
            vec![BackgroundProcess {
                pid: 4242,
                command: "python3 lock.py record --run 7".to_owned()
            }]
        );
        assert_eq!(serde_json::to_value(&exit).unwrap(), body);
        // Not a list of processes: not said, and the exit stands.
        let unsaid: PaneExit = serde_json::from_value(
            json!({ "id": "p", "generation": 1, "background": [{ "pid": "4242" }] }),
        )
        .unwrap();
        assert_eq!(unsaid.background, Vec::new());
    }

    #[test]
    fn a_screen_that_showed_nothing_is_an_empty_tail_and_not_an_absent_one() {
        let said: PaneExit =
            serde_json::from_value(json!({ "id": "p", "generation": 1, "tail": [] })).unwrap();
        assert_eq!(said.tail, Some(Vec::new()));
        assert_eq!(serde_json::to_value(&said).unwrap()["tail"], json!([]));
    }

    #[test]
    fn a_field_that_is_not_what_it_should_be_was_not_said_and_the_exit_stands() {
        let exit: PaneExit = serde_json::from_value(json!({
            "id": "p1-zeus",
            "generation": 4,
            "exitCode": "3",
            "signal": 9,
            "tail": ["a", 7],
        }))
        .unwrap();
        assert_eq!(
            exit,
            PaneExit {
                id: "p1-zeus".to_owned(),
                generation: 4,
                exit_code: None,
                signal: None,
                tail: None,
                background: Vec::new(),
            }
        );
        // The pane is the one thing an exit must name.
        assert!(serde_json::from_value::<PaneExit>(json!({ "generation": 4 })).is_err());
        assert!(
            serde_json::from_value::<PaneExit>(json!({ "id": "p", "generation": "4" })).is_err()
        );
        let answer: SnapshotTail =
            serde_json::from_value(json!({ "ok": true, "tail": "not a list" })).unwrap();
        assert_eq!(answer, SnapshotTail::default());
    }

    #[test]
    fn a_snapshot_asks_for_the_tail_only_when_it_wants_it() {
        let plain = SnapshotRequest {
            id: "p1-zeus".to_owned(),
            generation: 2,
            tail: None,
        };
        assert_eq!(
            serde_json::to_value(&plain).unwrap(),
            json!({ "id": "p1-zeus", "generation": 2 }),
            "the request the adapters make is as it was"
        );
        let asking = SnapshotRequest {
            tail: Some(TAIL_LINES),
            ..plain
        };
        assert_eq!(
            serde_json::to_value(&asking).unwrap(),
            json!({ "id": "p1-zeus", "generation": 2, "tail": TAIL_LINES })
        );
        assert!(serde_json::from_value::<SnapshotRequest>(
            json!({ "id": "p", "generation": 1, "lines": 3 })
        )
        .is_err());
    }

    #[test]
    fn an_answer_without_a_tail_is_a_host_that_did_not_say() {
        let answer: SnapshotTail =
            serde_json::from_value(json!({ "ok": true, "unsent": false })).unwrap();
        assert_eq!(answer, SnapshotTail::default());
        let shown: SnapshotTail =
            serde_json::from_value(json!({ "ok": true, "tail": ["a"] })).unwrap();
        assert_eq!(shown.tail, Some(vec!["a".to_owned()]));
    }
}
