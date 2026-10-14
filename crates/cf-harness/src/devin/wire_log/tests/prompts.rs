//! What the wire says of a turn before its agent has said a word, and of a message
//! its agent's queue holds: the TUI's own requests, read for the prompts it puts to
//! its agent and for the text of the message last pasted.

use super::*;

/// A prompt of the TUI to its agent, as its request is: the conversation in its
/// params, with the words of the message.
fn asks(id: i64, session: &str, text: &str) -> String {
    line(&json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "session/prompt",
        "params": { "sessionId": session, "prompt": [{ "type": "text", "text": text }] },
    }))
}

/// The end of a turn of `session`, as its agent says it.
fn ends(session: &str) -> String {
    line(&json!({ "cause": "complete", "stats": { "ttftMs": 1 }, "sessionId": session }))
}

/// The agent's answer to a request.
fn answers(id: i64) -> String {
    line(&json!({ "jsonrpc": "2.0", "id": id, "result": { "stopReason": "end_turn" } }))
}

#[test]
fn a_prompt_the_tui_puts_to_its_agent_is_asked_from_the_look_that_reads_it_until_a_turn_ends() {
    let log = Log::new();
    log.write(&shows("s"));
    assert_eq!(log.said().asked_at, None);
    log.append(&asks(2, "s", "hello"));
    let at = EPOCH_MS + 4_000;
    assert_eq!(log.wire.read(at).unwrap().asked_at, Some(at));
    // The agent's work on it ends nothing: the turn's end does.
    log.append(&says("hi"));
    assert_eq!(
        log.wire.read(at + 9_000).unwrap().asked_at,
        Some(at),
        "as the look that first read it has it"
    );
    log.append(&ends("s"));
    assert_eq!(log.wire.read(at + 12_000).unwrap().asked_at, None);
}

#[test]
fn a_prompt_is_closed_by_the_answer_to_its_request_by_the_id_and_by_no_other() {
    let log = Log::new();
    log.write(&format!(
        "{}{}{}",
        shows("s"),
        asks(2, "s", "a"),
        asks(3, "s", "b")
    ));
    let at = EPOCH_MS + 1_000;
    assert_eq!(log.wire.read(at).unwrap().asked_at, Some(at));
    // An answer to a request that was not a prompt of its own: the prompts wait.
    log.append(&answers(1));
    assert_eq!(log.wire.read(at + 1).unwrap().asked_at, Some(at));
    log.append(&answers(3));
    assert_eq!(
        log.wire.read(at + 2).unwrap().asked_at,
        Some(at),
        "the one asked first is asked still"
    );
    log.append(&line(
        &json!({ "jsonrpc": "2.0", "id": 2, "error": { "code": -32011, "message": "Quota exhausted." } }),
    ));
    assert_eq!(log.wire.read(at + 3).unwrap().asked_at, None);
}

#[test]
fn only_a_prompt_of_the_conversation_shown_is_asked_and_only_its_turns_end_closes_it() {
    let log = Log::new();
    // A prompt before the window says which conversation it shows is none of it.
    log.write(&asks(2, "s", "early"));
    assert_eq!(log.said().asked_at, None);
    log.append(&format!(
        "{}{}",
        shows("s"),
        asks(3, "another", "elsewhere")
    ));
    assert_eq!(log.said().asked_at, None);
    log.append(&asks(4, "s", "here"));
    assert_eq!(log.said().asked_at, Some(EPOCH_MS));
    log.append(&ends("another"));
    assert_eq!(log.said().asked_at, Some(EPOCH_MS));
    // A request that is no prompt asks nothing.
    let log = Log::new();
    log.write(&format!(
        "{}{}",
        shows("s"),
        line(&json!({ "jsonrpc": "2.0", "id": 5, "method": "session/cancel", "params": { "sessionId": "s" } }))
    ));
    assert_eq!(log.said().asked_at, None);
}

#[test]
fn a_log_read_on_a_line_at_a_time_says_what_the_same_log_read_whole_says_of_its_prompts() {
    let header = "[ConsensFlow m-9 | T-1 | note from ConsensFlow]";
    let lines = [
        shows("s"),
        asks(2, "s", "first"),
        says("hi"),
        ends("s"),
        asks(3, "s", &format!("{header}\nwords")),
    ];
    let whole = Log::new();
    whole.wire.expect(header);
    whole.write(&lines.concat());
    let piecemeal = Log::new();
    piecemeal.wire.expect(header);
    for line in &lines {
        piecemeal.append(line);
        // Each look reads what was appended since the last, and the same at the end.
        let _ = piecemeal.said();
    }
    let (whole, piecemeal) = (whole.said(), piecemeal.said());
    assert_eq!(whole.shown, piecemeal.shown);
    assert_eq!(whole.asked_at, piecemeal.asked_at);
    assert_eq!(
        whole.asked_at,
        Some(EPOCH_MS),
        "only the last prompt is asked"
    );
    assert!(whole.sent && piecemeal.sent);
}

#[test]
fn a_log_that_was_replaced_forgets_the_prompts_of_the_one_it_replaced() {
    let log = Log::new();
    log.write(&format!("{}{}", shows("s"), asks(2, "s", "a long prompt")));
    assert_eq!(log.said().asked_at, Some(EPOCH_MS));
    log.write(&shows("s"));
    assert_eq!(log.said().asked_at, None);
}

#[test]
fn the_message_last_pasted_is_told_sent_by_a_request_of_the_tui_that_holds_its_text() {
    let log = Log::new();
    let header = "[ConsensFlow m-9 | T-1 | note from ConsensFlow]";
    log.write(&shows("s"));
    log.wire.expect(header);
    assert!(!log.said().sent, "nothing of the TUI's yet");
    // What the agent says of the message, and a request that is another's, are not it.
    log.append(&line(&json!({
        "sessionId": "s",
        "update": { "sessionUpdate": "user_message_chunk", "content": { "type": "text", "text": header } },
    })));
    log.append(&asks(
        2,
        "s",
        "[ConsensFlow m-8 | T-1 | note from ConsensFlow]\nwords",
    ));
    assert!(!log.said().sent);
    log.append(&asks(3, "s", &format!("{header}\nthe cancel's words")));
    assert!(log.said().sent);
    assert!(
        log.said().sent,
        "told again, until the next message is pasted"
    );
    // The next message is another: its text is expected, and none is sent yet.
    log.wire
        .expect("[ConsensFlow m-10 | T-1 | note from ConsensFlow]");
    assert!(!log.said().sent);
}

#[test]
fn the_text_is_found_in_a_request_wherever_it_is_whatever_the_method_and_a_text_with_quotes_is_found_whole(
) {
    let log = Log::new();
    let header = "[ConsensFlow m-9 | T-1 | \"quoted\" \\ back]";
    log.write(&shows("s"));
    log.wire.expect(header);
    log.append(&line(&json!({
        "jsonrpc": "2.0",
        "method": "_cognition/queue",
        "params": { "sessionId": "s", "message": { "parts": [{ "text": format!("{header}\nwords") }] } },
    })));
    assert!(log.said().sent);
    // No text expected is no text found, and an empty line holds all.
    log.wire.expect("");
    log.append(&asks(4, "s", "anything"));
    assert!(!log.said().sent);
}
