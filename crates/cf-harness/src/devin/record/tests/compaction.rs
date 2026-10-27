//! A turn Devin's async compactor compacted before its answer (T-652, the live board's
//! T-602 of 2026-10-26): a system message that sums the conversation up begins a chain
//! of its own, the turn's steps are copied below it with the ids Devin gave them, and
//! the user's message the turn answers is left outside the main chain. The answer is
//! stored as Devin 3000.11 stores one, with no `finish_reason`, in a model request of
//! its own (`metadata.request_id`); the wire names that request as the turn ends, in the
//! record that names the turn's message (`turnRequestId`) and in the end's statistics
//! (`stats.requestId`).
//!
//! The user's message is read back where it was, found by the copies' originals, so the
//! reply answers it as before the compaction. Where no copy leads back to it (copies of
//! ids of their own, here), only the model request ties the answer to its end, and only
//! where all three name it. A look that read the turn before the compaction reads the
//! same as a fresh one.

use super::*;

/// The model's request the answer is of.
const ANSWERED: &str = "model-2";

/// The turn before the compaction: the user's message, a step that calls a tool, and
/// the tool's answer, with the answer on its way on the wire.
fn before(staged: &Staged) {
    staged.node(Some("n-1"), None, &user("u-1", Some(json!("request-1"))));
    staged.node(Some("n-2"), Some("n-1"), &step());
    staged.node(Some("n-3"), Some("n-2"), &tool());
    staged.head("n-3");
    staged.wire("launch-1", &[chunk(Some(json!("Done.")), "stream-1")]);
}

/// A step of the turn: the assistant's call of its shell, in a model request of its own.
fn step() -> Value {
    json!({
        "message_id": "a-step",
        "role": "assistant",
        "content": "",
        "tool_calls": [{ "id": "call-1", "name": "exec" }],
        "metadata": { "request_id": "model-1", "finish_reason": null },
    })
}

fn tool() -> Value {
    json!({ "message_id": "t-step", "role": "tool", "content": "ok", "tool_call_id": "call-1" })
}

/// The answer `text` as Devin 3000.11 stores one, in the model request `request`.
fn answer(id: &str, text: &str, request: &str) -> Value {
    json!({
        "message_id": id,
        "role": "assistant",
        "content": text,
        "metadata": { "request_id": request, "finish_reason": null },
    })
}

/// The summary Devin's compactor begins a chain with.
fn summary(id: &str) -> Value {
    json!({
        "message_id": id,
        "role": "system",
        "content": "The conversation so far.",
        "metadata": { "extensions": { "devin-rs/summary": { "source": "async_file_compactor", "kept_turns": 2 } } },
    })
}

/// `message` with the id `id`.
fn with_id(mut message: Value, id: &str) -> Value {
    message["message_id"] = json!(id);
    message
}

/// The compaction as Devin makes it, the steps copied with their ids, and the answer
/// below the copies, the main chain's head.
fn compacted(staged: &Staged, answered: &Value) {
    staged.node(Some("c-1"), None, &summary("s-1"));
    staged.node(Some("c-2"), Some("c-1"), &step());
    staged.node(Some("c-3"), Some("c-2"), &tool());
    staged.node(Some("c-4"), Some("c-3"), answered);
    staged.head("c-4");
}

/// The compaction with copies of ids of their own: nothing leads back to the user's
/// message, and only the model request can tie the answer to its end.
fn compacted_anew(staged: &Staged, answered: &Value) {
    staged.node(Some("c-1"), None, &summary("s-1"));
    staged.node(Some("c-2"), Some("c-1"), &with_id(step(), "a-copy"));
    staged.node(Some("c-3"), Some("c-2"), &with_id(tool(), "t-copy"));
    staged.node(Some("c-4"), Some("c-3"), answered);
    staged.head("c-4");
}

/// The end of the turn as Devin 3000.11 says it: the record that names its message
/// and, where `turn`, its model request, then the cause, with `stats` its request.
fn ended(cause: &str, turn: Option<&str>, stats: Option<&str>) -> Vec<Value> {
    let mut named = json!({ "sessionId": SESSION, "turnClientMessageId": "request-1" });
    if let Some(turn) = turn {
        named["turnRequestId"] = json!(turn);
    }
    let mut end = json!({ "sessionId": SESSION, "cause": cause, "stats": {} });
    if let Some(stats) = stats {
        end["stats"]["requestId"] = json!(stats);
    }
    vec![named, end]
}

/// The wire of the turn: the answer streamed, then `then`.
fn wired(staged: &Staged, then: &[Value]) {
    let mut events = vec![chunk(Some(json!("Done.")), "stream-1")];
    events.extend_from_slice(then);
    staged.wire("launch-1", &events);
}

