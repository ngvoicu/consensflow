//! How the board's tasks and messages read in a window: one line each, as
//! the API's JSON gives them, every value written as the JavaScript that
//! wrote these lines wrote it (`cf_base::js`), so a window reads what it
//! read before.

use std::borrow::Cow;
use std::fmt::Display;

use cf_base::js;
use cf_proto::cf_word::CfWord;
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

/// "a standard worker", "an image designer": who an open task waits for, as the roles'
/// table says it ([`cf_ledger::roles::a_pool`]); a pool or a tier that is no text printed as
/// JavaScript printed it.
pub fn a_pool(pool: Option<&Value>, tier: Option<&Value>) -> String {
    match pool {
        Some(Value::String(pool)) if cf_ledger::roles::untiered(pool) => {
            cf_ledger::roles::a_pool(Some(pool), None)
        }
        pool => format!(
            "a {} {}",
            js::text(tier),
            match pool {
                Some(Value::String(pool)) => cf_ledger::roles::name_of(pool).into(),
                pool => js::text(pool),
            }
        ),
    }
}

/// One task: its number and state, whose it is or who it waits for, who
/// asked, and its title. What it waits for is said whoever has it: a task given
/// to a window or to the chief itself waits on the board as one for a tier does,
/// and `cf task edit T-3 --needs -T-1` lets it go of one (T-798).
pub fn task_line(task: &Value) -> String {
    let blocked_by = list(task.get("blockedBy"));
    let blocked = if blocked_by.is_empty() {
        String::new()
    } else {
        format!("blocked by {} · ", tasks(numbers(blocked_by)))
    };
    let whose = match task.get("assignee") {
        Some(Value::Null) => format!(
            "{blocked}for {}",
            a_pool(task.get("pool"), task.get("tier"))
        ),
        assignee => format!("{blocked}@{}", js::text(assignee)),
    };
    format!(
        "T-{} [{}] {whose} ← @{}: {}",
        js::text(task.get("number")),
        state_with_aside(task),
        js::text(task.get("requester")),
        js::text(task.get("title")),
    )
}

/// A task's state as its line says it, and what ConsensFlow shows of the task instead of
/// telling its chief (`aside`, while it holds: a note that asks no decision is not typed into
/// the chief's window): `paused · its window stays open …`.
fn state_with_aside(task: &Value) -> String {
    match task.get("aside").and_then(Value::as_str) {
        Some(aside) if !aside.is_empty() => {
            format!("{} · {aside}", js::text(task.get("state")))
        }
        _ => js::text(task.get("state")),
    }
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
/// The words of `message`, as they are printed in the window of `cf`: a message ConsensFlow
/// wrote itself (it names no sender: a note of the engine's or the ledger's) has each command it
/// names started by the window's `cf`, as it was when it was pasted there; what a participant
/// wrote is as they wrote it.
pub fn printed_body(message: &Value, cf: &CfWord) -> String {
    let body = js::text(message.get("body"));
    match message.get("sender") {
        None | Some(Value::Null) => cf.in_text(&body).into_owned(),
        Some(_) => body.into_owned(),
    }
}

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
        state_said(message),
        js::text(message.get("kind")),
    )
}

/// The state a message line says: its own, but `not typed` for a note of ConsensFlow's own
/// that was shown and never typed into the window (it asks no decision; its task's line in
/// `cf task list` says it), whose state is `read` though nobody read it.
fn state_said(message: &Value) -> String {
    let shown = message.get("shown") == Some(&Value::Bool(true))
        || message.pointer("/receipt/shown").is_some();
    if shown {
        "not typed".to_owned()
    } else {
        js::text(message.get("state"))
    }
}

/// The numbers of the answers among `messages` that still wait in the queue
/// to be pasted, or are being pasted: the ones a command that wrote every body of
/// `messages` in full has written whole, and so says it has. An answer already
/// read, or withheld, or any other kind of message, is not one of them: a thread is the
/// task's, and the rest of it is not the reader's to receive one by one.
pub fn waiting_answers(messages: &[Value]) -> Vec<i64> {
    messages
        .iter()
        .filter(|message| message.get("kind").and_then(Value::as_str) == Some("answer"))
        .filter_map(waiting)
        .collect()
}

/// The number of a message `cf inbox read` wrote whole, when it still waits in
/// the queue to be pasted, or is being pasted: whatever its kind, the board
/// decides what a read in the inbox receives. A question, a result or a note read
/// there is the reader's, and is not typed in after it (T-260). One being pasted
/// is read all the same, and the board keeps the read until the paste is settled:
/// a window runs `cf` as soon as it has the message, before the daemon has seen
/// the paste arrive.
pub fn waiting_message(message: &Value) -> Vec<i64> {
    waiting(message).into_iter().collect()
}

/// The number of a message that still waits in the queue or is being pasted.
fn waiting(message: &Value) -> Option<i64> {
    matches!(
        message.get("state").and_then(Value::as_str),
        Some("queued" | "delivering")
    )
    .then(|| message.get("id").and_then(Value::as_i64))
    .flatten()
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
        let testing = json!({ "number": 6, "state": "open", "assignee": null, "requester": "chief",
            "title": "Run T-3", "blockedBy": [3], "pool": "tester", "tier": "standard" });
        assert_eq!(
            task_line(&testing),
            "T-6 [open] blocked by T-3 · for a standard tester ← @chief: Run T-3"
        );
        // A task given to a window, or the chief's own step, says what it waits for too.
        let held = json!({ "number": 7, "state": "open", "assignee": "chief", "requester": "chief",
            "title": "Release notes", "blockedBy": [1, 2], "pool": "worker", "tier": "standard" });
        assert_eq!(
            task_line(&held),
            "T-7 [open] blocked by T-1, T-2 · @chief ← @chief: Release notes"
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
    fn the_answers_still_waiting_to_be_pasted_or_being_pasted_are_the_queued_ones_and_only_the_answers(
    ) {
        let messages = [
            json!({ "id": 3, "kind": "task", "state": "delivered", "body": "Parser" }),
            json!({ "id": 5, "kind": "answer", "state": "queued", "body": "JSON" }),
            json!({ "id": 6, "kind": "answer", "state": "read", "body": "YAML" }),
            json!({ "id": 7, "kind": "note", "state": "queued", "body": "Mind the tests" }),
            json!({ "id": 8, "kind": "answer", "state": "delivering", "body": "TOML" }),
            json!({ "id": 9, "kind": "answer", "state": "queued", "body": "XML" }),
            json!({ "id": 10, "kind": "result", "state": "queued", "body": "Parser done" }),
            json!({ "kind": "answer", "state": "queued", "body": "no id" }),
        ];
        assert_eq!(waiting_answers(&messages), [5, 8, 9]);
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
