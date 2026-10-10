//! What Codex writes of the input of a turn, twice: as the model's own message
//! (`response_item`, id `msg_…`) and as the item Codex completes itself
//! (`item_completed`, `UserMessage`, an id of its own), the same words in the
//! same turn, the records a line or a millisecond apart. A transcript lists the
//! input once; and no message that has no twin is lost to that.

use super::*;

/// The model's message of a user's input: its id, and the turn it is in.
fn model(turn: &str, id: &str, words: &str) -> Value {
    response(json!({
        "type": "message", "role": "user", "id": id,
        "content": [{ "type": "input_text", "text": words }],
        "internal_chat_message_metadata_passthrough": { "turn_id": turn },
    }))
}

/// Codex's own item of a user's input.
fn codex(turn: &str, id: &str, words: &str) -> Value {
    event(json!({
        "type": "item_completed", "turn_id": turn,
        "item": {
            "type": "UserMessage", "id": id,
            "content": [{ "type": "text", "text": words, "text_elements": [] }],
        },
    }))
}

/// The user's items of what `records` say: their ids and words.
fn users(records: &[Value]) -> Vec<(String, String)> {
    read(records)
        .unwrap()
        .items
        .iter()
        .filter(|item| item.role == Role::User)
        .map(|item| (item.id.to_string(), item.text.to_string()))
        .collect()
}

fn said(id: &str, words: &str) -> (String, String) {
    (id.to_owned(), words.to_owned())
}

#[test]
fn an_input_told_twice_is_one_item_and_the_first_telling_names_it() {
    let brief = "[ConsensFlow m-1 · T-1 · task from @chief]\nDraw a jar of honey.";
    // As Codex writes a turn: the model's message, then its own item.
    assert_eq!(
        users(&[
            started(json!("t1")),
            model("t1", "msg_1", brief),
            codex("t1", "item-1", brief),
        ]),
        [said("msg_1", brief)]
    );
    // Whichever comes first is the item: the ledger keeps an item by its id, and
    // an id that changed between two looks would be two rows.
    assert_eq!(
        users(&[
            started(json!("t1")),
            codex("t1", "item-1", brief),
            model("t1", "msg_1", brief),
        ]),
        [said("item-1", brief)]
    );
}

#[test]
fn an_ordinary_turn_lists_every_message_it_has_once() {
    let ran = |id: &str, role: &str, words: &str, phase: &str| {
        response(json!({
            "type": "message", "role": role, "id": id, "phase": phase,
            "content": [{ "type": "output_text", "text": words }],
            "internal_chat_message_metadata_passthrough": { "turn_id": "t1" },
        }))
    };
    let said_by_codex = |id: &str, words: &str, phase: &str| {
        event(json!({
            "type": "item_completed", "turn_id": "t1",
            "item": { "type": "AgentMessage", "id": id, "phase": phase,
                      "content": [{ "type": "Text", "text": words }] },
        }))
    };
    let record = read(&[
        started(json!("t1")),
        // What Codex puts before the input: instructions for the model, and the
        // environment it runs in, which is a user's message that has no twin.
        response(
            json!({ "type": "message", "role": "developer", "id": "msg_dev",
            "content": [{ "type": "input_text", "text": "Your role is designer." }] }),
        ),
        model(
            "t1",
            "msg_env",
            "<environment_context>\n  <cwd>/work</cwd>\n</environment_context>",
        ),
        model("t1", "msg_in", "Draw a jar of honey."),
        codex("t1", "item-in", "Draw a jar of honey."),
        said_by_codex("msg_a", "I'll draw it.", "commentary"),
        ran("msg_a", "assistant", "I'll draw it.", "commentary"),
        said_by_codex("msg_z", "Done.", "final_answer"),
        ran("msg_z", "assistant", "Done.", "final_answer"),
        event(json!({ "type": "task_complete", "turn_id": "t1", "last_agent_message": "Done." })),
    ])
    .unwrap();
    let items: Vec<(&str, Role, &str)> = record
        .items
        .iter()
        .map(|item| (&*item.id, item.role, &*item.text))
        .collect();
    assert_eq!(
        items,
        [
            (
                "msg_env",
                Role::User,
                "<environment_context>\n  <cwd>/work</cwd>\n</environment_context>"
            ),
            ("msg_in", Role::User, "Draw a jar of honey."),
            ("msg_a", Role::Assistant, "I'll draw it."),
            ("msg_z", Role::Assistant, "Done."),
        ]
    );
    assert_eq!(record.settlement, Settlement::Settled);
}

