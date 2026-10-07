//! How the board's tasks and messages read in a window: one line each, as
//! the API's JSON gives them, every value written as the JavaScript that
//! wrote these lines wrote it (`cf_base::js`), so a window reads what it
//! read before.

use std::borrow::Cow;
use std::fmt::Display;

use cf_base::js;
use serde_json::Value;

/// "T-3, T-4": task numbers in a sentence.
pub fn tasks<T: Display>(numbers: impl IntoIterator<Item = T>) -> String {
    numbers
        .into_iter()
        .map(|number| format!("T-{number}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The task numbers in a JSON list, each as JavaScript wrote it.
pub fn numbers(list: &[Value]) -> impl Iterator<Item = Cow<'_, str>> {
    list.iter().map(|number| js::text(Some(number)))
}

/// "a standard worker", "an image designer": who an open task waits for.
pub fn a_pool(pool: Option<&Value>, tier: Option<&Value>) -> String {
    match pool {
        Some(Value::String(pool)) if pool == "designer" => "an image designer".to_string(),
        pool => format!("a {} {}", js::text(tier), js::text(pool)),
    }
}

/// One task: its number and state, whose it is or who it waits for, who
/// asked, and its title.
pub fn task_line(task: &Value) -> String {
    let whose = match task.get("assignee") {
        Some(Value::Null) => {
            let blocked_by = list(task.get("blockedBy"));
            let blocked = if blocked_by.is_empty() {
                String::new()
            } else {
                format!("blocked by {} · ", tasks(numbers(blocked_by)))
            };
            format!(
                "{blocked}for {}",
                a_pool(task.get("pool"), task.get("tier"))
            )
        }
        assignee => format!("@{}", js::text(assignee)),
    };
    format!(
        "T-{} [{}] {whose} ← @{}: {}",
        js::text(task.get("number")),
        js::text(task.get("state")),
        js::text(task.get("requester")),
        js::text(task.get("title")),
    )
}

/// What `cf task get` heads a task with: its line, and when the human
/// deleted it from the board, if they did.
pub fn task_head(task: &Value) -> String {
    match task.get("deletedAt") {
        Some(Value::Null) => task_line(task),
        deleted => format!(
            "{}\nDeleted from the board by the human at {}.",
            task_line(task),
            js::text(deleted)
        ),
    }
}

/// One message: its id and state, its kind and task, who sent it, and its
/// first line when the API gave one.
pub fn message_line(message: &Value) -> String {
    let task = match message.get("task") {
        None | Some(Value::Null) => message.get("taskNumber"),
        task => task,
    };
    let on_task = if js::truthy(task) {
        format!(" T-{}", js::text(task))
    } else {
        String::new()
    };
    let sender = match message.get("sender") {
        Some(Value::Null) => "ConsensFlow".to_string(),
        sender => format!("@{}", js::text(sender)),
    };
    let preview = match message.get("preview") {
        None => String::new(),
        preview => format!(": {}", js::text(preview)),
    };
    format!(
        "m-{} [{}] {}{on_task} from {sender}{preview}",
        js::text(message.get("id")),
        js::text(message.get("state")),
        js::text(message.get("kind")),
    )
}

/// The numbers of the answers among `messages` that still wait in the queue
/// to be pasted: the ones a command that wrote every body of `messages` in
/// full has written whole, and so says it has. An answer already read, or
/// withheld, or any other kind of message, is not one of them.
pub fn waiting_answers(messages: &[Value]) -> Vec<i64> {
    messages
        .iter()
        .filter(|message| {
            message.get("kind").and_then(Value::as_str) == Some("answer")
                && message.get("state").and_then(Value::as_str) == Some("queued")
        })
        .filter_map(|message| message.get("id").and_then(Value::as_i64))
        .collect()
}

/// The items of a JSON list; none when it is no list.
pub fn list(value: Option<&Value>) -> &[Value] {
    value.and_then(Value::as_array).map_or(&[], Vec::as_slice)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_task_reads_as_whose_it_is_or_who_it_waits_for() {
        let mine = json!({ "number": 3, "state": "working", "assignee": "zeus",
            "requester": "chief", "title": "Parser", "blockedBy": [], "pool": "worker", "tier": "standard" });
        assert_eq!(task_line(&mine), "T-3 [working] @zeus ← @chief: Parser");
        let open = json!({ "number": 4, "state": "open", "assignee": null, "requester": "chief",
            "title": "Docs", "blockedBy": [2, 3], "pool": "reviewer", "tier": "light" });
        assert_eq!(
            task_line(&open),
            "T-4 [open] blocked by T-2, T-3 · for a light reviewer ← @chief: Docs"
        );
        let drawing = json!({ "number": 5, "state": "open", "assignee": null, "requester": "chief",
            "title": "Logo", "blockedBy": [], "pool": "designer", "tier": null });
        assert_eq!(
            task_line(&drawing),
            "T-5 [open] for an image designer ← @chief: Logo"
        );
    }

    #[test]
    fn a_deleted_task_says_when_the_human_deleted_it() {
        let task = json!({ "number": 3, "state": "accepted", "assignee": "zeus", "requester": "chief",
            "title": "Parser", "blockedBy": [], "deletedAt": "2026-10-04T09:00:00.000Z" });
        assert_eq!(
            task_head(&task),
            "T-3 [accepted] @zeus ← @chief: Parser\nDeleted from the board by the human at 2026-10-04T09:00:00.000Z."
        );
        let kept = json!({ "number": 3, "state": "accepted", "assignee": "zeus", "requester": "chief",
            "title": "Parser", "blockedBy": [], "deletedAt": null });
        assert_eq!(task_head(&kept), "T-3 [accepted] @zeus ← @chief: Parser");
    }

    #[test]
    fn the_answers_still_waiting_to_be_pasted_are_the_queued_ones_and_only_the_answers() {
        let messages = [
            json!({ "id": 3, "kind": "task", "state": "delivered", "body": "Parser" }),
            json!({ "id": 5, "kind": "answer", "state": "queued", "body": "JSON" }),
            json!({ "id": 6, "kind": "answer", "state": "read", "body": "YAML" }),
            json!({ "id": 7, "kind": "note", "state": "queued", "body": "Mind the tests" }),
            json!({ "id": 8, "kind": "answer", "state": "delivering", "body": "TOML" }),
            json!({ "id": 9, "kind": "answer", "state": "queued", "body": "XML" }),
            json!({ "kind": "answer", "state": "queued", "body": "no id" }),
        ];
        assert_eq!(waiting_answers(&messages), [5, 9]);
        assert!(waiting_answers(&[]).is_empty());
    }

    #[test]
    fn a_message_reads_with_its_task_its_sender_and_its_first_line() {
        let asked = json!({ "id": 12, "state": "queued", "kind": "question", "task": 3,
            "sender": "zeus", "preview": "Which one?" });
        assert_eq!(
            message_line(&asked),
            "m-12 [queued] question T-3 from @zeus: Which one?"
        );
        let notice = json!({ "id": 13, "state": "read", "kind": "notice", "task": null,
            "taskNumber": 4, "sender": null });
        assert_eq!(
            message_line(&notice),
            "m-13 [read] notice T-4 from ConsensFlow"
        );
        let loose = json!({ "id": 14, "state": "read", "kind": "note", "task": null, "sender": "chief",
            "preview": "" });
        assert_eq!(message_line(&loose), "m-14 [read] note from @chief: ");
    }
}