/// What a reading says of the turn: its settlement, whether it is in flight, and
/// whether its last item is complete.
fn said(reading: &str) -> (String, bool, bool) {
    let reading: Value = serde_json::from_str(reading).unwrap_or_else(|_| panic!("{reading}"));
    let last = reading["items"]
        .as_array()
        .and_then(|items| items.last())
        .map_or(Value::Null, |item| item[3].clone());
    (
        reading["settlement"]
            .as_str()
            .unwrap_or_default()
            .to_owned(),
        reading["inFlight"] == true,
        last == true,
    )
}

/// What a look that read the turn before the compaction reads once `after` wrote the
/// rest, and what a fresh look reads: the same.
fn reading_after(after: impl Fn(&Staged)) -> String {
    let staged = Staged::new();
    before(&staged);
    let mut reader = staged.reader();
    assert_eq!(
        said(&show(&look(&mut reader))),
        ("in-flight".to_owned(), true, true),
        "the turn at work before the compaction"
    );
    after(&staged);
    let incremental = show(&look(&mut reader));
    let fresh = read_once(&staged);
    assert_eq!(
        incremental, fresh,
        "a look that read the turn before the compaction"
    );
    fresh
}

/// [`reading_after`] as [`said`] says it.
fn read_after(after: impl Fn(&Staged)) -> (String, bool, bool) {
    said(&reading_after(after))
}

/// The ids of a reading's items, in order.
fn ids(reading: &str) -> Vec<String> {
    let reading: Value = serde_json::from_str(reading).unwrap_or_else(|_| panic!("{reading}"));
    reading["items"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .map(|item| item[0].as_str().unwrap_or_default().to_owned())
                .collect()
        })
        .unwrap_or_default()
}

fn settled() -> (String, bool, bool) {
    ("settled".to_owned(), false, true)
}

fn unknown() -> (String, bool, bool) {
    ("unknown".to_owned(), true, false)
}

#[test]
fn an_answer_no_copy_leads_back_from_to_its_users_message_is_complete_by_the_model_request_the_wire_ended(
) {
    assert_eq!(
        read_after(|staged| {
            compacted_anew(staged, &answer("a-final", "Done.", ANSWERED));
            wired(staged, &ended("complete", Some(ANSWERED), Some(ANSWERED)));
        }),
        settled()
    );
}

#[test]
fn the_end_of_a_compacted_turn_that_names_no_model_request_or_two_proves_nothing() {
    for (turn, stats) in [
        (None, Some(ANSWERED)),
        (Some(ANSWERED), None),
        (None, None),
        (Some(ANSWERED), Some("model-9")),
        (Some("model-9"), Some(ANSWERED)),
        (Some(""), Some("")),
    ] {
        assert_eq!(
            read_after(|staged| {
                compacted_anew(staged, &answer("a-final", "Done.", ANSWERED));
                wired(staged, &ended("complete", turn, stats));
            }),
            unknown(),
            "the turn's request {turn:?}, the end's {stats:?}"
        );
    }
}

#[test]
fn a_compacted_answer_of_another_model_request_is_not_completed_by_the_same_text() {
    // The wire ended `model-9` with the same words: the stored answer is of `model-2`.
    assert_eq!(
        read_after(|staged| {
            compacted_anew(staged, &answer("a-final", "Done.", ANSWERED));
            wired(staged, &ended("complete", Some("model-9"), Some("model-9")));
        }),
        unknown()
    );
    // Nor is an answer with no request of its own.
    assert_eq!(
        read_after(|staged| {
            compacted_anew(staged, &reply("a-final", json!("Done.")));
            wired(staged, &ended("complete", Some(ANSWERED), Some(ANSWERED)));
        }),
        unknown()
    );
    // Nor one whose words are not the ones the wire streamed.
    assert_eq!(
        read_after(|staged| {
            compacted_anew(staged, &answer("a-final", "Done, and more.", ANSWERED));
            wired(staged, &ended("complete", Some(ANSWERED), Some(ANSWERED)));
        }),
        unknown()
    );
}

#[test]
fn work_after_a_compacted_turn_ended_keeps_it_in_flight() {
    // The agent thinks again on the wire.
    let thought = json!({
        "sessionId": SESSION,
        "update": { "sessionUpdate": "agent_thought_chunk", "content": { "type": "text", "text": "…" } },
    });
    let mut then = ended("complete", Some(ANSWERED), Some(ANSWERED));
    then.push(thought);
    assert_eq!(
        read_after(|staged| {
            compacted_anew(staged, &answer("a-final", "Done.", ANSWERED));
            wired(staged, &then);
        }),
        ("in-flight".to_owned(), true, true)
    );
    // The store goes on below the answer, in a model request the wire never ended.
    assert_eq!(
        read_after(|staged| {
            compacted_anew(staged, &answer("a-final", "Done.", ANSWERED));
            staged.node(
                Some("c-5"),
                Some("c-4"),
                &answer("a-next", "More", "model-3"),
            );
            staged.head("c-5");
            wired(staged, &ended("complete", Some(ANSWERED), Some(ANSWERED)));
        }),
        unknown()
    );
}

