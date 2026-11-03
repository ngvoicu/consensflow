//! The user's messages Codex writes itself in the model's context (3.0.7-alpha.2,
//! T-1113): its `# AGENTS.md instructions`, the `<environment_context>` of the thread,
//! which it writes again when the environment changes, and a subagent's notification.
//! No person typed them, so the reading says so (`synthetic`), as it says it of Claude
//! Code's `<task-notification>`: a copy of the chief's window that took them for the human
//! typing made a project's wake-up calls come for nothing.

use super::*;

/// The model's message of a user's input.
fn user(turn: &str, id: &str, words: &str) -> Value {
    response(json!({
        "type": "message", "role": "user", "id": id,
        "content": [{ "type": "input_text", "text": words }],
        "internal_chat_message_metadata_passthrough": { "turn_id": turn },
    }))
}

/// Codex's own item of a user's input.
fn completed(turn: &str, id: &str, words: &str) -> Value {
    event(json!({
        "type": "item_completed", "turn_id": turn,
        "item": {
            "type": "UserMessage", "id": id,
            "content": [{ "type": "text", "text": words, "text_elements": [] }],
        },
    }))
}

/// The user's items of what `records` say, each by its first line, with whether it is Codex's own.
fn users(records: &[Value]) -> Vec<(String, bool)> {
    read(records)
        .unwrap()
        .items
        .iter()
        .filter(|item| item.role == Role::User)
        .map(|item| {
            (
                item.text.lines().next().unwrap_or_default().to_owned(),
                item.synthetic,
            )
        })
        .collect()
}

#[test]
fn what_codex_writes_in_the_context_is_its_own_and_what_a_person_typed_is_not() {
    let environment = "<environment_context>\n  <cwd>/work/app</cwd>\n  <shell>zsh</shell>\n</environment_context>";
    let agents =
        "# AGENTS.md instructions for /work/app\n\n<INSTRUCTIONS>\nBe brief.\n</INSTRUCTIONS>";
    let subagent = "<subagent_notification>\n{\"agent_path\":\"a\",\"status\":\"done\"}\n</subagent_notification>";
    let brief = "[ConsensFlow m-1 · T-1 · task from @chief]\nWrite the parser";
    let typed = "Look at the docs next";
    assert_eq!(
        users(&[
            started(json!("t1")),
            user("t1", "m1", agents),
            user("t1", "m2", environment),
            user("t1", "m3", brief),
            started(json!("t2")),
            user("t2", "m4", typed),
            user("t2", "m5", environment),
            user("t2", "m6", subagent),
        ]),
        [
            ("# AGENTS.md instructions for /work/app".to_owned(), true),
            ("<environment_context>".to_owned(), true),
            (
                "[ConsensFlow m-1 · T-1 · task from @chief]".to_owned(),
                false
            ),
            ("Look at the docs next".to_owned(), false),
            ("<environment_context>".to_owned(), true),
            ("<subagent_notification>".to_owned(), true),
        ],
        "the environment written again after a person's words is Codex's still"
    );
}

#[test]
fn a_message_that_quotes_what_codex_writes_further_in_is_the_persons() {
    let quoting =
        "Why does the thread start with <environment_context> and # AGENTS.md instructions?";
    assert_eq!(
        users(&[started(json!("t1")), user("t1", "m1", quoting)]),
        [(quoting.to_owned(), false)]
    );
}

#[test]
fn what_codex_writes_is_its_own_whichever_telling_comes_first_and_is_one_item() {
    let environment = "<environment_context>\n  <cwd>/work/app</cwd>\n</environment_context>";
    for records in [
        vec![
            started(json!("t1")),
            user("t1", "msg_1", environment),
            completed("t1", "item-1", environment),
        ],
        vec![
            started(json!("t1")),
            completed("t1", "item-1", environment),
            user("t1", "msg_1", environment),
        ],
    ] {
        assert_eq!(
            users(&records),
            [("<environment_context>".to_owned(), true)]
        );
    }
}
