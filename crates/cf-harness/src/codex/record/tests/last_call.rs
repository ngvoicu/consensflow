//! The last tool call a rollout shows (T-823): what a window was doing when it closed under
//! its task, which its screen does not say.

use super::*;

/// A function call of `tool` with `arguments`, JSON text as Codex writes it.
fn function_call(id: &str, tool: &str, arguments: &str) -> Value {
    response(json!({
        "type": "function_call", "call_id": id, "name": tool, "arguments": arguments,
    }))
}

#[test]
fn the_last_call_is_the_shells_command_or_the_tools_name_and_input() {
    let last = |records: Vec<Value>| read(&records).unwrap().last_call;
    assert_eq!(
        last(vec![
            started(json!("t1")),
            function_call(
                "c1",
                "shell",
                r#"{"command":["bash","-lc","cargo test --workspace"],"workdir":"/work"}"#
            ),
        ])
        .as_deref(),
        Some("cargo test --workspace")
    );
    assert_eq!(
        last(vec![
            started(json!("t1")),
            function_call(
                "c1",
                "exec_command",
                r#"{"cmd":"make build","yield_time_ms":1000}"#
            ),
            output("c1"),
            function_call("c2", "sleep", r#"{"duration_ms":5000}"#),
        ])
        .as_deref(),
        Some("sleep"),
        "the latest, answered or not"
    );
    // A custom tool's input is text.
    assert_eq!(
        last(vec![
            started(json!("t1")),
            response(json!({
                "type": "custom_tool_call", "call_id": "c1", "name": "apply_patch",
                "input": "*** Begin Patch\n*** Update File: src/lib.rs\n*** End Patch",
            })),
        ])
        .as_deref(),
        Some("apply_patch *** Begin Patch *** Update File: src/lib.rs *** End Patch")
    );
    // A rollout with no call, or a call that names nothing, says none.
    assert_eq!(last(vec![started(json!("t1"))]), None);
    assert_eq!(last(vec![started(json!("t1")), call("c1")]), None);
}