#[test]
fn a_compacted_turn_an_escape_cancelled_is_over_by_the_model_request_the_wire_ended() {
    // What a stop's look reads: the turn is over, its answer not whole.
    assert_eq!(
        read_after(|staged| {
            compacted_anew(staged, &answer("a-final", "Do", ANSWERED));
            wired(staged, &ended("cancelled", Some(ANSWERED), Some(ANSWERED)));
        }),
        ("settled".to_owned(), false, false)
    );
}

#[test]
fn a_users_message_a_compaction_cut_out_is_read_back_before_the_first_copy_of_a_step_that_answers_it(
) {
    // The turn ends as Devin's wire said it before 3000.11 as well: by its message alone.
    for end in [
        ended("complete", Some(ANSWERED), Some(ANSWERED)),
        vec![
            json!({ "sessionId": SESSION, "turnClientMessageId": "request-1", "cause": "complete" }),
        ],
    ] {
        let reading = reading_after(|staged| {
            compacted(staged, &answer("a-final", "Done.", ANSWERED));
            wired(staged, &end);
        });
        assert_eq!(ids(&reading), ["s-1", "u-1", "a-step", "t-step", "a-final"]);
        assert_eq!(said(&reading), settled());
    }
    // Before the turn ends, the message is back as well: what a stop's look and a receipt
    // are found by.
    let reading = reading_after(|staged| compacted(staged, &answer("a-final", "Do", ANSWERED)));
    assert_eq!(ids(&reading), ["s-1", "u-1", "a-step", "t-step", "a-final"]);
    assert_eq!(said(&reading), ("in-flight".to_owned(), true, false));
}

#[test]
fn a_second_compaction_reads_back_the_users_message_of_the_steps_first_original() {
    let reading = reading_after(|staged| {
        compacted(staged, &answer("a-final", "Done.", ANSWERED));
        // The compactor runs again: it copies the copies.
        staged.node(Some("d-1"), None, &summary("s-2"));
        staged.node(Some("d-2"), Some("d-1"), &step());
        staged.node(Some("d-3"), Some("d-2"), &tool());
        staged.node(
            Some("d-4"),
            Some("d-3"),
            &answer("a-final", "Done.", ANSWERED),
        );
        staged.head("d-4");
        wired(staged, &ended("complete", Some(ANSWERED), Some(ANSWERED)));
    });
    assert_eq!(ids(&reading), ["s-2", "u-1", "a-step", "t-step", "a-final"]);
    assert_eq!(said(&reading), settled());
}

#[test]
fn nothing_is_read_back_where_no_original_has_a_users_message_above_it_or_the_chain_holds_it() {
    // The original of the copy has no user's message above it.
    let staged = Staged::new();
    staged.node(Some("n-1"), None, &summary("s-0"));
    staged.node(Some("n-2"), Some("n-1"), &step());
    staged.node(Some("c-1"), None, &summary("s-1"));
    staged.node(Some("c-2"), Some("c-1"), &step());
    staged.head("c-2");
    assert_eq!(ids(&read_once(&staged)), ["s-1", "a-step"]);
    // A user's message on the chain after the summary is the chain's own: the copies
    // before it have theirs back, and nothing after it.
    let reading = reading_after(|staged| {
        compacted(staged, &answer("a-final", "Done.", ANSWERED));
        staged.node(
            Some("c-5"),
            Some("c-4"),
            &user("u-2", Some(json!("request-2"))),
        );
        staged.node(Some("c-6"), Some("c-5"), &with_id(step(), "a-later"));
        staged.head("c-6");
    });
    assert_eq!(
        ids(&reading),
        ["s-1", "u-1", "a-step", "t-step", "a-final", "u-2", "a-later"]
    );
    // The user's message copied onto the chain as well is there once.
    let staged = Staged::new();
    before(&staged);
    staged.node(Some("c-1"), None, &summary("s-1"));
    staged.node(Some("c-2"), Some("c-1"), &step());
    staged.node(
        Some("c-3"),
        Some("c-2"),
        &user("u-1", Some(json!("request-1"))),
    );
    staged.head("c-3");
    assert_eq!(ids(&read_once(&staged)), ["s-1", "a-step", "u-1"]);
}

