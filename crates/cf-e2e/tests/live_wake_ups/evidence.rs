//! What a run reads of the daemon's event file and of the harness's own record
//! of the chief's conversation: the wake-ups the log says were queued and when,
//! what moved between two of them, the chief's native conversation, and what
//! the record holds of the words of a wake-up. Pure: the checks of [`verdict`]
//! are made of these, and they are tested here against events and records as
//! the daemon and the harnesses write them.
//!
//! [`verdict`]: crate::verdict

use cf_e2e::live::ask::Item;
use serde_json::Value;

/// A wake-up the log says was queued: its event's number in the file, when, and
/// the note it queued.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WakeUp {
    pub n: i64,
    pub at_ms: i64,
    pub message: i64,
}

/// The milliseconds since the epoch an event says it was logged at.
pub fn at_ms(event: &Value) -> Option<i64> {
    let at = event["at"].as_str()?;
    at.parse::<jiff::Timestamp>()
        .ok()
        .map(|stamp| stamp.as_millisecond())
}

/// The number of an event in the file, as the rig numbers them.
pub fn number(event: &Value) -> i64 {
    event["n"].as_i64().unwrap_or_default()
}

/// The wake-ups queued, oldest first.
pub fn wake_ups(events: &[Value]) -> Vec<WakeUp> {
    events
        .iter()
        .filter(|event| event["kind"] == "project.wake_up")
        .filter_map(|event| {
            Some(WakeUp {
                n: number(event),
                at_ms: at_ms(event)?,
                message: event["data"]["message"].as_i64()?,
            })
        })
        .collect()
}

/// When the setting last changed, and the number of its event: the interval
/// counts from it.
pub fn last_setting(events: &[Value]) -> Option<(i64, i64)> {
    events
        .iter()
        .rfind(|event| event["kind"] == "project.wake_up_setting")
        .and_then(|event| Some((number(event), at_ms(event)?)))
}

/// What moved in the log after the event numbered `after` and, when it is given,
/// before the one numbered `before`: the kinds of the task events and the
/// messages sent, but the notes of the wake-ups themselves, which are no
/// news to the chief.
pub fn moves(events: &[Value], after: i64, before: Option<i64>, wakes: &[WakeUp]) -> Vec<String> {
    events
        .iter()
        .filter(|event| number(event) > after && before.is_none_or(|before| number(event) < before))
        .filter(|event| {
            let kind = event["kind"].as_str().unwrap_or_default();
            let own_note = kind == "message.sent"
                && wakes
                    .iter()
                    .any(|wake| event["data"]["message"].as_i64() == Some(wake.message));
            (kind.starts_with("task.") || kind == "message.sent") && !own_note
        })
        .map(|event| event["kind"].as_str().unwrap_or_default().to_owned())
        .collect()
}

/// When the chief's window first read as `state` after the event numbered `after`.
pub fn chief_became(events: &[Value], state: &str, after: i64) -> Option<i64> {
    events
        .iter()
        .filter(|event| number(event) > after)
        .find(|event| {
            event["kind"] == "window.activity"
                && event["participant"] == "chief"
                && event["state"] == state
        })
        .and_then(at_ms)
}

/// When the chief's window last read at rest before the event numbered `before`:
/// where the turn that held a wake-up back ended.
pub fn chief_rested_before(events: &[Value], before: i64) -> Option<i64> {
    events
        .iter()
        .filter(|event| number(event) < before)
        .rfind(|event| {
            event["kind"] == "window.activity"
                && event["participant"] == "chief"
                && event["state"] == "idle"
        })
        .and_then(at_ms)
}

/// The conversation the chief's window is on in the harness's own record: the
/// newest conversation the log started for the chief, and the session it bound.
pub fn chief_session(events: &[Value]) -> Option<String> {
    let started = events.iter().rfind(|event| {
        event["kind"] == "conversation.started" && event["data"]["participant"] == "chief"
    })?["data"]["conversation"]
        .as_i64()?;
    events
        .iter()
        .rfind(|event| {
            event["kind"] == "conversation.bound" && event["data"]["conversation"] == started
        })
        .and_then(|event| event["data"]["nativeSession"].as_str().map(str::to_owned))
}

