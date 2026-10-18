//! What Devin's own record of a session says of the tools its agent called: each
//! call of a turn and what it came back with (an answer, or the words of a call that
//! did not run), and the words of a refusal anywhere in a message of the store.

use cf_e2e::devin_store::{Call, Message};
use cf_e2e::live::members::squash;

use crate::watch::{in_text, Said};
use crate::words::MARKERS;

/// What messages of the store say: each marker one of them holds, with the id of
/// the message and who it is from, and the words around it.
pub fn in_messages(messages: &[Message]) -> Vec<(String, Said)> {
    let mut found = Vec::new();
    for message in messages {
        for (saw, excerpt) in in_text(&message.raw.to_string()) {
            found.push((
                message.id.clone(),
                (saw, format!("{}: {excerpt}", message.role)),
            ));
        }
    }
    found
}

/// What a tool's message came back with, as words.
fn result_of(message: &Message) -> String {
    if message.text.is_empty() {
        message.raw["content"].to_string()
    } else {
        message.text.clone()
    }
}

/// A call of a message and what came of it, as a line: the tool, and whether its
/// answer holds the words of a call that did not run.
fn tool_line(call: &Call, answer: Option<&Message>) -> String {
    match answer {
        None => format!("{} (no answer in the record)", call.name),
        Some(answer) if MARKERS.iter().any(|marker| answer.holds(marker)) => {
            format!(
                "{} REFUSED ({})",
                call.name,
                squash(&result_of(answer), 100)
            )
        }
        Some(answer) => format!(
            "{} answered ({})",
            call.name,
            squash(&result_of(answer), 70)
        ),
    }
}

/// What came of a call, by what Devin's record has of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Called {
    /// It was answered, and the answer holds no word of a call that did not run.
    Answered(String),
    /// Its answer holds the words of a call that did not run.
    Refused(String),
    /// Nothing answered it.
    Unanswered,
}

/// The last call of the session that asked for `words` (anywhere in what it was
/// stored with), and what came of it; none where no call did.
pub fn called(messages: &[Message], words: &str) -> Option<Called> {
    let (call, at) = messages
        .iter()
        .enumerate()
        .rev()
        .find_map(|(at, message)| {
            message
                .calls()
                .into_iter()
                .rev()
                .find(|call| call.says(words))
                .map(|call| (call, at))
        })?;
    let answer = messages[at + 1..]
        .iter()
        .find(|other| other.answers() == Some(call.id.as_str()));
    Some(match answer {
        None => Called::Unanswered,
        Some(answer) if MARKERS.iter().any(|marker| answer.holds(marker)) => {
            Called::Refused(squash(&result_of(answer), 100))
        }
        Some(answer) => Called::Answered(squash(&result_of(answer), 100)),
    })
}

