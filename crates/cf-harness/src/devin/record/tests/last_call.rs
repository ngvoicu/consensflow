//! The last tool call Devin's main chain shows (T-823): what a window was doing when it
//! closed under its task, which its screen does not say.

use super::*;

/// A reply of Devin's that calls `tool` with `arguments` (JSON text, as Devin stores it).
fn calling(id: &str, call: &str, tool: &str, arguments: &str) -> Value {
    let mut message = reply(id, json!("Working"));
    message["tool_calls"] = json!([{ "id": call, "name": tool, "arguments": arguments, "index": 0, "kind": "function" }]);
    message
}

/// The last call a look at a chain of `messages` reads.
fn last_call_of(messages: &[Value]) -> Option<String> {
    let staged = Staged::new();
    staged.chain(messages);
    let reading = look(&mut staged.reader());
    let Reading::Known(record) = &*reading else {
        panic!("a reading");
    };
    record.last_call.clone()
}

#[test]
fn the_last_call_is_the_exec_command_or_the_tool_and_its_file_the_latest_winning() {
    let output = |id: &str, call: &str| json!({ "message_id": id, "role": "tool", "content": "ok", "tool_call_id": call });
    assert_eq!(
        last_call_of(&[
            user("u-1", Some(json!("request-1"))),
            calling(
                "a-1",
                "c-1",
                "exec",
                r#"{"command":"cargo test --workspace","timeout":600}"#
            ),
        ])
        .as_deref(),
        Some("cargo test --workspace")
    );
    assert_eq!(
        last_call_of(&[
            user("u-1", Some(json!("request-1"))),
            calling(
                "a-1",
                "c-1",
                "exec",
                r#"{"command":"cargo test --workspace"}"#
            ),
            output("t-1", "c-1"),
            calling(
                "a-2",
                "c-2",
                "read",
                r#"{"file_path":"src/lib.rs","limit":20}"#
            ),
        ])
        .as_deref(),
        Some("read src/lib.rs")
    );
    assert_eq!(
        last_call_of(&[user("u-1", None), reply("a-1", json!("Hi"))]),
        None
    );
}
