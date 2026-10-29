//! What a turn's record says of its final answer (the owner's decision of 2026-10-15: a
//! worker's result is what it wrote after its last tool call, and not every line of progress
//! before it). Claude Code writes each message of a turn in records of its own, and the
//! message that ended the turn is the one its boundary record completes: the messages
//! before it stopped for a tool, and are never complete, whatever they said. A turn that
//! was woken by a job's end, and a turn that said nothing after its last tool, are the
//! other shapes the engine reads the final answer out of (`turn_end`).

use super::*;
use crate::shared::record::reading::Role;

/// What the items of a reading are, in order: who from, and whether complete.
fn shape(record: &Record) -> Vec<(Role, bool)> {
    record
        .items
        .iter()
        .map(|item| (item.role, item.complete))
        .collect()
}

/// A message of the turn `m`, in the two records Claude Code writes of it: what it said,
/// and the call it stopped for.
fn step(n: &str, parent: &str, said: &str, call: &str) -> [Value; 2] {
    let (said_in, called_in) = (format!("a{n}"), format!("a{n}b"));
    [
        assistant(
            &said_in,
            parent,
            json!(format!("m{n}")),
            json!([text(json!(said))]),
            Value::Null,
        ),
        assistant(
            &called_in,
            &said_in,
            json!(format!("m{n}")),
            json!([tool_use(json!(call))]),
            json!("tool_use"),
        ),
    ]
}

/// The result of the call `call`.
fn result_of(uuid: &str, parent: &str, call: &str) -> Value {
    user(
        uuid,
        json!(parent),
        json!([tool_result(json!(call), json!("ok"))]),
    )
}

#[test]
fn the_messages_of_a_turn_before_the_one_that_ended_it_are_never_complete_and_its_last_is() {
    let [first_said, first_called] = step("1", "u1", "Still reviewing the diff", "t1");
    let [second_said, second_called] = step("2", "u2", "Now the tests", "t2");
    let report = assistant(
        "a3",
        "u3",
        json!("m3"),
        json!([text(json!("PASS: no findings."))]),
        json!("end_turn"),
    );
    let records = [
        hello(),
        first_said,
        first_called,
        result_of("u2", "a1b", "t1"),
        second_said,
        second_called,
        result_of("u3", "a2b", "t2"),
        report,
        summary("s1", "a3"),
        duration("d1", "s1"),
    ];
    let mut stage = Stage::new();
    stage.write(&records);
    let Reading::Known(record) = &*stage.reader.look(&Options::default(), 0) else {
        panic!("a reading");
    };

    assert_eq!(record.settlement, Settlement::Settled);
    assert_eq!(
        shape(record),
        [
            (Role::User, true),
            (Role::Assistant, false),
            (Role::Tool, true),
            (Role::Assistant, false),
            (Role::Tool, true),
            (Role::Assistant, true),
        ],
        "the progress is no complete answer, and the report is"
    );
    let words: Vec<&str> = record.items.iter().map(|item| &*item.text).collect();
    assert_eq!(words[1], "Still reviewing the diff");
    assert_eq!(words[3], "Now the tests");
    assert_eq!(words[5], "PASS: no findings.");
    assert!(record.items.iter().all(|item| !item.commentary));
}

#[test]
fn the_turn_a_jobs_end_wakes_has_its_own_final_answer_after_the_one_of_the_turn_before() {
    // T-14's shape: the first turn ended with "still going" (complete, its answer), the
    // notification is the user's record of the turn Claude Code begins by itself, and that
    // turn's report is the last complete message, with a user's record between.
    let notice = "<task-notification>\n<task-id>bayehs8y4</task-id>\n<status>completed</status>\n</task-notification>";
    let [said, called] = step("1", "u1", "Running the mutation tests", "t1");
    let going = assistant(
        "a2",
        "u2",
        json!("m2"),
        json!([text(json!("Still going; I'll write it up when it ends."))]),
        json!("end_turn"),
    );
    let woken = having(
        user("w1", json!("d1"), json!(notice)),
        json!({ "promptSource": "system", "turnOrigin": "task_notification" }),
    );
    let report = assistant(
        "a3",
        "w1",
        json!("m3"),
        json!([text(json!("All 33 mutants are killed."))]),
        json!("end_turn"),
    );
    let mut stage = Stage::new();
    stage.write(&[
        hello(),
        said,
        called,
        result_of("u2", "a1b", "t1"),
        going,
        summary("s1", "a2"),
        duration("d1", "s1"),
        woken,
        report,
        summary("s2", "a3"),
        duration("d2", "s2"),
    ]);
    let Reading::Known(record) = &*stage.reader.look(&Options::default(), 0) else {
        panic!("a reading");
    };

    assert_eq!(
        shape(record),
        [
            (Role::User, true),
            (Role::Assistant, false),
            (Role::Tool, true),
            (Role::Assistant, true),
            (Role::User, true),
            (Role::Assistant, true),
        ]
    );
    assert_eq!(record.items[4].text.as_ref(), notice);
    assert_eq!(record.items[5].text.as_ref(), "All 33 mutants are killed.");
    // The notification is the harness's own words, no human's (T-842's P3: one that
    // quoted a header took a message for arrived); the user's record before it is not.
    assert_eq!(
        (record.items[0].synthetic, record.items[4].synthetic),
        (false, true)
    );
    assert!(
        !record.items[4].commentary,
        "still a user's item, as Node read it"
    );
}
