//! Which of the store's rows a look reads again: its last eight, and all of
//! them after a look that failed.

use super::*;

#[test]
fn the_last_eight_rows_are_read_again_and_no_more() {
    let staged = Staged::new();
    let note = |at: usize, text: &str| json!({ "message_id": format!("m-{at}"), "role": "custom", "content": text });
    let notes: Vec<Value> = (1..=10)
        .map(|at| note(at, &format!("Note {at}.")))
        .collect();
    staged.chain(&notes);
    let mut reader = staged.reader();
    let texts = |reading: &Reading| match reading {
        Reading::Known(record) => record
            .items
            .iter()
            .map(|item| item.text.to_string())
            .collect::<Vec<_>>()
            .join(" "),
        Reading::Unknown(reason) => reason.clone(),
    };
    let first = look(&mut reader);
    // The ninth row from the last is past them: its rewrite is not seen.
    staged.rewrite("n-2", &note(2, "Rewritten 2."));
    assert!(Arc::ptr_eq(&first, &look(&mut reader)));
    staged.rewrite("n-3", &note(3, "Rewritten 3."));
    assert_eq!(
        texts(&look(&mut reader)),
        "Note 1. Note 2. Rewritten 3. Note 4. Note 5. Note 6. Note 7. Note 8. Note 9. Note 10."
    );
}

#[test]
fn a_failed_look_has_the_next_read_the_store_whole() {
    let staged = Staged::new();
    let note = |at: usize, text: &str| json!({ "message_id": format!("m-{at}"), "role": "custom", "content": text });
    let notes: Vec<Value> = (1..=10)
        .map(|at| note(at, &format!("Note {at}.")))
        .collect();
    staged.chain(&notes);
    let mut reader = staged.reader();
    let first_text = |reading: &Reading| match reading {
        Reading::Known(record) => record.items[0].text.to_string(),
        Reading::Unknown(reason) => reason.clone(),
    };
    look(&mut reader);
    // Past the rows each look reads again: not seen.
    staged.rewrite("n-1", &note(1, "Rewritten 1."));
    assert_eq!(first_text(&look(&mut reader)), "Note 1.");
    staged.wire(
        "launch-1",
        &[
            chunk(Some(json!("A")), "stream-1"),
            complete(),
            chunk(Some(json!("B")), "stream-2"),
            complete(),
        ],
    );
    assert_eq!(
        first_text(&look(&mut reader)),
        "unreadable: conflicting Devin completion evidence"
    );
    staged.wire("launch-1", &[]);
    assert_eq!(first_text(&look(&mut reader)), "Rewritten 1.");
}
