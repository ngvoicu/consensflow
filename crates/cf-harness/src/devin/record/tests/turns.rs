//! How a turn reads on Devin's store, now that its wire log holds no request of its TUI's
//! (T-102, Devin 3000.11.92, probed live): a message of the user's the agent has not
//! answered reads complete, at rest, until the agent's first word (the window reads it
//! as a turn begun: `adapter`'s `begun_on_the_store`); and the shape the app's Stop
//! leaves behind a paste queued in a turn reads at rest, the paste kept in the
//! conversation.

use super::*;
use crate::shared::record::reading::Role;
use crate::shared::record_state::record_state;

fn from(client: &str) -> Value {
    json!({ "metadata": { "extensions": { "chisel/client-message-id": client } } })
}

fn said_by_user(id: &str, client: &str, text: &str) -> Value {
    let mut message = json!({ "message_id": id, "role": "user", "content": text });
    message["metadata"] = from(client)["metadata"].clone();
    message
}

/// What the wire says as the agent takes in the user's message `client`.
fn taken_in(client: &str, text: &str) -> Value {
    json!({
        "sessionId": SESSION,
        "update": {
            "sessionUpdate": "user_message_chunk",
            "content": { "type": "text", "text": text },
            "_meta": { "cognition.ai/clientMessageId": client },
        },
    })
}

#[test]
fn a_message_of_the_users_its_agent_has_not_answered_reads_at_rest_until_the_agents_first_word() {
    let staged = Staged::new();
    staged.chain(&[said_by_user(
        "u-1",
        "c-1",
        "[ConsensFlow m-9 · note]\nwords",
    )]);
    staged.wire(
        "launch-a",
        &[taken_in("c-1", "[ConsensFlow m-9 · note]\nwords")],
    );
    assert!(record_state(look(&mut staged.reader())).settled);
    // The agent's first word: at work.
    staged.wire(
        "launch-a",
        &[
            taken_in("c-1", "[ConsensFlow m-9 · note]\nwords"),
            json!({ "sessionId": SESSION, "update": { "sessionUpdate": "agent_thought_chunk" } }),
        ],
    );
    assert!(!record_state(look(&mut staged.reader())).settled);
}

/// T-102 (3 of 3 runs on 3000.11.92): a paste queued behind a turn, then the app's Stop.
/// The turn is cancelled and the paste never runs (the wire has no `user_message_chunk`
/// of it), but Devin keeps it in the conversation, the head of its main chain, and its
/// agent reads it in the next turn. The record reads at rest, and holds the paste.
#[test]
fn a_paste_the_stop_cancelled_while_queued_is_kept_in_the_record_which_reads_at_rest() {
    let staged = Staged::new();
    staged.chain(&[
        said_by_user("u-4", "c-4", "Write the integers from 1 to 600."),
        reply("a-4", json!("1\n2\n3")),
        said_by_user("u-5", "c-5", "[ConsensFlow m-9 · note]\nwords"),
    ]);
    staged.wire(
        "launch-a",
        &[
            taken_in("c-4", "Write the integers from 1 to 600."),
            json!({
                "sessionId": SESSION,
                "update": {
                    "sessionUpdate": "agent_message_chunk",
                    "content": { "type": "text", "text": "1\n2\n3" },
                    "_meta": { "cognition.ai/streamingMessageId": "stream-4" },
                },
            }),
            json!({ "sessionId": SESSION, "turnClientMessageId": "c-4", "turnRequestId": "r-4" }),
            json!({ "sessionId": SESSION, "turnClientMessageId": "c-5" }),
            json!({ "sessionId": SESSION, "cause": "cancelled", "stats": {} }),
        ],
    );
    let reading = look(&mut staged.reader());
    let shown = show(&reading);
    assert!(
        shown.ends_with(r#""settlement":"settled"}"#),
        "the cancelled turn's end: {shown}"
    );
    let Reading::Known(record) = &*reading else {
        panic!("{shown}")
    };
    let last = record.items.last().unwrap();
    assert_eq!(
        (last.role, &*last.text),
        (Role::User, "[ConsensFlow m-9 · note]\nwords"),
        "the paste is in the record"
    );
    assert!(record_state(Arc::clone(&reading)).settled);
}
