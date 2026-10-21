//! What a stop case sees of the Claude window it stops: its status as the daemon
//! reads it, the Escapes the daemon presses into it, what Claude kept of the
//! conversation, and what the daemon wrote of the stop. The reading of each is a
//! plain function of what was seen, which is what the cases judge by.

use cf_e2e::live::ask::{self, Record};
use cf_e2e::live::claude::{self, Entry, Status};
use cf_e2e::live::members::{clock, plus_maybe, squash, Account, MemberWindow, Members};
use cf_e2e::{Error, Result};
use serde_json::{json, Map, Value};

/// A member's window and what is watched of it from here on: its status, which is
/// read every 150 ms, and (by the rig) the Escapes the daemon presses.
pub struct Watched {
    pub window: MemberWindow,
    pub session: String,
    pub status: Status,
}

/// Watches the window of the member `agent`, which has opened by now.
pub fn watch(members: &Members, agent: &str) -> Result<Watched> {
    let window = members
        .window_of(agent)?
        .ok_or_else(|| Error::Daemon(format!("the member {agent} has no window to watch")))?;
    let session = window.session().ok_or_else(|| {
        Error::Daemon(format!(
            "the window of {agent} was opened on no conversation"
        ))
    })?;
    let status = Status::watch(&members.claude_config(), &session);
    Ok(Watched {
        window,
        session,
        status,
    })
}

impl Watched {
    /// When the daemon pressed Escape into the window, each press, in
    /// milliseconds since the epoch.
    pub fn escapes(&self, members: &Members) -> Vec<i64> {
        members.rig().presses(self.window.id(), &[27])
    }

    /// The end of its screen, as a line.
    pub fn screen(&self, members: &Members) -> String {
        members.tail(self.window.id())
    }

    /// What Claude kept of its conversation so far.
    pub fn record(&self, members: &Members) -> Vec<Entry> {
        claude::record(&members.claude_config(), &self.session)
    }
}

/// How the daemon's record reader (`cf_harness::records`, before the Claude adapter
/// reads the window by more than its record) reads the window's transcript now. The
/// native daemon's look at the window needs Claude's status idle and the record
/// settled, as this reader says, but it departs from it in two cases, and reads
/// the window at rest where this one says "not settled": an interrupt record that
/// names no message (the `slowhook` case), and a turn the daemon's own press stopped
/// before Claude wrote a word (the `early` case, which has no record of the stop).
/// The daemon's own reading is the `window.activity` line of each case.
pub fn settlement_line(members: &Members, session: &str) -> String {
    let read = ask::records("claude-code", session, members.rig().environment())
        .ok()
        .flatten();
    settlement_of(read.as_ref())
}

/// The line that says what the record reader made of a record.
pub fn settlement_of(read: Option<&Record>) -> String {
    let Some(read) = read else {
        return "the record reader finds no transcript".to_owned();
    };
    let last = read
        .items
        .last()
        .map(|item| format!("{}: {}", item.role, squash(&item.text, 60)));
    format!(
        "the record reader (cf_harness::records, before the adapter's own look) reads its transcript as {} (settlement {}; {} items, the last {})",
        if read.settled { "SETTLED" } else { "NOT settled" },
        read.settlement,
        read.items.len(),
        last.map_or_else(|| "null".to_owned(), |last| Value::String(last).to_string())
    )
}

/// The last `last` records of the conversation, as lines with their time
/// relative to `from`; the first line says how many records it has.
pub fn record_since(items: &[Entry], session: &str, from: i64, last: usize) -> Vec<String> {
    let mut lines = vec![format!(
        "{} records in the transcript of {session}",
        items.len()
    )];
    lines.extend(
        items
            .iter()
            .skip(items.len().saturating_sub(last))
            .map(|item| {
                format!(
                    "{} {}: {}",
                    plus_maybe(item.at_ms, from),
                    item.kind,
                    squash(&item.text, 220)
                )
            }),
    );
    lines
}

/// The last `last` records the transcript holds, each with the start of its line:
/// what Claude wrote of a stop.
pub fn raw_tail(items: &[Entry], from: i64, last: usize) -> Vec<String> {
    items
        .iter()
        .skip(items.len().saturating_sub(last))
        .map(|item| {
            format!(
                "{} {}",
                plus_maybe(item.at_ms, from),
                squash(&item.raw, 260)
            )
        })
        .collect()
}

