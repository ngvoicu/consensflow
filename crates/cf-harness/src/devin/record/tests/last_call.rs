//! The last tool call Devin's main chain shows (T-823): what a window was doing when it
//! closed under its task, which its screen does not say. Devin can leave a call that nothing
//! has answered below the head of its main chain (the stamped fixtures); that call is what a
//! window that died mid-call was doing, and the chain's own rows hold only the calls that
//! were answered (the review of `a26e0848e`, P2).

use super::*;
use crate::records::LastCall;
use std::sync::Arc;

/// A reply of Devin's that calls `tool` with `arguments` (JSON text, as Devin stores it).
fn calling(id: &str, call: &str, tool: &str, arguments: &str) -> Value {
    let mut message = reply(id, json!("Working"));
    message["tool_calls"] = json!([{ "id": call, "name": tool, "arguments": arguments, "index": 0, "kind": "function" }]);
    message
}

fn output(id: &str, call: &str) -> Value {
    json!({ "message_id": id, "role": "tool", "content": "ok", "tool_call_id": call })
}

fn last_call_in(reading: &Reading) -> Option<Arc<LastCall>> {
    let Reading::Known(record) = reading else {
        panic!("a reading");
    };
    record.last_call.clone()
}

/// The last call a look at a chain of `messages` reads.
fn last_call_of(messages: &[Value]) -> Option<Arc<LastCall>> {
    let staged = Staged::new();
    staged.chain(messages);
    last_call_in(&look(&mut staged.reader()))
}

#[test]
fn the_last_call_is_the_exec_command_or_the_tool_and_its_file_the_latest_winning() {
    assert_eq!(
        last_call_of(&[
            user("u-1", Some(json!("request-1"))),
            calling(
                "a-1",
                "c-1",
                "exec",
                r#"{"command":"cargo test --workspace","timeout":600}"#
            ),
        ]),
        Some(LastCall::shell("exec", "cargo test --workspace"))
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
        ]),
        Some(LastCall::on("read", "src/lib.rs"))
    );
    assert_eq!(
        last_call_of(&[user("u-1", None), reply("a-1", json!("Hi"))]),
        None
    );
}

#[test]
fn a_call_nothing_answered_below_the_head_of_the_chain_is_the_last_call() {
    // The user's message is the head; the assistant's call hangs below it, as the stamped
    // fixtures have a call Devin's question tool waited on.
    let staged = Staged::new();
    staged.node(Some("n-1"), None, &user("u-1", Some(json!("request-1"))));
    staged.node(
        Some("n-2"),
        Some("n-1"),
        &calling(
            "a-1",
            "c-1",
            "exec",
            r#"{"command":"cargo test --workspace"}"#,
        ),
    );
    staged.head("n-1");
    assert_eq!(
        last_call_in(&look(&mut staged.reader())),
        Some(LastCall::shell("exec", "cargo test --workspace"))
    );
}

#[test]
fn a_call_that_comes_below_the_head_between_two_looks_is_read_on_the_next() {
    let staged = Staged::new();
    staged.node(Some("n-1"), None, &user("u-1", Some(json!("request-1"))));
    staged.head("n-1");
    let mut reader = staged.reader();
    assert_eq!(last_call_in(&look(&mut reader)), None);
    staged.node(
        Some("n-2"),
        Some("n-1"),
        &calling("a-1", "c-1", "exec", r#"{"command":"make build"}"#),
    );
    assert_eq!(
        last_call_in(&look(&mut reader)),
        Some(LastCall::shell("exec", "make build")),
        "the next look of the same reader"
    );
    // A call below that one, the newest child after the newest, wins.
    staged.node(
        Some("n-3"),
        Some("n-2"),
        &calling("a-2", "c-2", "read", r#"{"file_path":"src/main.rs"}"#),
    );
    assert_eq!(
        last_call_in(&look(&mut reader)),
        Some(LastCall::on("read", "src/main.rs"))
    );
}

#[test]
fn a_call_on_another_branch_or_in_another_session_is_no_call_of_this_window() {
    let staged = Staged::new();
    staged.node(Some("n-1"), None, &user("u-1", Some(json!("request-1"))));
    // Two children of the head: the newest is the one followed, and the older is a branch
    // the chain does not take.
    staged.node(
        Some("n-2"),
        Some("n-1"),
        &calling("a-1", "c-1", "exec", r#"{"command":"rm -rf target"}"#),
    );
    staged.node(
        Some("n-3"),
        Some("n-1"),
        &calling("a-2", "c-2", "exec", r#"{"command":"cargo build"}"#),
    );
    // A row of another session, below the same node name.
    staged
        .store
        .execute(
            "INSERT INTO message_nodes (session_id, node_id, parent_node_id, chat_message, created_at)
             VALUES ('another-session', 'n-4', 'n-3', ?, '2026-10-04T10:00:00Z')",
            [calling("a-3", "c-3", "exec", r#"{"command":"curl evil.test"}"#).to_string()],
        )
        .unwrap();
    staged.head("n-1");
    assert_eq!(
        last_call_in(&look(&mut staged.reader())),
        Some(LastCall::shell("exec", "cargo build"))
    );
}
