//! What a Pi session ran on last, each trap of CHF-05 a test, in Pi 1.4.0's shapes.

use serde_json::{json, Value};

use super::used_in;
use crate::contract::Used;

fn answer(provider: &str, model: &str, thinking: &str, stop: &str) -> Value {
    json!({ "type": "message", "id": "a", "message": { "role": "assistant", "provider": provider,
        "model": model, "thinkingLevel": thinking, "stopReason": stop,
        "content": [{ "type": "text", "text": "Done" }] } })
}

fn asked(text: &str) -> Value {
    json!({ "type": "message", "message": { "role": "user", "content": [{ "type": "text", "text": text }] } })
}

fn model_change(provider: &str, model: &str) -> Value {
    json!({ "type": "model_change", "provider": provider, "modelId": model })
}

fn thinking_change(level: &str) -> Value {
    json!({ "type": "thinking_level_change", "thinkingLevel": level })
}

fn read(entries: &[Value]) -> Option<Used> {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("session.jsonl");
    let text: String = entries.iter().map(|entry| format!("{entry}\n")).collect();
    std::fs::write(&file, text).unwrap();
    used_in(&file, 1 << 20).unwrap()
}

fn used(model: &str, thinking: &str) -> Option<Used> {
    Some(Used {
        model: model.to_owned(),
        effort: None,
        thinking: Some(thinking.to_owned()),
        by_harness: false,
    })
}

#[test]
fn the_thinking_is_the_last_messages_never_the_settings_a_launchs_flags_left_unwritten() {
    // A `--thinking low` launch: Pi wrote the settings' level, then answered at low.
    let entries = [
        model_change("openai", "gpt-6-luna"),
        thinking_change("high"),
        asked("Hi"),
        answer("openai", "gpt-6-luna", "low", "stop"),
    ];
    assert_eq!(read(&entries), used("openai/gpt-6-luna", "low"));
}

#[test]
fn a_native_model_switch_resets_the_thinking_and_its_next_message_says_both() {
    let entries = [
        asked("Hi"),
        answer("openai", "gpt-6-luna", "low", "stop"),
        model_change("openrouter", "anthropic/claude-fable-5"),
        thinking_change("medium"),
        asked("Go on"),
        answer(
            "openrouter",
            "anthropic/claude-fable-5",
            "medium",
            "toolUse",
        ),
    ];
    assert_eq!(
        read(&entries),
        used("openrouter/anthropic/claude-fable-5", "medium")
    );
}

#[test]
fn a_message_that_ended_in_an_error_is_never_the_model_used() {
    let entries = [
        asked("Hi"),
        answer("openai", "gpt-6-luna", "low", "stop"),
        asked("Again"),
        answer("openai", "gpt-gone-9", "low", "error"),
    ];
    assert_eq!(read(&entries), used("openai/gpt-6-luna", "low"));
}

#[test]
fn a_session_with_no_answer_says_nothing() {
    assert_eq!(
        read(&[model_change("openai", "gpt-6-luna"), asked("Hi")]),
        None
    );
}