#[test]
fn two_inputs_that_say_the_same_words_are_two_messages() {
    // One after the other, each with its twin.
    assert_eq!(
        users(&[
            started(json!("t1")),
            model("t1", "msg_1", "continue"),
            codex("t1", "item-1", "continue"),
            model("t1", "msg_2", "continue"),
            codex("t1", "item-2", "continue"),
        ]),
        [said("msg_1", "continue"), said("msg_2", "continue")]
    );
    // Both of the model's, then both of Codex's: one twin for each.
    assert_eq!(
        users(&[
            started(json!("t1")),
            model("t1", "msg_1", "continue"),
            model("t1", "msg_2", "continue"),
            codex("t1", "item-1", "continue"),
            codex("t1", "item-2", "continue"),
        ]),
        [said("msg_1", "continue"), said("msg_2", "continue")]
    );
    // One whose twin was never written is a message of its own.
    assert_eq!(
        users(&[
            started(json!("t1")),
            model("t1", "msg_1", "continue"),
            model("t1", "msg_2", "continue"),
            codex("t1", "item-2", "continue"),
        ]),
        [said("msg_1", "continue"), said("msg_2", "continue")]
    );
    // And a twin is the twin of one: the model's message that Codex's first
    // item repeated is not repeated again by its second.
    assert_eq!(
        users(&[
            started(json!("t1")),
            model("t1", "msg_1", "continue"),
            codex("t1", "item-1", "continue"),
            codex("t1", "item-2", "continue"),
        ]),
        [said("msg_1", "continue"), said("item-2", "continue")]
    );
}

#[test]
fn the_same_words_in_another_turn_are_another_message() {
    assert_eq!(
        users(&[
            started(json!("t1")),
            model("t1", "msg_1", "yes"),
            codex("t1", "item-1", "yes"),
            started(json!("t2")),
            model("t2", "msg_2", "yes"),
            codex("t2", "item-2", "yes"),
        ]),
        [said("msg_1", "yes"), said("msg_2", "yes")]
    );
    // A message a fork inherited has no twin; the words of a later one are no
    // twin of it.
    assert_eq!(
        users(&[
            model("parent", "msg_old", "yes"),
            started(json!("t1")),
            codex("t1", "item-1", "yes"),
        ]),
        [said("msg_old", "yes"), said("item-1", "yes")]
    );
}

#[test]
fn a_message_with_no_twin_or_other_words_stays() {
    assert_eq!(
        users(&[started(json!("t1")), model("t1", "msg_1", "only this")]),
        [said("msg_1", "only this")]
    );
    assert_eq!(
        users(&[started(json!("t1")), codex("t1", "item-1", "only that")]),
        [said("item-1", "only that")]
    );
    assert_eq!(
        users(&[
            started(json!("t1")),
            model("t1", "msg_1", "one"),
            codex("t1", "item-1", "another"),
        ]),
        [said("msg_1", "one"), said("item-1", "another")]
    );
    // The same telling twice is not a twin: the twin is the other telling.
    assert_eq!(
        users(&[
            started(json!("t1")),
            model("t1", "msg_1", "one"),
            model("t1", "msg_2", "one"),
        ]),
        [said("msg_1", "one"), said("msg_2", "one")]
    );
}

#[test]
fn a_twin_is_named_by_the_turn_and_a_record_that_names_none_has_none() {
    assert_eq!(
        users(&[
            response(json!({ "type": "message", "role": "user", "id": "msg_1",
                "content": [{ "type": "input_text", "text": "hi" }] })),
            event(json!({ "type": "item_completed", "item": {
                "type": "UserMessage", "id": "item-1",
                "content": [{ "type": "text", "text": "hi" }] } })),
        ]),
        [said("msg_1", "hi"), said("item-1", "hi")]
    );
}

#[test]
fn the_twins_id_is_the_items_too_so_a_record_repeated_under_it_adds_nothing() {
    assert_eq!(
        users(&[
            started(json!("t1")),
            model("t1", "msg_1", "hi"),
            codex("t1", "item-1", "hi"),
            codex("t1", "item-1", "hi"),
        ]),
        [said("msg_1", "hi")]
    );
}

#[test]
fn a_twin_is_held_to_what_every_item_is_a_record_that_names_no_id_fails_the_look() {
    let unnamed = event(json!({ "type": "item_completed", "turn_id": "t1", "item": {
        "type": "UserMessage", "content": [{ "type": "text", "text": "hi" }] } }));
    assert_eq!(
        read(&[started(json!("t1")), model("t1", "msg_1", "hi"), unnamed]).unwrap_err(),
        "missing native codex item id at record 2"
    );
}
