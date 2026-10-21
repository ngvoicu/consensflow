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
    assert!(
        whole.held && piecemeal.held,
        "in the turn of its own prompt"
    );
}

#[test]
fn a_log_that_was_replaced_forgets_the_prompts_of_the_one_it_replaced() {
    let log = Log::new();
    log.write(&format!("{}{}", shows("s"), asks(2, "s", "a long prompt")));
    assert_eq!(log.said().asked_at, Some(EPOCH_MS));
    log.write(&shows("s"));
    assert_eq!(log.said().asked_at, None);
}

/// A log replaced is read from its start, and the turns since the paste with it: what
/// the log before said of them (the turn the paste went into, a turn's end since it) is
/// none of the new one's, which says them again where it holds them.
#[test]
fn a_log_that_was_replaced_says_the_turns_since_the_paste_as_the_new_one_has_them() {
    let header = "[ConsensFlow m-9 | T-1 | result from @worker]";
    // The paste's own prompt was asked in the log before: the prompt of other words the
    // new one begins with is no turn the paste went into.
    let log = Log::new();
    log.write(&shows("s"));
    log.wire.expect(header);
    log.append(&asks(2, "s", &format!("{header}\nRESULT_ONE")));
    assert!(log.said().held);
    log.write(&asks(3, "s", "hello"));
    assert!(!log.said().held, "the turn the log before held it in");
    // A turn ended since the paste in the log before: the new one's first prompt read
    // after the paste is one the TUI may have queued it behind, as at the paste.
    let log = Log::new();
    log.write(&shows("s"));
    log.wire.expect(header);
    log.append(&asks(2, "s", "a message of the human's"));
    log.append(&ends("s"));
    assert!(!log.said().held);
    log.write(&format!("{}{}", shows("s"), asks(3, "s", "hi")));
    assert!(log.said().held, "a turn's end of the log before");
    log.append(&ends("s"));
    assert!(!log.said().held);
}

#[test]
fn the_message_last_pasted_is_held_in_the_turn_of_the_tuis_prompt_whose_words_begin_with_it_until_that_turn_ends(
) {
    let log = Log::new();
    let header = "[ConsensFlow m-9 | T-1 | note from ConsensFlow]";
    log.write(&shows("s"));
    log.wire.expect(header);
    assert!(!log.said().held, "nothing of the TUI's yet");
    // What the agent says of the message is not the TUI's prompt of it.
    log.append(&line(&json!({
        "sessionId": "s",
        "update": { "sessionUpdate": "user_message_chunk", "content": { "type": "text", "text": header } },
    })));
    assert!(!log.said().held);
    log.append(&asks(3, "s", &format!("{header}\nthe cancel's words")));
    assert!(log.said().held, "in the turn its own prompt began");
    assert!(log.said().held, "at every look while that turn is on");
    // The turn ends: its record shows it now, and nothing holds it.
    log.append(&answers(3));
    assert!(!log.said().held);
    // The next message is another, pasted at rest: nothing holds it yet.
    log.wire
        .expect("[ConsensFlow m-10 | T-1 | note from ConsensFlow]");
    assert!(!log.said().held);
}

/// The reviews of 2026-10-16: Devin dropped a paste, and the human asked why it was
/// still queued, quoting its header. The words were taken for the paste, and the
/// message was waited for until the window closed.
#[test]
fn a_prompt_that_quotes_the_header_of_a_paste_devin_dropped_holds_it_for_no_longer_than_its_own_turn(
) {
    let log = Log::new();
    let header = "[ConsensFlow m-9 | T-1 | result from @worker]";
    log.write(&shows("s"));
    log.wire.expect(header);
    // The TUI never had the paste. The human's prompt that quotes its header is no
    // paste of it: a turn begun after the paste, which the TUI may have queued it
    // behind, and no longer than that turn.
    log.append(&asks(2, "s", &format!("Why is \"{header}\" still queued?")));
    assert!(log.said().held, "a turn begun after the paste");
    log.append(&ends("s"));
    assert!(
        !log.said().held,
        "the turn is over, and the record has its whole wait"
    );
    // None of the TUI's prompts was the paste, and a turn ended since: the TUI dropped
    // it, and the next turn holds nothing.
    log.append(&asks(3, "s", "and now?"));
    assert!(!log.said().held, "a turn begun after the TUI had the paste");
    log.append(&ends("s"));
    // Words that quote it after a blank line are no paste of it either.
    log.append(&asks(
        4,
        "s",
        &format!("Investigate this:\n\n{header}\nwords"),
    ));
    assert!(!log.said().held);
    log.append(&ends("s"));
    // Its own prompt holds it in the turn it begins, and no longer.
    log.append(&asks(6, "s", &format!("{header}\nRESULT_ONE")));
    assert!(log.said().held);
    log.append(&answers(6));
    assert!(!log.said().held);
}

