//! What the goldens' scenarios never reach: a call left open in a turn that
//! ended, a call answered in a later turn, ids JavaScript keys oddly, a
//! completion that names no answer, and the records Node's reader threw on.

mod twins;

use super::*;
use serde_json::json;

/// What a rollout of `records` says, or why it says nothing.
fn read(records: &[Value]) -> Result<Record, String> {
    let mut rollout = Rollout::new(Arc::from("s"));
    for (index, record) in records.iter().enumerate() {
        rollout
            .visit(record.clone(), index)
            .map_err(|stop| stop.reason())?;
    }
    rollout.result()
}

fn event(payload: Value) -> Value {
    json!({ "type": "event_msg", "payload": payload })
}

fn response(payload: Value) -> Value {
    json!({ "type": "response_item", "payload": payload })
}

fn started(turn: Value) -> Value {
    event(json!({ "type": "task_started", "turn_id": turn }))
}

fn aborted(turn: Value) -> Value {
    event(json!({ "type": "turn_aborted", "turn_id": turn }))
}

fn call(id: &str) -> Value {
    response(json!({ "type": "function_call", "call_id": id }))
}

fn output(call: &str) -> Value {
    response(
        json!({ "type": "function_call_output", "call_id": call, "id": format!("out-{call}"), "output": "ok" }),
    )
}

#[test]
fn a_call_left_open_keeps_a_turn_that_ended_in_flight() {
    let mut records = vec![started(json!("t1")), call("c1"), aborted(json!("t1"))];
    let open = read(&records).unwrap();
    assert!(open.in_flight);
    assert_eq!(open.settlement, Settlement::InFlight);
    records.push(output("c1"));
    let answered = read(&records).unwrap();
    assert!(!answered.in_flight);
    assert_eq!(answered.settlement, Settlement::Settled);
}

#[test]
fn a_calls_output_in_a_later_turn_closes_it_in_the_turn_it_was_made_in() {
    let records = [
        started(json!("t1")),
        call("c1"),
        started(json!("t2")),
        output("c1"),
        // The first turn is the latest again, its call answered.
        aborted(json!("t1")),
    ];
    let read = read(&records).unwrap();
    assert!(!read.in_flight);
    assert_eq!(read.settlement, Settlement::Settled);
}

#[test]
fn turn_ids_are_keyed_as_javascript_keys_them() {
    // The number 36 and the text "36" are two turns: the call stays open in
    // the first while the second ends.
    let two = read(&[started(json!(36)), call("c1"), aborted(json!("36"))]).unwrap();
    assert!(!two.in_flight);
    let one = read(&[started(json!(36)), call("c1"), aborted(json!(36.0))]).unwrap();
    assert!(one.in_flight, "36 and 36.0 are one number");
    // Two objects are two turns, however alike.
    let objects = read(&[started(json!({})), call("c1"), aborted(json!({}))]).unwrap();
    assert!(!objects.in_flight);
}

#[test]
fn a_completion_that_names_no_answer_is_proven_by_a_complete_one_with_no_final_text() {
    // An item first said by the user and then by the assistant keeps its
    // role and its completeness, and is among the turn's answers.
    let said = |text: &str, role: &str| {
        response(json!({ "type": "message", "role": role, "id": "m1", "content": text }))
    };
    let mut records = vec![
        started(json!("t1")),
        said("hi", "user"),
        said("hi again", "assistant"),
    ];
    records.push(event(json!({ "type": "task_complete", "turn_id": "t1" })));
    let unnamed = read(&records).unwrap();
    assert_eq!(unnamed.settlement, Settlement::Settled);
    assert_eq!(unnamed.items[0].role, Role::User);
    assert_eq!(&*unnamed.items[0].text, "hi again");
    records.pop();
    records.push(event(
        json!({ "type": "task_complete", "turn_id": "t1", "last_agent_message": null }),
    ));
    assert_eq!(
        read(&records).unwrap().settlement,
        Settlement::Unknown,
        "null is not undefined"
    );
}

#[test]
fn a_null_record_and_a_turn_event_that_names_no_turn_fail_the_look() {
    assert_eq!(
        read(&[Value::Null]).unwrap_err(),
        "record 0 is null, where an object was read"
    );
    for named in [
        json!({ "type": "task_started" }),
        json!({ "type": "task_complete", "turn_id": "" }),
        json!({ "type": "turn_aborted", "turn_id": 0 }),
    ] {
        let kind = named["type"].as_str().unwrap().to_owned();
        assert_eq!(
            read(&[json!({ "ordinal": 7.0, "type": "event_msg", "payload": named })]).unwrap_err(),
            format!("a codex {kind} event names no turn at record 7")
        );
    }
    assert_eq!(read(&[]).unwrap_err(), "empty codex rollout for s");
}

#[test]
fn the_output_of_an_image_tool_is_a_line_naming_the_tool_the_call_made_never_the_picture() {
    // What the live designer's Codex wrote on 2026-10-10: `exec` drew the image,
    // and its output holds the picture, then where Codex saved it.
    let called = |kind: &str, id: &str| {
        response(json!({ "type": kind, "id": format!("c-{id}"), "call_id": id, "name": "exec" }))
    };
    let answered = |id: &str| {
        response(json!({
            "type": "custom_tool_call_output",
            "id": format!("o-{id}"),
            "call_id": id,
            "output": [
                { "type": "input_text", "text": "Script completed" },
                { "type": "input_image", "image_url": format!("data:image/png;base64,{}", "A".repeat(4_000)) },
                { "type": "input_text", "text": "Generated images are saved to /h/g as /h/g/a.png by default." },
            ]
        }))
    };
    let read = read(&[
        started(json!("t1")),
        called("custom_tool_call", "c1"),
        answered("c1"),
        // An output of a call the rollout never showed: no tool to name.
        answered("c2"),
    ])
    .unwrap();
    let texts: Vec<&str> = read.items.iter().map(|item| &*item.text).collect();
    assert_eq!(
        texts,
        [
            "Script completed\n[image from exec: image/png, 3.0 KB, saved to /h/g/a.png]\nGenerated images are saved to /h/g as /h/g/a.png by default.",
            "Script completed\n[image: image/png, 3.0 KB, saved to /h/g/a.png]\nGenerated images are saved to /h/g as /h/g/a.png by default.",
        ]
    );
    assert!(read
        .items
        .iter()
        .all(|item| item.role == Role::Tool && item.complete));
}

#[test]
fn a_command_output_javascript_cannot_make_text_fails_the_look() {
    let command = |native: Value| event(json!({ "type": "item_completed", "item": native }));
    for native in [
        json!({ "type": "CommandExecution", "id": "c", "aggregated_output": { "toString": null } }),
        json!({ "type": "CommandExecution", "id": "c", "formatted_output": [{ "toString": 1 }] }),
        json!({ "type": "CommandExecution", "id": "c", "stdout": "ok", "stderr": { "toString": "x" } }),
    ] {
        assert!(read(&[command(native.clone())]).is_err(), "{native}");
    }
    let text = read(&[command(json!({ "type": "CommandExecution", "id": "c", "aggregated_output": [1, { "valueOf": 2 }] }))]).unwrap();
    assert_eq!(&*text.items[0].text, "1,[object Object]");
}