/// A user item's words as a record may have rewritten their line breaks.
fn plain(text: &str) -> String {
    text.replace("\r\n", "\n")
}

/// The positions, in the record, of the user items that hold `words` whole.
pub fn given(items: &[Item], words: &str) -> Vec<usize> {
    let words = plain(words);
    items
        .iter()
        .enumerate()
        .filter(|(_, item)| item.role == "user" && plain(&item.text).contains(&words))
        .map(|(at, _)| at)
        .collect()
}

/// Whether the chief said something after the item at `at`, before the next
/// thing the user said: an assistant item with words in it.
pub fn answered(items: &[Item], at: usize) -> bool {
    items
        .iter()
        .skip(at + 1)
        .take_while(|item| item.role != "user")
        .any(|item| item.role == "assistant" && !item.text.trim().is_empty())
}

/// The header a delivery of the note `message` opens with, which the record shows.
pub fn header(message: i64) -> String {
    format!("[ConsensFlow m-{message} ")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn event(n: i64, at: &str, kind: &str, data: Value) -> Value {
        json!({ "n": n, "at": at, "project": 1, "kind": kind, "data": data })
    }

    fn item(role: &str, text: &str) -> Item {
        Item {
            id: format!("{role}-{text}"),
            role: role.to_owned(),
            text: text.to_owned(),
            complete: true,
        }
    }

    #[test]
    fn the_wake_ups_are_the_events_of_the_kind_in_order_with_the_note_each_queued_and_its_time() {
        let events = [
            event(
                3,
                "2026-10-13T10:00:00.000Z",
                "project.wake_up_setting",
                json!({}),
            ),
            event(
                6,
                "2026-10-13T10:01:00.500Z",
                "project.wake_up",
                json!({ "message": 4 }),
            ),
            event(
                9,
                "2026-10-13T10:03:05.000Z",
                "project.wake_up",
                json!({ "message": 8 }),
            ),
            json!({ "n": 10, "kind": "project.wake_up", "data": { "message": 1 } }),
        ];
        let wakes = wake_ups(&events);
        assert_eq!(
            wakes,
            [
                WakeUp {
                    n: 6,
                    at_ms: 1_791_885_660_500,
                    message: 4
                },
                WakeUp {
                    n: 9,
                    at_ms: 1_791_885_785_000,
                    message: 8
                },
            ],
            "an event with no time is no wake-up"
        );
        assert_eq!(last_setting(&events), Some((3, 1_791_885_600_000)));
        assert_eq!(last_setting(&[]), None);
    }

    #[test]
    fn what_moved_is_the_tasks_and_the_messages_between_two_events_and_never_a_wake_ups_own_note() {
        let events = [
            event(
                1,
                "2026-10-13T10:00:00.000Z",
                "message.sent",
                json!({ "message": 4 }),
            ),
            event(
                2,
                "2026-10-13T10:00:01.000Z",
                "project.wake_up",
                json!({ "message": 4 }),
            ),
            event(
                3,
                "2026-10-13T10:00:02.000Z",
                "delivery.begun",
                json!({ "message": 4 }),
            ),
            event(4, "2026-10-13T10:00:03.000Z", "task.opened", json!({})),
            event(
                5,
                "2026-10-13T10:00:04.000Z",
                "message.sent",
                json!({ "message": 7 }),
            ),
            event(6, "2026-10-13T10:00:05.000Z", "window.activity", json!({})),
            event(7, "2026-10-13T10:00:06.000Z", "task.state", json!({})),
        ];
        let wakes = wake_ups(&events);
        assert_eq!(
            moves(&events, 0, None, &wakes),
            ["task.opened", "message.sent", "task.state"]
        );
        assert_eq!(
            moves(&events, 2, Some(7), &wakes),
            ["task.opened", "message.sent"]
        );
        assert!(moves(&events, 7, None, &wakes).is_empty());
    }

    #[test]
    fn the_chiefs_conversation_is_the_newest_the_log_started_for_it_with_the_session_it_bound() {
        let events = [
            event(
                1,
                "t",
                "conversation.started",
                json!({ "participant": "chief", "conversation": 1 }),
            ),
            event(
                2,
                "t",
                "conversation.bound",
                json!({ "conversation": 1, "nativeSession": "old" }),
            ),
            event(
                3,
                "t",
                "conversation.started",
                json!({ "participant": "zeus", "conversation": 2 }),
            ),
            event(
                4,
                "t",
                "conversation.started",
                json!({ "participant": "chief", "conversation": 3 }),
            ),
            event(
                5,
                "t",
                "conversation.bound",
                json!({ "conversation": 2, "nativeSession": "zeus" }),
            ),
        ];
        assert_eq!(chief_session(&events), None, "bound later, or not at all");
        let mut more = events.to_vec();
        more.push(event(
            6,
            "t",
            "conversation.bound",
            json!({ "conversation": 3, "nativeSession": "new" }),
        ));
        assert_eq!(chief_session(&more).as_deref(), Some("new"));
        assert_eq!(chief_session(&[]), None);
    }

    #[test]
    fn the_chiefs_turn_is_from_when_its_window_read_working_to_when_it_last_read_at_rest_before_a_wake_up(
    ) {
        let activity = |n: i64, at: &str, who: &str, state: &str| json!({ "n": n, "at": at, "kind": "window.activity", "participant": who, "state": state });
        let events = [
            activity(1, "2026-10-13T10:00:00.000Z", "chief", "idle"),
            activity(2, "2026-10-13T10:00:10.000Z", "chief", "working"),
            activity(3, "2026-10-13T10:00:11.000Z", "zeus", "idle"),
            activity(4, "2026-10-13T10:01:50.000Z", "chief", "idle"),
            event(
                5,
                "2026-10-13T10:01:51.000Z",
                "project.wake_up",
                json!({ "message": 9 }),
            ),
        ];
        assert_eq!(chief_became(&events, "working", 1), Some(1_791_885_610_000));
        assert_eq!(
            chief_became(&events, "working", 2),
            None,
            "after the event, not at it"
        );
        assert_eq!(chief_rested_before(&events, 5), Some(1_791_885_710_000));
        assert_eq!(
            chief_rested_before(&events, 4),
            Some(1_791_885_600_000),
            "zeus's is not the chief's"
        );
        assert_eq!(chief_rested_before(&events, 1), None);
    }

    #[test]
    fn the_words_are_given_where_a_user_item_holds_them_whole_whatever_its_line_breaks() {
        let words = "Anything left?\nSecond line.";
        let items = [
            item(
                "user",
                "[ConsensFlow m-4 · note from @human]\nAnything left?\r\nSecond line.",
            ),
            item("assistant", "No."),
            item("user", "Anything left?"),
            item("assistant", "Anything left?\nSecond line."),
        ];
        assert_eq!(given(&items, words), [0]);
        assert_eq!(given(&items, "Anything left?"), [0, 2]);
        assert!(given(&items, "nothing of the sort").is_empty());
    }

    #[test]
    fn the_chief_answered_when_an_assistant_item_with_words_follows_before_the_user_speaks_again() {
        let items = [
            item("user", "[ConsensFlow m-4 ·"),
            item("tool", "ls"),
            item("assistant", "  "),
            item("assistant", "Nothing is left."),
            item("user", "[ConsensFlow m-5 ·"),
        ];
        assert!(answered(&items, 0));
        assert!(!answered(&items, 4), "nothing follows the last");
        assert!(
            !answered(&items[..3], 0),
            "an item with no words is no answer"
        );
        let asked_twice = [item("user", "a"), item("user", "b"), item("assistant", "c")];
        assert!(
            !answered(&asked_twice, 0),
            "the next user item ends the look"
        );
        assert_eq!(header(12), "[ConsensFlow m-12 ");
    }
}
