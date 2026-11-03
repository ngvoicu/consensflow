//! The last tool call a transcript shows, which the note of a window that closed under its
//! task says it was on (T-823, kluris T-153: a Claude Code worker closed with exit code 143
//! three seconds after the `cargo test --workspace` it ran was killed; the transcript had
//! the command, and the note did not).

use super::*;

/// The last call the transcript of `records` shows.
fn last_call_of(records: &[Value]) -> Option<String> {
    let mut stage = Stage::new();
    stage.write(records);
    match &*stage.reader.look(&Options::default(), 0) {
        Reading::Known(record) => record.last_call.clone(),
        Reading::Unknown(reason) | Reading::Missing(reason) => panic!("unknown: {reason}"),
    }
}

/// A call of `tool` with `input`, by `id`.
fn call(id: &str, tool: &str, input: Value) -> Value {
    json!({ "type": "tool_use", "id": id, "name": tool, "input": input })
}

#[test]
fn the_command_a_turn_ran_last_is_the_last_call_whether_or_not_its_result_came() {
    let command = "cargo fmt --check && cargo test --workspace";
    let ran = assistant(
        "a1",
        "u1",
        json!("m1"),
        json!([call(
            "t1",
            "Bash",
            json!({ "command": command, "timeout": 600_000 })
        )]),
        Value::Null,
    );
    // Open: it runs still.
    assert_eq!(
        last_call_of(&[hello(), ran.clone()]).as_deref(),
        Some(command)
    );
    // Answered: the result came ("Exit code 137"), and the window went three seconds later.
    let result = user(
        "u2",
        json!("a1"),
        json!([tool_result(json!("t1"), json!("Exit code 137"))]),
    );
    assert_eq!(
        last_call_of(&[hello(), ran, result]).as_deref(),
        Some(command)
    );
}

#[test]
fn the_latest_call_wins_a_tool_that_is_no_shell_is_its_name_and_main_argument_and_none_is_none() {
    let first = assistant(
        "a1",
        "u1",
        json!("m1"),
        json!([
            call("t1", "Bash", json!({ "command": "make build" })),
            call("t2", "Read", json!({ "file_path": "src/lib.rs" })),
        ]),
        Value::Null,
    );
    assert_eq!(
        last_call_of(&[hello(), first.clone()]).as_deref(),
        Some("Read src/lib.rs"),
        "the last block of a message"
    );
    let later = assistant(
        "a2",
        "a1",
        json!("m2"),
        json!([call("t3", "Bash", json!({ "command": "git status" }))]),
        Value::Null,
    );
    assert_eq!(
        last_call_of(&[hello(), first, later]).as_deref(),
        Some("git status"),
        "the message after"
    );
    assert_eq!(
        last_call_of(&[hello(), answer("a1", "u1")]),
        None,
        "a turn with no call"
    );
}

#[test]
fn a_subagents_call_is_no_call_of_the_window() {
    let main = assistant(
        "a1",
        "u1",
        json!("m1"),
        json!([call("t1", "Bash", json!({ "command": "cargo build" }))]),
        Value::Null,
    );
    let sidechain = having(
        assistant(
            "a2",
            "a1",
            json!("m2"),
            json!([call("t2", "Bash", json!({ "command": "rm -rf /tmp/x" }))]),
            Value::Null,
        ),
        json!({ "isSidechain": true }),
    );
    assert_eq!(
        last_call_of(&[hello(), main, sidechain]).as_deref(),
        Some("cargo build")
    );
}
