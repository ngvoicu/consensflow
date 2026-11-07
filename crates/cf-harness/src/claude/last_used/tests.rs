//! What a Claude Code conversation ran on last, each trap of CHF-05 a test. The entries are
//! shaped as Claude Code 2.1.312 writes them (CHF-02's probes); none holds a real message.

use serde_json::{json, Value};

use super::used_in;
use crate::contract::Used;

const BOUND: u64 = 1 << 20;

fn assistant(model: &str, effort: Option<&str>) -> Value {
    json!({
        "type": "assistant", "isSidechain": false, "effort": effort, "requestedModel": model,
        "message": { "model": model, "role": "assistant", "content": [{ "type": "text", "text": "Done" }] },
    })
}

fn user(text: &str) -> Value {
    json!({ "type": "user", "isSidechain": false, "message": { "role": "user", "content": text } })
}

fn read(entries: &[Value], asked: Option<&str>) -> Option<Used> {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("session.jsonl");
    let text: String = entries.iter().map(|entry| format!("{entry}\n")).collect();
    std::fs::write(&file, text).unwrap();
    used_in(&file, asked, BOUND).unwrap()
}

fn used(model: &str, effort: Option<&str>, by_harness: bool) -> Option<Used> {
    Some(Used {
        model: model.to_owned(),
        effort: effort.map(str::to_owned),
        thinking: None,
        by_harness,
    })
}

#[test]
fn the_last_assistant_entry_says_the_model_and_the_effort_a_flagless_resume_would_lose() {
    let entries = [
        user("Hi"),
        assistant("claude-haiku-5-5", Some("low")),
        user("More"),
        assistant("claude-haiku-5-5", Some("low")),
    ];
    assert_eq!(
        read(&entries, Some("claude-haiku-5-5")),
        used("claude-haiku-5-5", Some("low"), false)
    );
}

#[test]
fn a_switch_shows_only_at_the_next_assistant_entry() {
    // `/model` then nothing answered yet: the command's echoes name no model.
    let switched = [
        user("Hi"),
        assistant("claude-opus-5-5", Some("high")),
        user("<command-name>/model</command-name>"),
        user("<local-command-stdout>Set model to Sonnet 5.5</local-command-stdout>"),
    ];
    assert_eq!(
        read(&switched, Some("claude-opus-5-5")),
        used("claude-opus-5-5", Some("high"), false)
    );
    let answered = [
        switched.as_slice(),
        &[
            user("Go on"),
            assistant("claude-sonnet-5-5", Some("medium")),
        ],
    ]
    .concat();
    assert_eq!(
        read(&answered, Some("claude-opus-5-5")),
        used("claude-sonnet-5-5", Some("medium"), false)
    );
}

#[test]
fn a_sidechain_a_synthetic_entry_and_an_api_error_are_never_the_model_used() {
    let mut sidechain = assistant("claude-haiku-5-5", Some("low"));
    sidechain["isSidechain"] = json!(true);
    // What Claude Code writes itself to close a turn that was cut: no model ran it, and no
    // error says so.
    let synthetic = assistant("<synthetic>", None);
    let mut refused = assistant("claude-gone-9-9", None);
    refused["isApiErrorMessage"] = json!(true);
    let entries = [
        user("Hi"),
        assistant("claude-opus-5-5", Some("high")),
        sidechain,
        synthetic,
        refused,
    ];
    assert_eq!(
        read(&entries, Some("claude-gone-9-9")),
        used("claude-opus-5-5", Some("high"), false)
    );
}

#[test]
fn a_fallback_claude_code_made_by_itself_is_the_harnesss_and_one_the_human_made_after_is_theirs() {
    let fell_back = json!({
        "type": "assistant", "isSidechain": false,
        "message": { "model": "claude-sonnet-5-5", "content": [
            { "type": "fallback", "from": { "model": "claude-opus-5-5" }, "to": { "model": "claude-sonnet-5-5" } }
        ] },
    });
    let entries = [
        user("Hi"),
        assistant("claude-opus-5-5", Some("high")),
        fell_back,
        assistant("claude-sonnet-5-5", Some("high")),
        assistant("claude-sonnet-5-5", Some("high")),
    ];
    assert_eq!(
        read(&entries, Some("claude-opus-5-5")),
        used("claude-sonnet-5-5", Some("high"), true)
    );
    // The human's `/model` back to Sonnet after it, with an Opus turn between: theirs.
    let chosen = [
        entries.as_slice(),
        &[
            assistant("claude-opus-5-5", Some("high")),
            user("<command-name>/model</command-name>"),
            assistant("claude-sonnet-5-5", Some("low")),
        ],
    ]
    .concat();
    assert_eq!(
        read(&chosen, Some("claude-opus-5-5")),
        used("claude-sonnet-5-5", Some("low"), false)
    );
}

