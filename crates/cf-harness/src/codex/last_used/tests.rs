//! What a Codex thread ran on last, in the shapes Codex 0.165.0 writes its rollout.

use serde_json::{json, Value};

use super::used_in;
use crate::contract::Used;

fn turn(model: &str, effort: &str) -> Value {
    json!({ "type": "turn_context", "payload": { "model": model, "effort": effort, "cwd": "/w" } })
}

fn applied(model: &str, effort: &str) -> Value {
    json!({ "type": "event_msg", "payload": { "type": "thread_settings_applied",
        "thread_settings": { "model": model, "reasoning_effort": effort } } })
}

fn said(text: &str) -> Value {
    json!({ "type": "response_item", "payload": { "type": "message", "role": "assistant",
        "content": [{ "type": "output_text", "text": text }] } })
}

fn read(entries: &[Value]) -> Option<Used> {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("rollout.jsonl");
    let text: String = entries.iter().map(|entry| format!("{entry}\n")).collect();
    std::fs::write(&file, text).unwrap();
    used_in(&file, 1 << 20).unwrap()
}

fn used(model: &str, effort: &str) -> Option<Used> {
    Some(Used {
        model: model.to_owned(),
        effort: Some(effort.to_owned()),
        thinking: None,
        by_harness: false,
    })
}

#[test]
fn the_last_turns_context_says_the_model_and_the_effort() {
    let entries = [
        turn("gpt-6-luna", "low"),
        said("Hi"),
        turn("gpt-6-luna", "low"),
        said("Done"),
    ];
    assert_eq!(read(&entries), used("gpt-6-luna", "low"));
}

#[test]
fn a_switch_applied_before_any_turn_of_it_is_the_model_used_last() {
    let entries = [
        turn("gpt-6-luna", "low"),
        said("Hi"),
        applied("gpt-6.1-sol", "medium"),
    ];
    assert_eq!(read(&entries), used("gpt-6.1-sol", "medium"));
    // And the next turn's context, once it begins, says the same.
    let entries = [
        turn("gpt-6-luna", "low"),
        applied("gpt-6.1-sol", "medium"),
        turn("gpt-6.1-sol", "medium"),
        said("Done"),
    ];
    assert_eq!(read(&entries), used("gpt-6.1-sol", "medium"));
}

#[test]
fn a_rollout_with_no_turn_yet_says_nothing() {
    assert_eq!(
        read(&[json!({ "type": "session_meta", "payload": {} })]),
        None
    );
}