/// The tools the agent called after the user's message that has `header`, and what
/// each came back with, in the order they were called.
pub fn tools_after(messages: &[Message], header: &str) -> Vec<String> {
    let Some(from) = messages
        .iter()
        .rposition(|message| message.role == "user" && message.text.contains(header))
    else {
        return Vec::new();
    };
    let after = &messages[from + 1..];
    let mut tools = Vec::new();
    for message in after {
        for call in message.calls() {
            let answer = after
                .iter()
                .find(|other| other.answers() == Some(call.id.as_str()));
            tools.push(tool_line(&call, answer));
        }
    }
    tools
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};

    use super::*;

    fn message(id: &str, role: &str, raw: Value) -> Message {
        let mut raw = raw;
        raw["message_id"] = json!(id);
        raw["role"] = json!(role);
        let text = raw["content"].as_str().unwrap_or_default().to_owned();
        Message {
            id: id.to_owned(),
            role: role.to_owned(),
            text,
            raw,
        }
    }

    #[test]
    fn a_message_of_the_store_says_which_it_is_and_what_it_holds_of_the_markers() {
        let messages = [
            message("a", "assistant", json!({ "content": "fine" })),
            message(
                "b",
                "tool",
                json!({ "tool_call_id": "c1", "content": [
                    { "type": "text", "text": "Tool execution was rejected: User skipped this tool call" },
                ] }),
            ),
        ];
        let found = in_messages(&messages);
        let said: Vec<(&str, &str)> = found
            .iter()
            .map(|(id, (saw, _))| (id.as_str(), saw.as_str()))
            .collect();
        assert_eq!(
            said,
            [
                ("b", "User skipped this tool call"),
                ("b", "Tool execution was rejected")
            ]
        );
        assert!(found[0].1 .1.starts_with("tool: "), "{found:?}");
    }

    #[test]
    fn a_call_is_found_by_what_it_asked_for_and_what_came_of_it_is_what_answered_it() {
        let call = |id: &str, command: &str| {
            message(
                &format!("a-{id}"),
                "assistant",
                json!({ "content": "", "tool_calls": [
                    { "id": id, "name": "run_command", "arguments": { "command": command } },
                ] }),
            )
        };
        let answer = |id: &str, said: &str| {
            message(
                &format!("t-{id}"),
                "tool",
                json!({ "tool_call_id": id, "content": said }),
            )
        };
        // The user's own words are no call, whatever they ask for.
        let asked = message(
            "u",
            "user",
            json!({ "content": "Run this: cf ask which-ab12" }),
        );
        assert_eq!(called(std::slice::from_ref(&asked), "which-ab12"), None);
        let ran = [
            asked.clone(),
            call("c1", "cf ask which-ab12"),
            answer("c1", "stand-in cf: ask which-ab12"),
        ];
        assert_eq!(
            called(&ran, "which-ab12"),
            Some(Called::Answered("stand-in cf: ask which-ab12".to_owned()))
        );
        assert_eq!(called(&ran, "which-ffff"), None);
        // The last call that asked is the one that counts, and what refused it is told.
        let refused = [
            call("c1", "cf ask which-ab12"),
            answer("c1", "stand-in cf: ask which-ab12"),
            call("c2", "cf ask \"which-ab12\""),
            answer(
                "c2",
                "Tool execution was rejected: User skipped this tool call",
            ),
        ];
        assert_eq!(
            called(&refused, "which-ab12"),
            Some(Called::Refused(
                "Tool execution was rejected: User skipped this tool call".to_owned()
            ))
        );
        let waiting = [call("c3", "cf ask which-ab12")];
        assert_eq!(called(&waiting, "which-ab12"), Some(Called::Unanswered));
    }

    #[test]
    fn the_tools_a_turn_called_are_told_with_what_each_came_back_with() {
        let header = "[ConsensFlow m-10 ";
        let messages = [
            message(
                "old",
                "user",
                json!({ "content": "[ConsensFlow m-9 · T-1 · note from @chief]\nbefore" }),
            ),
            message(
                "u",
                "user",
                json!({ "content": "[ConsensFlow m-10 · T-1 · note from @chief]\nrun it" }),
            ),
            message(
                "a1",
                "assistant",
                json!({ "content": "", "tool_calls": [
                    { "id": "c1", "name": "run_command" },
                    { "id": "c2", "name": "write" },
                    { "id": "c3", "name": "edit" },
                ] }),
            ),
            message(
                "t1",
                "tool",
                json!({ "tool_call_id": "c1", "content": "exit 0" }),
            ),
            message(
                "t2",
                "tool",
                json!({ "tool_call_id": "c2",
                        "content": "Tool execution was rejected: User skipped this tool call" }),
            ),
            message("a2", "assistant", json!({ "content": "done" })),
        ];
        assert_eq!(
            tools_after(&messages, header),
            [
                "run_command answered (exit 0)",
                "write REFUSED (Tool execution was rejected: User skipped this tool call)",
                "edit (no answer in the record)",
            ]
        );
        // A message that never arrived has no turn to tell the tools of.
        assert!(tools_after(&messages, "[ConsensFlow m-11 ").is_empty());
    }
}