#[test]
fn a_model_that_drifted_with_no_word_of_claude_codes_is_taken_as_the_record_says() {
    // An alias repointed under a running session (T-816): told, never held.
    let entries = [
        user("Hi"),
        assistant("claude-opus-5", Some("high")),
        assistant("claude-opus-5-5", Some("high")),
    ];
    assert_eq!(
        read(&entries, Some("claude-opus-5")),
        used("claude-opus-5-5", Some("high"), false)
    );
}

#[test]
fn a_record_with_no_assistant_entry_or_none_within_the_bound_says_nothing() {
    assert_eq!(read(&[user("Hi")], None), None);
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("session.jsonl");
    let long = user(&"x".repeat(4096));
    let text = format!("{}\n{long}\n", assistant("claude-opus-5-5", None));
    std::fs::write(&file, text).unwrap();
    assert_eq!(used_in(&file, None, 2048).unwrap(), None);
}

/// The walk back from a model the launch did not ask for ends at the human's own `/model`:
/// what came before it is no word of how the newest model came (Calliope's P3, T-955), and
/// a 16 MB tail is not parsed line by line past it.
#[test]
fn the_walk_back_ends_at_the_humans_own_switch_whatever_came_before_it() {
    let mut fell_back = assistant("claude-sonnet-5-5", None);
    fell_back["message"]["content"] = json!([{
        "type": "fallback",
        "from": { "model": "claude-opus-5-5" },
        "to": { "model": "claude-sonnet-5-5" },
    }]);
    let entries = [
        user("Hi"),
        assistant("claude-opus-5-5", None),
        fell_back,
        user("<command-name>/model</command-name>"),
        user("<local-command-stdout>Set model to Sonnet 5.5</local-command-stdout>"),
        user("More"),
        assistant("claude-sonnet-5-5", Some("low")),
    ];
    assert_eq!(
        read(&entries, Some("claude-opus-5-5")),
        used("claude-sonnet-5-5", Some("low"), false),
        "the human chose it, after Claude Code's own move"
    );
}

/// A command's echo and its output as Claude Code 2.1.316 writes those of a `/model` the
/// human cancelled: `system` entries of subtype `local_command` (model_cancel.py).
fn local_command(content: &str) -> Value {
    json!({ "type": "system", "subtype": "local_command", "isSidechain": false, "content": content })
}

/// A `/model` the human opened and cancelled chose nothing: the walk back goes on past it
/// to Claude Code's own fallback, which is never taken for the human's choice (Calliope's
/// P3, T-994). It stops at the line that says a model was set, as 2.1.316 writes one.
#[test]
fn a_cancelled_model_command_is_no_choice_and_the_walk_back_finds_the_fallback_before_it() {
    let mut fell_back = assistant("claude-sonnet-5-5", None);
    fell_back["message"]["content"] = json!([{
        "type": "fallback",
        "from": { "model": "claude-opus-5-5" },
        "to": { "model": "claude-sonnet-5-5" },
    }]);
    let entries = [
        user("Hi"),
        assistant("claude-opus-5-5", None),
        fell_back,
        local_command("<command-name>/model</command-name>\n            <command-message>model</command-message>\n            <command-args></command-args>"),
        local_command("<local-command-stdout>Kept model as `Sonnet 5.5`</local-command-stdout>"),
        user("More"),
        assistant("claude-sonnet-5-5", Some("low")),
    ];
    assert_eq!(
        read(&entries, Some("claude-opus-5-5")),
        used("claude-sonnet-5-5", Some("low"), true),
        "Claude Code's own move, the human's cancel no choice"
    );
}
