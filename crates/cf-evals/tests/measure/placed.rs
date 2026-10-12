//! The tasks the board places.

use crate::support::{measured, recording};

#[test]
fn the_tasks_the_board_places_are_every_one_it_lists_one_no_lane_has_whatever_its_state_and_not_one_the_human_deleted(
) {
    // Waiting (T-1), paused (T-2), called off (T-3), failed (T-4): no member had them.
    // Zeus's, on his session's lane (T-5); diana's, who then left the staff (T-6);
    // and one the human deleted (T-7). `listed` is how many the ledger's own board
    // listed when the ledger was recorded: the lanes' tasks and the open ones.
    let listed = recording("placed")["listed"].as_i64().unwrap();
    assert_eq!(listed, 6, "every task but the deleted one is on the board");
    assert_eq!(measured("placed").plumbing.placed, listed);
}