/// The window's interrupt, as Claude's record writes it, if it wrote one since `from`.
pub fn interrupted(items: &[Entry], from: i64) -> Option<&Entry> {
    items.iter().find(|item| {
        item.at_ms.is_some_and(|at| at >= from - 500)
            && item.text.contains("Request interrupted by user")
    })
}

/// Claude's own words of the turn it was stopped in: none, where it stopped before
/// the first.
pub fn spoke_since(items: &[Entry], from: i64) -> Vec<&Entry> {
    items
        .iter()
        .filter(|item| item.kind == "assistant" && item.at_ms.is_some_and(|at| at >= from - 500))
        .collect()
}

/// What the transcript holds, as a line: how many records, of which kinds in the
/// order they first came, and the last message of the user's.
pub fn kinds_line(items: &[Entry]) -> String {
    let mut kinds: Vec<(&str, usize)> = Vec::new();
    for item in items {
        match kinds.iter_mut().find(|(kind, _)| *kind == item.kind) {
            Some((_, count)) => *count += 1,
            None => kinds.push((&item.kind, 1)),
        }
    }
    let last_user = items
        .iter()
        .rev()
        .find(|item| item.kind == "user")
        .map(|item| Value::String(item.text.chars().take(80).collect()));
    format!(
        "     its transcript holds {} records: {}; the last user message: {}",
        items.len(),
        kinds
            .iter()
            .map(|(kind, count)| format!("{kind} ×{count}"))
            .collect::<Vec<_>>()
            .join(", "),
        last_user.map_or_else(|| "null".to_owned(), |text| text.to_string())
    )
}

/// The stop's own events: its pause, and the daemon's notes that it was ignored.
/// A window's own lines (`delivery.held`) carry their fields beside the kind, a
/// ledger event's are in `data`.
pub fn stop_events(events: &[Value], number: i64) -> Vec<String> {
    events
        .iter()
        .filter(|event| {
            let kind = event["kind"].as_str().unwrap_or_default();
            (kind == "task.state" && event["data"]["task"] == number)
                || kind == "message.sent"
                || kind.starts_with("delivery.")
                || kind == "message.carried"
        })
        .map(|event| {
            let said = match event.get("data") {
                Some(data) if !data.is_null() => data.clone(),
                _ => Value::Object(
                    event
                        .as_object()
                        .map(|fields| {
                            fields
                                .iter()
                                .filter(|(name, _)| {
                                    !["n", "at", "kind", "data", "project"].contains(&name.as_str())
                                })
                                .map(|(name, value)| (name.clone(), value.clone()))
                                .collect::<Map<String, Value>>()
                        })
                        .unwrap_or_default(),
                ),
            };
            format!(
                "{} {} {} {said}",
                event["n"],
                clock(event["at"].as_str().unwrap_or_default()),
                event["kind"].as_str().unwrap_or_default()
            )
        })
        .collect()
}

/// What the daemon wrote of the stop of the task `number`, as lines of the case.
pub fn say_events(case: &mut Account, members: &Members, number: i64) {
    case.say("     what the daemon wrote:");
    for line in stop_events(&members.events(), number) {
        case.say(format!("       {line}"));
    }
}

/// The notes to the human that the task did not stop (`cf` tells them once the
/// stop is exhausted), from their inbox.
pub fn told_the_human(inbox: &[Value]) -> Vec<&Value> {
    inbox
        .iter()
        .filter(|message| {
            message["body"]
                .as_str()
                .is_some_and(|body| body.contains("did not stop for T-"))
        })
        .collect()
}

/// The notes to the requester that the task did not stop, from its inbox.
pub fn told_the_requester(inbox: &[Value]) -> Vec<&Value> {
    inbox
        .iter()
        .filter(|message| {
            message["body"]
                .as_str()
                .is_some_and(|body| body.contains("did not stop"))
        })
        .collect()
}

/// What `unstopped` says on the member's lane: the board's word that its window did
/// not stop, once every round was ignored; none while it is not said.
pub fn unstopped(lane: &Value) -> Option<Value> {
    Some(&lane["unstopped"])
        .filter(|flag| !flag.is_null())
        .cloned()
}