/// The reviews of 2026-10-16: a paste Devin dropped was held in each turn the human
/// began after it, ten seconds of work and twenty of rest over and over, and never went
/// again. Only the turn it went into holds it, or its own prompt's.
#[test]
fn a_paste_the_tui_dropped_is_held_in_no_turn_begun_after_the_one_it_went_into() {
    let log = Log::new();
    let header = "[ConsensFlow m-9 | T-1 | result from @worker]";
    log.write(&shows("s"));
    log.wire.expect(header);
    // Pasted at rest: the first turn begun after it may be the one the TUI queued it
    // behind.
    log.append(&asks(2, "s", "a message of the human's"));
    assert!(log.said().held, "the turn it may have gone into");
    log.append(&ends("s"));
    assert!(!log.said().held);
    // The TUI did not put it to its agent when that turn ended: it dropped it, and the
    // turns the human begins after hold nothing, by the turn's end or by the answer.
    for id in 3..13 {
        log.append(&asks(id, "s", "and now?"));
        assert!(!log.said().held, "turn {id}");
        if id % 2 == 0 {
            log.append(&ends("s"));
        } else {
            log.append(&answers(id));
        }
    }
    // A paste made while a prompt is asked went into its turn, and no turn after it.
    let next = "[ConsensFlow m-10 | T-1 | result from @worker]";
    log.append(&asks(13, "s", "busy"));
    assert_eq!(log.said().asked_at, Some(EPOCH_MS), "a look reads the turn");
    log.wire.expect(next);
    assert!(log.said().held, "the turn it went into");
    log.append(&ends("s"));
    log.append(&asks(14, "s", "and now?"));
    assert!(!log.said().held, "a turn begun after it");
    log.append(&ends("s"));
    // Its own prompt holds it still, whenever the TUI puts it to its agent.
    log.append(&asks(15, "s", &format!("{next}\nRESULT_TWO")));
    assert!(log.said().held, "its own prompt's turn");
    log.append(&ends("s"));
    assert!(!log.said().held);
}

#[test]
fn a_message_pasted_while_a_prompt_is_asked_is_held_until_no_prompt_is_and_in_its_own_prompts_turn_and_no_other_after_it(
) {
    let log = Log::new();
    let header = "[ConsensFlow m-9 | T-1 | result from @worker]";
    log.write(&format!("{}{}", shows("s"), asks(2, "s", "hello")));
    assert_eq!(log.said().asked_at, Some(EPOCH_MS));
    // The turn the prompt began is at work, its agent silent as it may be: the paste
    // is held in it.
    log.wire.expect(header);
    assert!(log.said().held, "held");
    assert!(log.said().held, "held at every look while the turn is on");
    // The turn ends: held no longer. The TUI puts the paste to its agent, in the turn
    // of its own prompt, and the turn of a prompt asked after it is none the paste
    // went into.
    log.append(&ends("s"));
    assert!(!log.said().held);
    log.append(&asks(3, "s", &format!("{header}\nRESULT_ONE")));
    assert!(log.said().held, "in its own prompt's turn");
    log.append(&ends("s"));
    log.append(&asks(4, "s", "a message of the human's"));
    assert!(!log.said().held, "a turn the paste did not go into");
    // A paste made at rest (a look read the turn's end) is held by nothing but its own
    // prompt's turn, nor by a turn that begins after the TUI put it to its agent, once
    // that turn is over.
    log.append(&ends("s"));
    assert_eq!(log.said().asked_at, None);
    let next = "[ConsensFlow m-10 | T-1 | result from @worker]";
    log.wire.expect(next);
    assert!(!log.said().held);
    log.append(&asks(5, "s", &format!("{next}\nRESULT_TWO")));
    log.append(&asks(6, "s", "another message of the human's"));
    assert!(log.said().held, "its own prompt's turn is on");
    log.append(&ends("s"));
    log.append(&asks(7, "s", "a third message of the human's"));
    assert!(!log.said().held);
}

