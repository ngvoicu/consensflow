//! What the daemon wrote as it went, in its event file (`events.jsonl` in its
//! home): one line for each event the ledger logged (`kind`, `data`) and each
//! change of a window's activity (`kind` `window.activity`, with the
//! `participant` and its `state`). The daemon holds its database itself while
//! it runs; this is its running copy, which a run reads again at each look,
//! oldest event first, and which tells in what order things happened.

use cf_e2e::files;
use cf_e2e::rig::Rig;
use serde_json::Value;

/// The events of a daemon so far.
pub struct Trace {
    events: Vec<Value>,
}

impl Trace {
    /// What the daemon of `rig` has written so far; a line that is not whole
    /// yet (the daemon is writing it) is left for the next read.
    pub fn read(rig: &Rig) -> Self {
        let text = files::read_string(&rig.home().join("events.jsonl")).unwrap_or_default();
        Self::of(&text)
    }

    /// The events of the file's `text`.
    pub fn of(text: &str) -> Self {
        Self {
            events: text
                .lines()
                .filter_map(|line| serde_json::from_str(line).ok())
                .collect(),
        }
    }

    /// How many events there are: a mark to ask what came after.
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// The events about the message `message`, with their places, in order.
    pub fn about(&self, message: i64) -> Vec<(usize, &Value)> {
        self.events
            .iter()
            .enumerate()
            .filter(|(_, event)| {
                event["data"]["message"] == message
                    || event["data"]["carried"]
                        .as_array()
                        .is_some_and(|carried| carried.iter().any(|id| *id == message))
            })
            .collect()
    }

    /// How many events of `kind` are about the message `message`.
    pub fn count(&self, kind: &str, message: i64) -> usize {
        self.about(message)
            .iter()
            .filter(|(_, event)| event["kind"] == kind)
            .count()
    }

    /// The place of the first event of `kind` about the message `message`.
    pub fn first(&self, kind: &str, message: i64) -> Option<usize> {
        self.about(message)
            .into_iter()
            .find(|(_, event)| event["kind"] == kind)
            .map(|(at, _)| at)
    }

    /// The state the window of `participant` was last said to be in before the
    /// event at `at`: none if it was said nothing of.
    pub fn activity_before(&self, participant: &str, at: usize) -> Option<&str> {
        self.events[..at.min(self.events.len())]
            .iter()
            .rev()
            .find(|event| event["kind"] == "window.activity" && event["participant"] == participant)
            .and_then(|event| event["state"].as_str())
    }

    /// Whether the window of `participant` went to work after the event at `at`.
    pub fn worked_after(&self, participant: &str, at: usize) -> bool {
        self.events.iter().skip(at).any(|event| {
            event["kind"] == "window.activity"
                && event["participant"] == participant
                && event["state"] == "working"
        })
    }

    /// The last `count` events, one to a line, for a report.
    pub fn tail(&self, count: usize) -> String {
        let skipped = self.events.len().saturating_sub(count);
        self.events
            .iter()
            .skip(skipped)
            .map(|event| {
                let kind = event["kind"].as_str().unwrap_or("?");
                let rest = match (event["participant"].as_str(), event["state"].as_str()) {
                    (Some(who), Some(state)) => format!("{who} {state}"),
                    _ => event["data"].to_string(),
                };
                format!("{kind} {rest}")
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOG: &str = concat!(
        r#"{"at":"t","kind":"window.activity","participant":"chief","state":"idle"}"#,
        "\n",
        r#"{"at":"t","kind":"message.sent","project":1,"data":{"message":7}}"#,
        "\n",
        r#"{"at":"t","kind":"window.activity","participant":"chief","state":"working"}"#,
        "\n",
        r#"{"at":"t","kind":"window.activity","participant":"chief","state":"idle"}"#,
        "\n",
        r#"{"at":"t","kind":"delivery.begun","project":1,"data":{"message":7,"attempt":1}}"#,
        "\n",
        r#"{"at":"t","kind":"delivery.confirmed","project":1,"data":{"message":9,"carried":[7]}}"#,
        "\n",
        "{\"at\":\"t\",\"kind\":\"delivery.begun\",\"pro",
    );

    #[test]
    fn the_events_of_the_file_are_read_a_whole_line_at_a_time() {
        let trace = Trace::of(LOG);
        assert_eq!(
            trace.len(),
            6,
            "the line the daemon is writing is not one yet"
        );
        assert_eq!(Trace::of("").len(), 0);
    }

    #[test]
    fn what_is_about_a_message_includes_what_it_rode_in_the_paste_of() {
        let trace = Trace::of(LOG);
        let kinds: Vec<&str> = trace
            .about(7)
            .iter()
            .map(|(_, event)| event["kind"].as_str().unwrap())
            .collect();
        assert_eq!(
            kinds,
            ["message.sent", "delivery.begun", "delivery.confirmed"]
        );
        assert_eq!(trace.count("delivery.begun", 7), 1);
        assert_eq!(trace.count("delivery.begun", 9), 0);
        assert_eq!(trace.first("delivery.begun", 7), Some(4));
        assert_eq!(trace.first("delivery.failed", 7), None);
    }

    #[test]
    fn a_windows_state_before_an_event_is_the_last_it_was_said_to_be_in() {
        let trace = Trace::of(LOG);
        assert_eq!(trace.activity_before("chief", 0), None);
        assert_eq!(trace.activity_before("chief", 2), Some("idle"));
        assert_eq!(trace.activity_before("chief", 3), Some("working"));
        assert_eq!(trace.activity_before("chief", 4), Some("idle"));
        assert_eq!(trace.activity_before("chief", 99), Some("idle"));
        assert_eq!(trace.activity_before("worker", 4), None);
    }

    #[test]
    fn a_window_went_to_work_after_a_mark_only_if_it_was_said_to_then() {
        let trace = Trace::of(LOG);
        assert!(trace.worked_after("chief", 0));
        assert!(trace.worked_after("chief", 2));
        assert!(!trace.worked_after("chief", 3));
        assert!(!trace.worked_after("worker", 0));
    }

    #[test]
    fn the_tail_is_a_line_to_an_event_with_who_and_what_or_its_data() {
        let tail = Trace::of(LOG).tail(3);
        assert_eq!(
            tail,
            "window.activity chief idle\n\
             delivery.begun {\"message\":7,\"attempt\":1}\n\
             delivery.confirmed {\"message\":9,\"carried\":[7]}"
        );
    }
}