/// A note's words as a line of the case say them, the start of them: ` ("the
/// body…")`, where there is a note.
pub fn excerpt(note: Option<&Value>, most: usize) -> String {
    note.map_or_else(String::new, |note| {
        let body: String = note["body"]
            .as_str()
            .unwrap_or_default()
            .chars()
            .take(most)
            .collect();
        format!(" (\"{body}…\")")
    })
}

/// A note's words, whole, as JSON writes them: ` ("the body")`, where there is a
/// note.
pub fn whole(note: Option<&Value>) -> String {
    note.map_or_else(String::new, |note| format!(" ({})", json!(note["body"])))
}

#[cfg(test)]
mod tests {
    use cf_e2e::live::ask::Item;

    use super::*;

    fn entry(at_ms: Option<i64>, kind: &str, text: &str) -> Entry {
        Entry {
            at_ms,
            kind: kind.to_owned(),
            text: text.to_owned(),
            model: None,
            raw: format!("{{\"type\":\"{kind}\",\"text\":\"{text}\"}}"),
        }
    }

    fn items() -> Vec<Entry> {
        vec![
            entry(Some(1_000), "user", "Run this\n  command"),
            entry(Some(2_000), "assistant", "tool_use Bash {}"),
            entry(Some(9_000), "user", "[Request interrupted by user]"),
            entry(None, "system/turn_duration", ""),
            entry(Some(9_100), "user", "next"),
        ]
    }

    #[test]
    fn the_end_of_the_transcript_is_quoted_a_record_a_line_with_its_time_since_the_pause() {
        let lines = record_since(&items(), "s-1", 8_000, 3);
        assert_eq!(
            lines,
            [
                "5 records in the transcript of s-1",
                "+1.0 s user: [Request interrupted by user]",
                "+? s system/turn_duration: ",
                "+1.1 s user: next",
            ]
        );
        // All of a short one, and the words of a long one cut where they are.
        assert_eq!(record_since(&items(), "s-1", 0, 99).len(), 6);
        let long = [entry(Some(0), "assistant", &"word ".repeat(100))];
        assert_eq!(
            record_since(&long, "s", 0, 5)[1].len(),
            "+0.0 s assistant: ".len() + 220
        );
    }

    #[test]
    fn what_claude_wrote_of_a_stop_is_its_last_records_by_the_start_of_their_lines() {
        let tail = raw_tail(&items(), 8_000, 2);
        assert_eq!(tail.len(), 2);
        assert_eq!(
            tail[0],
            "+? s {\"type\":\"system/turn_duration\",\"text\":\"\"}"
        );
        assert!(tail[1].starts_with("+1.1 s {\"type\":\"user\""));
    }

    #[test]
    fn the_interrupt_is_the_record_that_says_so_that_was_written_a_moment_before_the_pause_or_later(
    ) {
        let items = items();
        assert_eq!(interrupted(&items, 8_000).unwrap().at_ms, Some(9_000));
        // Half a second before the pause counts: the clocks are not the same clock.
        assert!(interrupted(&items, 9_400).is_some());
        assert!(
            interrupted(&items, 9_600).is_none(),
            "written before the pause"
        );
        // A record with no time is none.
        let untimed = [entry(None, "user", "[Request interrupted by user]")];
        assert!(interrupted(&untimed, 0).is_none());
    }

    #[test]
    fn claude_spoke_in_the_turn_it_was_stopped_in_only_if_an_assistant_record_came_with_the_pause()
    {
        let items = items();
        assert_eq!(spoke_since(&items, 8_000).len(), 0);
        assert_eq!(spoke_since(&items, 1_800).len(), 1);
        assert_eq!(spoke_since(&items, 2_600).len(), 0);
    }

    #[test]
    fn the_transcript_is_a_line_of_its_kinds_in_the_order_they_came_and_the_users_last_words() {
        assert_eq!(
            kinds_line(&items()),
            "     its transcript holds 5 records: user ×3, assistant ×1, system/turn_duration ×1; the last user message: \"next\""
        );
        assert_eq!(
            kinds_line(&[entry(Some(0), "assistant", "hi")]),
            "     its transcript holds 1 records: assistant ×1; the last user message: null"
        );
        let long = [entry(Some(0), "user", &"x".repeat(200))];
        assert!(kinds_line(&long).ends_with(&format!("\"{}\"", "x".repeat(80))));
    }

