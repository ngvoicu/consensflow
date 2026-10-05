//! Records Claude flushes after the answer they started, and the uuids that
//! decide whether a user's turn is one of them.
//!
//! The answer `A` is written first, then the boundary `D` that ends its turn,
//! then the user's turn `U` and the attachments between it and the answer:
//! `A`'s parent is the attachment `T2`, whose parent is `T1`, whose parent is
//! `U`. Whether `U` begins a new turn, or is a late ancestor of `A` and
//! leaves the turn settled, is decided from every record read.

use super::*;

fn prompt() -> Value {
    user("U", Value::Null, json!("Hello"))
}

fn first() -> Value {
    attachment("T1", "U")
}

fn second() -> Value {
    attachment("T2", "T1")
}

/// The records in the order Claude wrote them.
fn late() -> Vec<Value> {
    vec![
        answer("A", "T2"),
        duration("D", "A"),
        prompt(),
        first(),
        second(),
    ]
}

/// A record of another type, which claims `uuid` too.
fn progress(uuid: Value) -> Value {
    of_session(json!({ "type": "progress", "uuid": uuid }))
}

/// A reading of the answer and the user's turn, at record `at`; settled or not.
fn reading(at: usize, settles: bool) -> String {
    let items = format!(r#"[["m1","assistant","Hi",true,0],["U","user","Hello",true,{at}]]"#);
    if settles {
        settled(&items)
    } else {
        in_flight(&items)
    }
}

/// A reading of the answer alone, its turn settled.
fn answered() -> String {
    settled(r#"[["m1","assistant","Hi",true,0]]"#)
}

#[test]
fn a_user_turn_and_its_attachments_written_after_the_answer_leave_it_settled() {
    assert_eq!(once(&late()), reading(2, true));
}

#[test]
fn a_look_that_reads_under_a_uuid_a_decision_looked_up_reads_the_transcript_again() {
    let records = late();
    // The user is read with none of its ancestors, and begins a turn. The
    // look that brings the attachments reads `T1` first, which nothing looked
    // up, and then `T2`, which the decision did: the records of the look
    // wait for the transcript to be read again, whole.
    let [answer, boundary, prompt, first, second] = &records[..] else {
        unreachable!("five records");
    };
    let read = looks(&[
        &[answer.clone(), boundary.clone(), prompt.clone()],
        &[first.clone(), second.clone()],
    ]);
    assert_eq!(read, [reading(2, false), reading(2, true)]);
    assert_eq!(read[1], once(&records), "as a look at the whole transcript");
    // And with the user in a look of its own, between the answer and the attachments.
    let read = looks(&[
        &[answer.clone(), boundary.clone()],
        std::slice::from_ref(prompt),
        &[first.clone(), second.clone()],
    ]);
    assert_eq!(read, [answered(), reading(2, false), reading(2, true)]);
}

#[test]
fn a_user_read_after_its_attachments_is_a_late_ancestor_in_the_look_that_reads_it() {
    let records = late();
    let [answer, boundary, prompt, first, second] = &records[..] else {
        unreachable!("five records");
    };
    let read = looks(&[
        &[
            answer.clone(),
            boundary.clone(),
            first.clone(),
            second.clone(),
        ],
        std::slice::from_ref(prompt),
    ]);
    assert_eq!(read, [answered(), reading(4, true)]);
}

#[test]
fn a_record_read_under_a_uuid_nothing_looked_up_changes_no_decision() {
    let records = late();
    let kept = reading(2, true);
    let read = looks(&[&records, &[attachment("T3", "T2")], &[]]);
    assert_eq!(read, [kept.clone(), kept.clone(), kept]);
}

#[test]
fn a_uuid_two_records_claim_belongs_to_neither_and_a_third_claim_does_not_give_it_back() {
    let claimed = |extra: &[Value]| {
        let mut records = late();
        records.extend(extra.iter().cloned());
        once(&records)
    };
    // The attachment between the user and the answer is claimed again.
    assert_eq!(claimed(&[attachment("T1", "U")]), reading(2, false));
    assert_eq!(
        claimed(&[attachment("T1", "U"), attachment("T1", "U")]),
        reading(2, false)
    );
    assert_eq!(
        once(&[
            answer("A", "T2"),
            duration("D", "A"),
            first(),
            attachment("T1", "U"),
            attachment("T1", "U"),
            second(),
            prompt()
        ]),
        reading(6, false)
    );
    // The boundary, the answer, and the user.
    assert_eq!(
        once(&[
            answer("A", "T2"),
            duration("D", "A"),
            duration("D", "A"),
            prompt(),
            first(),
            second()
        ]),
        reading(3, false)
    );
    assert_eq!(
        once(&[
            answer("A", "T2"),
            duration("D", "A"),
            progress(json!("A")),
            prompt(),
            first(),
            second()
        ]),
        reading(3, false)
    );
    assert_eq!(
        once(&[
            answer("A", "T2"),
            duration("D", "A"),
            prompt(),
            prompt(),
            first(),
            second()
        ]),
        in_flight(
            r#"[["m1","assistant","Hi",true,0],["U","user","Hello",true,2],["U","user","Hello",true,3]]"#
        )
    );
    assert_eq!(
        once(&[
            answer("A", "T2"),
            duration("D", "A"),
            prompt(),
            prompt(),
            prompt(),
            first(),
            second()
        ]),
        in_flight(
            r#"[["m1","assistant","Hi",true,0],["U","user","Hello",true,2],["U","user","Hello",true,3],["U","user","Hello",true,4]]"#
        )
    );
    // The user claims the answer's uuid.
    assert_eq!(
        once(&[
            answer("A", "U"),
            duration("D", "A"),
            user("A", Value::Null, json!("Hello")),
            first(),
            second()
        ]),
        in_flight(r#"[["m1","assistant","Hi",true,0],["A","user","Hello",true,2]]"#)
    );
}

#[test]
fn a_boundary_that_is_not_the_main_conversation_s_own_does_not_make_the_user_late() {
    let with = |boundary: Value| once(&[answer("A", "T2"), boundary, prompt(), first(), second()]);
    // Node's answer: the boundary ends the turn, but the user is no ancestor
    // of an answer that a record of another session or a sidechain ended.
    assert_eq!(with(duration("D", "A")), reading(2, true));
    assert_eq!(with(summary("D", "A")), reading(2, true));
    let others = [
        having(duration("D", "A"), json!({ "sessionId": "other" })),
        lacking(duration("D", "A"), "sessionId"),
        having(summary("D", "A"), json!({ "sessionId": "other" })),
        having(summary("D", "A"), json!({ "isSidechain": true })),
        lacking(summary("D", "A"), "isSidechain"),
    ];
    for boundary in others {
        assert_eq!(with(boundary.clone()), reading(2, false), "{boundary}");
    }
}

#[test]
fn a_record_with_no_uuid_text_claims_nothing() {
    assert_eq!(
        once(&[
            answer("A", "T2"),
            duration("D", "A"),
            lacking(progress(json!(null)), "uuid"),
            progress(json!("")),
            progress(json!(7)),
            prompt(),
            first(),
            second()
        ]),
        reading(5, true)
    );
}

#[test]
fn a_boundary_or_a_chain_that_does_not_lead_to_the_user_leaves_it_a_new_turn() {
    let boundary = || duration("D", "A");
    let rest = || vec![prompt(), first(), second()];
    let with = |boundary: Value, rest: Vec<Value>| {
        let mut records = vec![answer("A", "T2"), boundary];
        records.extend(rest);
        once(&records)
    };
    // The boundary says no uuid, an empty one, or no parent to the answer.
    assert_eq!(with(lacking(boundary(), "uuid"), rest()), reading(2, false));
    assert_eq!(
        with(having(boundary(), json!({ "uuid": "" })), rest()),
        reading(2, false)
    );
    assert_eq!(
        with(lacking(boundary(), "parentUuid"), rest()),
        reading(2, false)
    );
    // A parent that is no text, a loop, and a record that is no attachment.
    assert_eq!(
        with(
            boundary(),
            vec![
                prompt(),
                having(first(), json!({ "parentUuid": 5 })),
                second()
            ]
        ),
        reading(2, false)
    );
    assert_eq!(
        with(
            boundary(),
            vec![
                prompt(),
                having(first(), json!({ "parentUuid": "T2" })),
                second()
            ]
        ),
        reading(2, false)
    );
    assert_eq!(
        with(
            boundary(),
            vec![
                prompt(),
                having(first(), json!({ "type": "progress" })),
                second()
            ]
        ),
        reading(2, false)
    );
}