#[test]
fn a_prompt_read_after_a_paste_the_tui_has_not_put_to_its_agent_holds_the_paste_in_its_turn() {
    let log = Log::new();
    let header = "[ConsensFlow m-9 | T-1 | result from @worker]";
    // Nothing pasted yet: a turn holds nothing.
    log.write(&format!("{}{}", shows("s"), asks(1, "s", "hello")));
    assert!(!log.said().held);
    // The look before the paste read the window at rest.
    log.append(&ends("s"));
    assert_eq!(log.said().asked_at, None);
    log.wire.expect(header);
    // A prompt of the human's the TUI put to its agent before it had the paste, which
    // it queues behind the turn (a review of 2026-10-16).
    log.append(&asks(2, "s", "a message of the human's"));
    assert!(log.said().held, "held in the human's turn");
    // The turn ends, and the TUI puts the paste to its agent.
    log.append(&ends("s"));
    assert!(!log.said().held);
    log.append(&asks(3, "s", &format!("{header}\nwords")));
    assert!(log.said().held, "in its own prompt's turn");
    log.append(&ends("s"));
    assert!(!log.said().held);
}

#[test]
fn a_prompt_of_the_conversation_the_window_showed_is_no_turn_of_the_one_it_shows_now() {
    let log = Log::new();
    log.write(&format!("{}{}", shows("s"), asks(2, "s", "hello")));
    assert_eq!(log.said().asked_at, Some(EPOCH_MS));
    log.wire
        .expect("[ConsensFlow m-9 | T-1 | note from ConsensFlow]");
    assert!(log.said().held);
    // The human opened another conversation in the window: its prompt is not asked,
    // and nothing is held in the turn it began.
    log.append(&shows("t"));
    assert_eq!(log.said().asked_at, None);
    assert!(!log.said().held);
    // Nor is it when the window shows the first again: the turn it began is not this one.
    log.append(&shows("s"));
    assert_eq!(log.said().asked_at, None);
    // A window that says again which conversation it shows forgets none of its prompts.
    log.append(&asks(3, "s", "again"));
    log.append(&shows("s"));
    assert_eq!(log.said().asked_at, Some(EPOCH_MS));
}

#[test]
fn the_paste_is_the_text_of_its_prompt_after_the_space_it_begins_with_and_one_with_quotes_is_found_whole(
) {
    let log = Log::new();
    let header = "[ConsensFlow m-9 | T-1 | \"quoted\" \\ back]";
    let paste = format!("{header}\nwords");
    log.write(&shows("s"));
    log.wire.expect(&format!("{paste}\n"));
    log.append(&asks(3, "s", &format!("\n{paste}")));
    assert!(log.said().sent, "its own prompt");
    assert!(log.said().held, "in its own prompt's turn");
    log.append(&answers(3));
    // The TUI put it to its agent: a prompt of other words after it is no turn it waits
    // behind.
    log.append(&asks(4, "s", "anything"));
    assert!(!log.said().held);
    // An empty paste, which would begin every text, is nothing expected: no prompt
    // holds a paste.
    log.append(&answers(4));
    log.wire.expect("  \n");
    log.append(&asks(5, "s", "anything"));
    assert!(!log.said().held);
    assert!(!log.said().sent);
}

