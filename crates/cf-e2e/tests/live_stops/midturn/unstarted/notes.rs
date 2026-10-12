//! What the board and the daemon say, in words, of a task that did not get going:
//! the task and the messages of its thread, what the chief and the human were
//! told of it, and the daemon's own log.

use cf_e2e::live::cut;
use cf_e2e::live::members::squash;
use serde_json::Value;

/// How much of a message's words, and of the board's reason for its state, a line says.
const WORDS: usize = 400;
const REASON: usize = 400;

/// How much of a line of the daemon's log a line says.
const LOG_LINE: usize = 240;

/// A message as a line: its number, kind and state, who sent it, why it is in its
/// state where the board says, its receipt where it has one, and its words.
fn message_line(message: &Value) -> String {
    let sender = message["sender"]
        .as_str()
        .map(|sender| format!(" from @{sender}"))
        .unwrap_or_default();
    let reason = message["reason"]
        .as_str()
        .filter(|reason| !reason.is_empty())
        .map(|reason| format!(" ({})", squash(reason, REASON)))
        .unwrap_or_default();
    let receipt = Some(&message["receipt"])
        .filter(|receipt| !receipt.is_null() && **receipt != serde_json::json!({}))
        .map(|receipt| format!(" receipt {}", squash(&receipt.to_string(), 80)))
        .unwrap_or_default();
    format!(
        "m-{} {} {}{sender}{reason}{receipt}: {}",
        message["id"],
        message["kind"].as_str().unwrap_or("?"),
        message["state"].as_str().unwrap_or("?"),
        squash(message["body"].as_str().unwrap_or_default(), WORDS)
    )
}

/// The task `number` as the board has it, in lines: how it stands, then each
/// message of its thread. (Was the brief delivered is in the state of the first.)
pub fn task_lines(thread: &Value, number: i64) -> Vec<String> {
    if thread.is_null() {
        return vec![format!("the board has no T-{number}")];
    }
    let mut lines = vec![format!(
        "the task: T-{number} is {}",
        thread["state"].as_str().unwrap_or("undefined")
    )];
    lines.extend(
        thread["messages"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|message| format!("  {}", message_line(message))),
    );
    lines
}

/// What `who` was told, in lines: the last `most` messages of the inbox, and how
/// many there are in all.
pub fn inbox_lines(who: &str, inbox: &[Value], most: usize) -> Vec<String> {
    if inbox.is_empty() {
        return vec![format!("{who} was told: nothing")];
    }
    let shown = &inbox[inbox.len().saturating_sub(most)..];
    let mut lines = vec![format!(
        "{who} was told ({} in all, the last {}):",
        inbox.len(),
        shown.len()
    )];
    lines.extend(
        shown
            .iter()
            .map(|message| format!("  {}", message_line(message))),
    );
    lines
}

/// What the chief was told of the task that is over, in the daemon's words: the
/// first note of its inbox that begins with the task (`T-1 failed: the window never
/// showed its first message; its screen ended with: "…"`). None where it was told
/// nothing of it.
pub fn told_why(number: i64, chief: &[Value]) -> Option<String> {
    let begins = format!("T-{number} ");
    chief
        .iter()
        .filter_map(|message| message["body"].as_str())
        .find(|body| body.starts_with(&begins))
        .map(|body| squash(body, WORDS))
}

