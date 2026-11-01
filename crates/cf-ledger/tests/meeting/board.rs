//! The board's read of a meeting (3.0.7 chief-meeting, CMT-16): when it began, and
//! what it holds, in the order it will go, each with its kind, task and sender, its
//! body cut as a gated message's is. A board with no meeting says nothing of one,
//! as it always read.

use cf_ledger::Board;

use crate::note_to_chief;
use crate::world::world;

#[test]
fn the_board_says_when_the_meeting_began_and_what_it_holds_in_the_order_it_will_go() {
    let mut w = world();
    let quiet: Board = w.ledger.board(w.project.id).unwrap();
    let read = serde_json::to_value(&quiet).unwrap();
    assert!(read.get("held").is_none(), "{read}");
    assert!(read["project"].get("meeting").is_none(), "{read}");
    let begun = w.ledger.set_meeting(w.project.id, true).unwrap().meeting;
    let given = w.give(&[]);
    w.start(&given);
    let long = "x".repeat(5_000);
    let note = note_to_chief(&mut w, Some("zeus"), Some(1), &long);
    let question = w.ask(given.task.number);
    let board = w.ledger.board(w.project.id).unwrap();
    assert_eq!(board.project.meeting, begun);
    let held: Vec<(i64, &str, Option<i64>, Option<&str>)> = board
        .held
        .iter()
        .map(|message| {
            (
                message.id,
                message.kind.as_str(),
                message.task_number,
                message.sender.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        held,
        vec![
            (question.id, "question", Some(1), Some("zeus")),
            (note.id, "note", Some(1), Some("zeus")),
        ]
    );
    assert!(
        board.held[1].body.chars().count() < 2_100,
        "cut as a gated body is"
    );
    let read = serde_json::to_value(&board).unwrap();
    assert_eq!(read["project"]["meeting"], serde_json::json!(begun));
    assert_eq!(read["held"].as_array().map(Vec::len), Some(2));
}
