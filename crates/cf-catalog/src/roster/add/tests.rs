use super::*;
use crate::roster::testing::{body, file_with, sentence, Counted};
use serde_json::json;
use std::fs;

fn catalog() -> Catalog {
    Catalog::bundled().unwrap()
}

/// What adding `input` to a file of these bytes answers, the file after, and how often the clock was read.
fn added(file: &[u8], input: serde_json::Value) -> (Result<AgentView, Refusal>, String, usize) {
    let (_home, path) = file_with(file);
    let mut clock = Counted::new();
    let answer = catalog().add(&path, &body(input), &mut clock);
    (answer, fs::read_to_string(&path).unwrap(), clock.readings)
}

#[test]
fn a_new_agent_is_one_row_in_nodes_key_order_both_stamps_one_reading_of_the_clock() {
    let (answer, file, readings) = added(
        br#"{"schemaVersion":1,"agents":[]}"#,
        json!({ "name": "zed", "harness": "pi", "model": "openrouter/acme/x", "effort": "low", "description": "Mine", "workTier": "complex" }),
    );
    assert_eq!(readings, 1);
    // What Node's addAgent wrote for this input at 2026-10-04T12:00:00.000Z.
    assert_eq!(
        file,
        "{\n  \"schemaVersion\": 1,\n  \"agents\": [\n    {\n      \"id\": \"zed\",\n      \"name\": \"Zed\",\n      \"kind\": \"pi\",\n      \"createdAt\": \"2026-10-04T12:00:00.000Z\",\n      \"updatedAt\": \"2026-10-04T12:00:00.000Z\",\n      \"model\": \"openrouter/acme/x\",\n      \"workTier\": \"complex\",\n      \"thinking\": \"low\",\n      \"description\": \"Mine\"\n    }\n  ]\n}\n"
    );
    let view = answer.unwrap();
    assert!(view.custom);
    assert_eq!(view.effort.as_deref(), Some("low"));
    assert_eq!(view.harness.as_deref(), Some("pi"));
}

#[test]
fn an_image_agent_is_a_designing_codex_agent_and_an_empty_effort_or_description_is_not_kept() {
    let (answer, file, _) = added(
        b"{}",
        json!({ "name": "painter-two", "harness": "codex", "model": "gpt-image-2", "designer": true, "effort": "", "description": "" }),
    );
    assert!(answer.unwrap().designer);
    assert!(file.contains("\"kind\": \"codex\",\n      \"designer\": true,\n      \"createdAt\""));
    assert!(
        !file.contains("effort") && !file.contains("description"),
        "{file}"
    );
}

#[test]
fn each_refusal_is_said_in_the_order_node_checked_and_the_file_is_left_as_it_was() {
    let file = br#"{"agents":[{"id":"nova","kind":"codex","model":"m"}]}"#;
    let cases = [
        (
            json!({ "name": "Bad Name", "harness": "codex", "model": "m" }),
            "agent names are lowercase [a-z0-9-] starting with a letter; got \"Bad Name\"",
        ),
        (
            json!({ "harness": "codex", "model": "m" }),
            "agent names are lowercase [a-z0-9-] starting with a letter; got undefined",
        ),
        (
            json!({ "name": 5, "harness": "codex", "model": "m" }),
            "agent names are lowercase [a-z0-9-] starting with a letter; got 5",
        ),
        (
            json!({ "name": "thoth", "harness": "codex", "model": "m" }),
            "thoth is a catalog agent: pick another name for your own",
        ),
        (
            json!({ "name": "zed", "harness": "kimi", "model": "m" }),
            "unknown harness \"kimi\"; expected claude, codex, pi, opencode, devin",
        ),
        (
            json!({ "name": "zed", "harness": "claude-code", "model": "m" }),
            "unknown harness \"claude-code\"; expected claude, codex, pi, opencode, devin",
        ),
        (
            json!({ "name": "zed", "model": "m" }),
            "unknown harness undefined; expected claude, codex, pi, opencode, devin",
        ),
        (
            json!({ "name": "zed", "harness": "codex", "model": "m", "designer": "yes" }),
            "an agent is an image agent (designer true) or not (false)",
        ),
        (
            json!({ "name": "zed", "harness": "codex", "model": "m", "designer": null }),
            "an agent is an image agent (designer true) or not (false)",
        ),
        (
            json!({ "name": "zed", "harness": "pi", "model": "m", "designer": true }),
            "an image agent is a Codex agent: Codex's image tool draws",
        ),
        (
            json!({ "name": "zed", "harness": "codex", "model": "" }),
            "an agent needs a model (any identifier its harness accepts)",
        ),
        (
            json!({ "name": "zed", "harness": "codex" }),
            "an agent needs a model (any identifier its harness accepts)",
        ),
        (
            json!({ "name": "zed", "harness": "codex", "model": "m", "workTier": "huge" }),
            "Work tier must be critical, complex, standard or light",
        ),
        (
            json!({ "name": "nova", "harness": "codex", "model": "m" }),
            "an agent named nova already exists",
        ),
        (
            json!({ "name": "zed", "harness": "codex", "model": "m", "effort": 7 }),
            "an agent's effort is the name of a level, as text",
        ),
    ];
    for (input, said) in cases {
        let (answer, after, readings) = added(file, input.clone());
        assert_eq!(answer.unwrap_err().message, said, "{input}");
        assert_eq!(after.as_bytes(), file, "{input}: the file as it was");
        assert_eq!(readings, 0, "{input}: no time read");
    }
}

#[test]
fn the_request_is_refused_before_the_file_is_read_and_a_duplicate_after() {
    let (_home, path) = file_with(b"{ broken");
    let catalog = catalog();
    let mut clock = Counted::new();
    let bad_name = catalog.add(&path, &body(json!({ "name": "Bad" })), &mut clock);
    assert!(bad_name
        .unwrap_err()
        .message
        .starts_with("agent names are lowercase"));
    let good = catalog.add(
        &path,
        &body(json!({ "name": "zed", "harness": "codex", "model": "m" })),
        &mut clock,
    );
    assert_eq!(
        good.unwrap_err().message,
        sentence(&path, "is not valid JSON")
    );
    // An effort that is no text is said only of a file that can be read.
    let effort = catalog.add(
        &path,
        &body(json!({ "name": "zed", "harness": "codex", "model": "m", "effort": [] })),
        &mut clock,
    );
    assert_eq!(
        effort.unwrap_err().message,
        sentence(&path, "is not valid JSON")
    );
}