/// The daemon's log, in lines: the warnings and errors before its last `most`
/// lines, the first few of them, and then the last lines.
pub fn log_lines(log: &str, most: usize) -> Vec<String> {
    let lines: Vec<&str> = log.lines().collect();
    let cut_at = lines.len().saturating_sub(most);
    let earlier: Vec<String> = lines[..cut_at]
        .iter()
        .filter(|line| line.contains(" warn ") || line.contains(" error "))
        .take(10)
        .map(|line| cut(line, LOG_LINE))
        .collect();
    let mut said = Vec::new();
    if !earlier.is_empty() {
        said.push("the daemon's log, its earlier warnings and errors:".to_owned());
        said.extend(earlier.into_iter().map(|line| format!("  {line}")));
    }
    if lines.is_empty() {
        said.push("the daemon's log: empty".to_owned());
        return said;
    }
    said.push(format!(
        "the daemon's log, its last {} of {}:",
        lines.len() - cut_at,
        lines.len()
    ));
    said.extend(
        lines[cut_at..]
            .iter()
            .map(|line| format!("  {}", cut(line, LOG_LINE))),
    );
    said
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn failed_thread() -> Value {
        json!({
            "state": "failed",
            "messages": [
                { "id": 3, "kind": "task", "state": "queued", "sender": "chief", "reason": null,
                  "receipt": null, "body": "Write the whole numbers\nfrom 1 to 5000" },
                { "id": 4, "kind": "note", "state": "delivered", "sender": null,
                  "reason": "T-1 was cancelled", "receipt": { "record": "m-4" }, "body": "x" },
            ],
        })
    }

    #[test]
    fn a_task_is_how_it_stands_and_each_message_of_its_thread_with_its_state_and_words() {
        assert_eq!(
            task_lines(&failed_thread(), 1),
            [
                "the task: T-1 is failed",
                "  m-3 task queued from @chief: Write the whole numbers from 1 to 5000",
                "  m-4 note delivered (T-1 was cancelled) receipt {\"record\":\"m-4\"}: x",
            ]
        );
        assert_eq!(task_lines(&Value::Null, 7), ["the board has no T-7"]);
        assert_eq!(
            task_lines(&json!({ "messages": [] }), 2),
            ["the task: T-2 is undefined"]
        );
    }

    #[test]
    fn an_empty_receipt_is_none_and_the_words_of_a_message_are_cut() {
        let message = json!({ "id": 9, "kind": "note", "state": "queued", "receipt": {},
                              "body": "word ".repeat(200) });
        let line = message_line(&message);
        assert!(line.starts_with("m-9 note queued: word word"), "{line}");
        assert_eq!(line.chars().count(), "m-9 note queued: ".len() + WORDS);
        // And so is the board's reason, which is long where the daemon quotes a screen.
        let reasoned =
            json!({ "id": 9, "kind": "task", "state": "failed", "reason": "r ".repeat(500) });
        assert_eq!(
            message_line(&reasoned).chars().count(),
            "m-9 task failed (".len() + REASON + "): ".len()
        );
    }

    #[test]
    fn what_a_participant_was_told_is_the_last_messages_of_its_inbox_and_how_many_there_are() {
        let inbox: Vec<Value> = (1..=5)
            .map(|id| json!({ "id": id, "kind": "note", "state": "delivered", "body": format!("n{id}") }))
            .collect();
        assert_eq!(
            inbox_lines("the chief", &inbox, 2),
            [
                "the chief was told (5 in all, the last 2):",
                "  m-4 note delivered: n4",
                "  m-5 note delivered: n5",
            ]
        );
        assert_eq!(
            inbox_lines("the human", &[], 5),
            ["the human was told: nothing"]
        );
    }

    #[test]
    fn the_daemons_words_of_a_task_are_the_chiefs_first_note_that_begins_with_it() {
        let chief = [
            json!({ "body": "T-10 failed: not this one" }),
            json!({ "kind": "note" }),
            json!({ "body": "T-1 failed: the window never showed its first message; its screen ended with: \"No API key\". Reopen it with: cf task reopen T-1" }),
            json!({ "body": "T-1 was reopened" }),
        ];
        assert_eq!(
            told_why(1, &chief).as_deref(),
            Some("T-1 failed: the window never showed its first message; its screen ended with: \"No API key\". Reopen it with: cf task reopen T-1")
        );
        assert_eq!(told_why(2, &chief), None);
        assert_eq!(told_why(1, &[]), None);
    }

    #[test]
    fn the_log_is_its_last_lines_and_the_warnings_and_errors_before_them() {
        let mut log =
            String::from("t0 info start\nt1 warn the launch of p1-x failed: no\nt2 error boom\n");
        for at in 3..13 {
            log.push_str(&format!("t{at} info alive: {at} passes\n"));
        }
        let said = log_lines(&log, 4);
        assert_eq!(
            said,
            [
                "the daemon's log, its earlier warnings and errors:",
                "  t1 warn the launch of p1-x failed: no",
                "  t2 error boom",
                "the daemon's log, its last 4 of 13:",
                "  t9 info alive: 9 passes",
                "  t10 info alive: 10 passes",
                "  t11 info alive: 11 passes",
                "  t12 info alive: 12 passes",
            ]
        );
        // A short log is said whole, once, and an empty one is said to be.
        assert_eq!(
            log_lines("a warn b\nc\n", 30),
            ["the daemon's log, its last 2 of 2:", "  a warn b", "  c"]
        );
        assert_eq!(log_lines("", 30), ["the daemon's log: empty"]);
    }

    #[test]
    fn a_long_line_of_the_log_is_cut() {
        let said = log_lines(&format!("{}\n", "x".repeat(500)), 5);
        assert_eq!(said[1].chars().count(), "  ".len() + LOG_LINE + 1);
        assert!(said[1].ends_with('…'));
    }
}