    #[test]
    fn the_record_reader_is_said_to_find_none_or_to_read_the_record_settled_or_not() {
        assert_eq!(settlement_of(None), "the record reader finds no transcript");
        let read = Record {
            items: vec![
                Item {
                    id: "1".into(),
                    role: "user".into(),
                    text: "hi".into(),
                    complete: false,
                },
                Item {
                    id: "2".into(),
                    role: "assistant".into(),
                    text: "hello  there".into(),
                    complete: true,
                },
            ],
            settled: false,
            settlement: "in-flight".to_owned(),
            failed: Value::Null,
            quota: Value::Null,
            agents_pending: false,
            jobs_pending: 0,
            jobs_retired: 0,
        };
        assert_eq!(
            settlement_of(Some(&read)),
            "the record reader (cf_harness::records, before the adapter's own look) reads its transcript as NOT settled (settlement in-flight; 2 items, the last \"assistant: hello there\")"
        );
        let empty = Record {
            items: vec![],
            settled: true,
            settlement: "settled".to_owned(),
            ..read
        };
        assert!(settlement_of(Some(&empty)).ends_with(
            "reads its transcript as SETTLED (settlement settled; 0 items, the last null)"
        ));
    }

    #[test]
    fn the_events_of_a_stop_are_the_pause_the_messages_and_every_delivery_of_the_task() {
        let events = [
            json!({ "n": 1, "at": "2026-10-11T09:30:00.100Z", "kind": "task.state", "data": { "task": 3, "to": "paused" } }),
            json!({ "n": 2, "at": "2026-10-11T09:30:00.200Z", "kind": "task.state", "data": { "task": 4, "to": "paused" } }),
            json!({ "n": 3, "at": "2026-10-11T09:30:01.000Z", "kind": "message.sent", "data": { "message": 9 } }),
            json!({ "n": 4, "at": "2026-10-11T09:30:02.000Z", "kind": "delivery.held", "project": 1,
                   "participant": "bench-claude-1", "reason": "the window is working", "message": 9 }),
            json!({ "n": 5, "at": "2026-10-11T09:30:03.000Z", "kind": "window.activity", "participant": "chief", "state": "idle" }),
            json!({ "n": 6, "at": "2026-10-11T09:30:04.000Z", "kind": "message.carried", "data": { "carried": [9] } }),
        ];
        assert_eq!(
            stop_events(&events, 3),
            [
                "1 09:30:00.100 task.state {\"task\":3,\"to\":\"paused\"}",
                "3 09:30:01.000 message.sent {\"message\":9}",
                "4 09:30:02.000 delivery.held {\"participant\":\"bench-claude-1\",\"reason\":\"the window is working\",\"message\":9}",
                "6 09:30:04.000 message.carried {\"carried\":[9]}",
            ]
        );
    }

    #[test]
    fn a_window_that_did_not_stop_is_told_to_the_human_by_its_task_and_to_the_requester_by_its_words(
    ) {
        let inbox = [
            json!({ "body": "T-3 did not stop for T-3's pause: it is still at work" }),
            json!({ "body": "an ordinary message" }),
            json!({ "body": "@worker did not stop; the pause goes on" }),
        ];
        assert_eq!(told_the_human(&inbox).len(), 1);
        assert_eq!(told_the_requester(&inbox).len(), 2);
        assert!(told_the_human(&[]).is_empty());
    }

    #[test]
    fn a_note_is_quoted_by_its_start_with_a_mark_of_the_cut_or_whole_as_json_writes_it() {
        let note = json!({ "body": "T-3 did not stop: \"it\" works" });
        assert_eq!(excerpt(Some(&note), 13), " (\"T-3 did not s…\")");
        assert_eq!(
            whole(Some(&note)),
            " (\"T-3 did not stop: \\\"it\\\" works\")"
        );
        assert_eq!(
            (excerpt(None, 5), whole(None)),
            (String::new(), String::new())
        );
    }

    #[test]
    fn the_board_says_a_window_did_not_stop_only_when_its_lane_carries_the_flag() {
        assert_eq!(
            unstopped(&json!({ "unstopped": { "rounds": 3 } })),
            Some(json!({ "rounds": 3 }))
        );
        assert_eq!(unstopped(&json!({ "unstopped": null })), None);
        assert_eq!(unstopped(&json!({})), None);
        assert_eq!(unstopped(&Value::Null), None);
    }
}