#[test]
fn a_step_written_between_two_compactions_reads_back_the_users_message_through_the_first_ones_copies(
) {
    // Calliope's P2-1 on T-655 (T-679): the four SWE-2 sessions of T-655's live runs were
    // each compacted three times, and after the last summary 5 of 7 kept copies were of
    // steps written after an earlier one, whose rows lead up through that one's copies.
    let staged = Staged::new();
    before(&staged);
    let mut reader = staged.reader();
    look(&mut reader);
    // The first compaction, and a step written below its copies.
    staged.node(Some("c-1"), None, &summary("s-1"));
    staged.node(Some("c-2"), Some("c-1"), &step());
    staged.node(Some("c-3"), Some("c-2"), &tool());
    staged.node(Some("c-4"), Some("c-3"), &with_id(step(), "b-step"));
    staged.node(Some("c-5"), Some("c-4"), &with_id(tool(), "b-tool"));
    staged.head("c-5");
    assert_eq!(
        ids(&show(&look(&mut reader))),
        ["s-1", "u-1", "a-step", "t-step", "b-step", "b-tool"]
    );
    // The second keeps only the later step.
    staged.node(Some("d-1"), None, &summary("s-2"));
    staged.node(Some("d-2"), Some("d-1"), &with_id(step(), "b-step"));
    staged.node(Some("d-3"), Some("d-2"), &with_id(tool(), "b-tool"));
    staged.node(
        Some("d-4"),
        Some("d-3"),
        &answer("a-final", "Done.", ANSWERED),
    );
    staged.head("d-4");
    wired(&staged, &ended("complete", Some(ANSWERED), Some(ANSWERED)));
    let incremental = show(&look(&mut reader));
    assert_eq!(
        incremental,
        read_once(&staged),
        "a fresh look reads the same"
    );
    assert_eq!(
        ids(&incremental),
        ["s-2", "u-1", "b-step", "b-tool", "a-final"]
    );
    assert_eq!(said(&incremental), settled());
}

#[test]
fn a_summary_below_a_system_prefix_is_read_as_one_at_the_chains_root() {
    // Real chains hold the summary after the system's own rows (index 23 of 190 in
    // T-602's store, 6 of 15 in the SWE-2 sessions): Calliope's P3-2 on T-655.
    let prefix = json!({ "message_id": "p-1", "role": "system", "content": "Rules." });
    let reading = reading_after(|staged| {
        staged.node(Some("c-0"), None, &prefix);
        staged.node(Some("c-1"), Some("c-0"), &summary("s-1"));
        staged.node(Some("c-2"), Some("c-1"), &step());
        staged.node(Some("c-3"), Some("c-2"), &tool());
        staged.node(
            Some("c-4"),
            Some("c-3"),
            &answer("a-final", "Done.", ANSWERED),
        );
        staged.head("c-4");
        wired(staged, &ended("complete", Some(ANSWERED), Some(ANSWERED)));
    });
    assert_eq!(
        ids(&reading),
        ["p-1", "s-1", "u-1", "a-step", "t-step", "a-final"]
    );
    assert_eq!(said(&reading), settled());
}

#[test]
fn a_compacted_turn_devin_stamps_is_at_work_by_its_wire_alone_until_its_end_and_settles_by_its_request(
) {
    // Where T-655 and T-660 meet (Calliope on T-679): a turn Devin 3000.11 stamps, compacted
    // with nothing that leads back to its user's message, is at work by the wire's word
    // alone, whatever the chain holds, and ends at the end the wire says, by the model
    // request it names.
    let stamp = json!({ "cognition.ai/timestamp": "2026-10-26T08:18:30.000000+00:00" });
    let status = |state: &str| {
        json!({ "sessionId": SESSION, "update": { "sessionUpdate": "session_info_update",
            "_meta": { "cognition.ai/timestamp": 1_792_976_967_738_i64,
                "cognition.ai/statusEnum": state } } })
    };
    let work = vec![
        status("working"),
        json!({ "sessionId": SESSION, "update": { "sessionUpdate": "tool_call", "_meta": stamp } }),
        json!({ "sessionId": SESSION, "update": { "sessionUpdate": "tool_call_update", "status": "in_progress" } }),
    ];
    let at_work = reading_after(|staged| {
        compacted_anew(staged, &with_id(tool(), "t-later"));
        wired(staged, &work);
    });
    assert_eq!(said(&at_work), ("in-flight".to_owned(), true, true));
    let ended = reading_after(|staged| {
        compacted_anew(staged, &answer("a-final", "Done.", ANSWERED));
        let mut then = work.clone();
        then.push(status("blocked"));
        then.extend(ended("complete", Some(ANSWERED), Some(ANSWERED)));
        wired(staged, &then);
    });
    assert_eq!(said(&ended), settled());
}