/// The reviews of 2026-10-18: the human's Enter reached Devin just before the paste, and
/// the turn it began held the paste; no look came until after it ended. The look after
/// says a turn that held it ended, once, whether or not a look read it held.
#[test]
fn a_turn_that_held_the_paste_and_ended_is_said_released_once_whether_or_not_a_look_saw_it_held() {
    let log = Log::new();
    let header = "[ConsensFlow m-9 | T-1 | result from @worker]";
    log.write(&shows("s"));
    assert!(!log.wire.released(), "nothing pasted");
    log.wire.expect(header);
    // Begun as the paste went in, and over before the next look read the log.
    log.append(&asks(2, "s", "a message of the human's"));
    log.append(&ends("s"));
    assert!(!log.said().held);
    assert!(log.wire.released(), "a turn that held it ended");
    assert!(!log.wire.released(), "said once");
    // A turn begun after a turn ended held nothing, and its end releases nothing.
    log.append(&asks(3, "s", "and now?"));
    log.append(&ends("s"));
    let _ = log.said();
    assert!(!log.wire.released());
    // A turn a look saw holding a paste is said released when it ends, by the look after.
    log.append(&asks(4, "s", "busy"));
    let _ = log.said();
    log.wire
        .expect("[ConsensFlow m-10 | T-1 | result from @worker]");
    assert!(log.said().held);
    assert!(!log.wire.released(), "held still");
    log.append(&answers(4));
    let _ = log.said();
    assert!(log.wire.released());
    // A new paste forgets what the last one's turns said.
    log.append(&asks(5, "s", "busy again"));
    let _ = log.said();
    log.wire
        .expect("[ConsensFlow m-11 | T-1 | result from @worker]");
    log.append(&ends("s"));
    let _ = log.said();
    log.wire
        .expect("[ConsensFlow m-12 | T-1 | result from @worker]");
    assert!(
        !log.wire.released(),
        "the turn held the paste before the last"
    );
}

/// A prompt of the TUI's of `session`, its parts as given.
fn asks_in_parts(id: i64, session: &str, parts: &[Value]) -> String {
    line(&json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "session/prompt",
        "params": { "sessionId": session, "prompt": parts },
    }))
}

/// The reviews of 2026-10-18 (Astra and Sol on B5): the wire said the TUI put a paste to
/// its agent when any text of any request began with the paste's first line, so a
/// human's prompt in two parts, the second quoting the header of a paste Devin dropped,
/// was taken for it, and the paste was waited for for good. Only a prompt of the
/// conversation shown whose text, its parts joined as Devin's record joins them, begins
/// with the whole paste is it.
#[test]
fn the_tui_put_the_paste_to_its_agent_only_in_a_prompt_of_the_conversation_shown_whose_joined_text_begins_with_the_whole_paste(
) {
    let header = "[ConsensFlow m-9 · T-1 · result from @worker]";
    let paste = format!("{header}\nRESULT_ONE\nthe rest");
    let fresh = || {
        let log = Log::new();
        log.write(&shows("s"));
        log.wire.expect(&paste);
        log
    };
    // A human's prompt whose later part quotes the paste whole.
    let log = fresh();
    log.append(&asks_in_parts(
        2,
        "s",
        &[
            json!({ "type": "text", "text": "Why is this still queued?\n" }),
            json!({ "type": "text", "text": paste }),
        ],
    ));
    assert!(!log.said().sent, "a later part that quotes it");
    // A request of the TUI's that is no prompt, its words the paste's.
    let log = fresh();
    log.append(&line(&json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "session/set_title",
        "params": {
            "sessionId": "s",
            "title": paste,
            "prompt": [{ "type": "text", "text": paste }],
        },
    })));
    assert!(!log.said().sent, "a request that is no prompt");
    // A prompt of another conversation than the one shown.
    let log = fresh();
    log.append(&asks(4, "another", &paste));
    assert!(!log.said().sent, "another conversation's prompt");
    // A prompt that begins with the header, and other words after it.
    let log = fresh();
    log.append(&asks(5, "s", &format!("{header}\nsomething else")));
    assert!(!log.said().sent, "the header with other words");
    // A part that is no text is no words of the prompt, and the parts are joined as they
    // come: the paste, whole, split over two of them.
    let log = fresh();
    let (first, rest) = paste.split_at(header.len() + 4);
    log.append(&asks_in_parts(
        6,
        "s",
        &[
            json!({ "type": "image", "data": "", "mimeType": "image/png" }),
            json!({ "type": "text", "text": format!("\n{first}") }),
            json!({ "type": "text", "text": format!("{rest}\n") }),
        ],
    ));
    assert!(log.said().sent, "its own prompt");
    assert!(log.said().held, "in the turn its own prompt began");
}
