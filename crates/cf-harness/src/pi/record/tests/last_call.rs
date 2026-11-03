//! The last tool call a session shows (T-823): what a window was doing when it closed under
//! its task, which its screen does not say.

use super::*;

/// An assistant's step that called `tool` with `arguments`.
fn using(id: &str, call: &str, tool: &str, arguments: &Value) -> Value {
    message(
        id,
        json!({ "role": "assistant", "stopReason": "toolUse",
            "content": [{ "type": "toolCall", "id": call, "name": tool, "arguments": arguments }] }),
    )
}

#[test]
fn the_last_call_is_the_bash_command_or_the_tool_and_its_path_the_latest_winning() {
    let mut stage = Stage::with(&[
        header(),
        user("u1", "Go"),
        using(
            "a1",
            "c1",
            "bash",
            &json!({ "command": "cargo test --workspace", "timeout": 600 }),
        ),
    ]);
    assert_eq!(
        stage.at(0).last_call.as_deref(),
        Some("cargo test --workspace"),
        "open"
    );
    stage.write(&[
        header(),
        user("u1", "Go"),
        using(
            "a1",
            "c1",
            "bash",
            &json!({ "command": "cargo test --workspace" }),
        ),
        result("r1", &json!("c1")),
        using(
            "a2",
            "c2",
            "read",
            &json!({ "path": "src/lib.rs", "limit": 20 }),
        ),
    ]);
    assert_eq!(stage.at(0).last_call.as_deref(), Some("read src/lib.rs"));
    // No call in the session says none.
    let mut none = Stage::with(&[header(), user("u1", "Go"), assistant("a1", "stop", "Hi")]);
    assert_eq!(none.at(0).last_call, None);
}
