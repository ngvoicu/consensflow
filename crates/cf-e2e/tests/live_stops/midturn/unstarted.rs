//! What the case says when it never got to pause: the member did not get to write its
//! numbers in time, or its turn was over before the pause. A run on a machine nobody
//! sits at says nothing of why unless the case does, so each look the case has at a
//! member that did not get going is said at once:
//!
//! - the task and the messages of its thread (was the brief delivered), what the daemon
//!   wrote and told the chief and the human of it;
//! - the lane over the wait;
//! - each window the daemon opened: how it was opened, how it ended or what its pane host
//!   says of it now, and what it printed, as the text of a screen and as it came;
//! - the harness's own files, where the case reads them (Devin's wire log and store);
//! - the daemon's log.
//!
//! The daemon says why itself when a window never shows its first message (it fails the
//! task, and quotes the screen it has), which a case that waits for its lane to work does
//! not read where it looks at the lane alone: a window the daemon gave up on is none of
//! its lane's, and its screen is gone with it.

#[path = "unstarted/devin.rs"]
mod devin;
#[path = "unstarted/lane.rs"]
mod lane;
#[path = "unstarted/notes.rs"]
mod notes;
#[path = "unstarted/window.rs"]
mod window;

use cf_e2e::live::members::{Account, Members};
use serde_json::Value;

use super::begin::Doing;
use crate::watch::say_events;

pub use lane::Waiting;

/// How many of the last messages of an inbox, and of the last lines of the daemon's
/// log, are said.
const INBOX: usize = 6;
const LOG: usize = 25;

/// The failure, as the line of the case that did not hold: where the task is over,
/// what the chief was told of it, and where it is not, how far the member got: no window,
/// a window the brief never reached (`delivered` is whether it did), or a window that has
/// it and a lane that did not read working with what the case looks for (`doing`).
pub fn failure(
    within: u64,
    over: Option<(&str, Option<&str>)>,
    waiting: &Waiting,
    delivered: bool,
    doing: &Doing,
) -> String {
    match over {
        Some((state, told)) => format!(
            "the member did not {}: its task is {state}, and {}",
            doing.to,
            told.map_or_else(
                || "the chief was told nothing of it".to_owned(),
                |told| format!("the chief was told: {told}")
            )
        ),
        None => format!(
            "the member never {} in {within} s: {}",
            doing.did,
            if !waiting.had_a_window() {
                "the daemon opened no window for it".to_owned()
            } else if !delivered {
                "its window opened, and its brief never reached it (its task's first message is not delivered)".to_owned()
            } else {
                format!(
                    "its brief reached its window, and its lane did not read working with {} (its lane is below)",
                    doing.shown
                )
            }
        ),
    }
}

/// Says each line of `lines` as a line of the case, five blanks in, with the blanks
/// a line has of its own after them.
fn put(case: &mut Account, lines: Vec<String>) {
    for line in lines {
        case.say(format!("     {line}"));
    }
}

/// What the case saw of a member that did not get going, said (see the module). `number`
/// is its task, `waiting` its lane over the wait, and `devin` whether the member runs on
/// Devin, whose own files are read.
pub fn say_why(
    case: &mut Account,
    members: &Members,
    agent: &str,
    number: i64,
    waiting: &Waiting,
    devin: bool,
) {
    put(
        case,
        match members.task(number) {
            Ok(thread) => notes::task_lines(&thread, number),
            Err(failed) => vec![format!("the task could not be read: {failed}")],
        },
    );
    say_events(case, members, number);
    for (who, inbox) in [
        ("the chief", members.inbox(Some("chief"))),
        ("the human", members.inbox(None)),
    ] {
        put(
            case,
            match inbox {
                Ok(inbox) => notes::inbox_lines(who, &inbox, INBOX),
                Err(failed) => vec![format!("what {who} was told could not be read: {failed}")],
            },
        );
    }
    put(
        case,
        vec![format!("its lane over the wait: {}", waiting.line())],
    );
    put(case, window::window_lines(members, agent));
    if devin {
        put(case, devin::devin_lines(members));
    }
    put(case, notes::log_lines(&members.rig().log(), LOG));
}

/// What the chief was told of the task `number`, which is over: the daemon's words
/// of it, none where it said nothing or the inbox could not be read.
pub fn told_of(members: &Members, number: i64) -> Option<String> {
    let chief: Vec<Value> = members.inbox(Some("chief")).unwrap_or_default();
    notes::told_why(number, &chief)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::{numbers, tool};
    use super::*;

    #[test]
    fn a_task_that_is_over_is_said_with_what_the_chief_was_told_of_it_and_with_nothing_where_it_was_none(
    ) {
        let waiting = Waiting::new(0);
        let told = Some("T-1 failed: the window never showed its first message");
        assert_eq!(
            failure(300, Some(("failed", told)), &waiting, false, &numbers::DOING),
            "the member did not write its numbers: its task is failed, and the chief was told: T-1 failed: the window never showed its first message"
        );
        assert_eq!(
            failure(300, Some(("cancelled", None)), &waiting, true, &tool::DOING),
            "the member did not run its command: its task is cancelled, and the chief was told nothing of it"
        );
    }

    #[test]
    fn a_member_that_never_got_going_is_said_by_how_far_it_got() {
        let mut waiting = Waiting::new(0);
        waiting.note(&json!({ "activity": { "state": "closed" } }), 0);
        assert_eq!(
            failure(300, None, &waiting, false, &numbers::DOING),
            "the member never wrote its numbers in 300 s: the daemon opened no window for it"
        );
        waiting.note(
            &json!({ "pane": { "id": "p1-devin-a-b" }, "activity": { "state": "working" } }),
            1_000,
        );
        // A window its brief never reached: its lane may read working all the while.
        assert_eq!(
            failure(300, None, &waiting, false, &numbers::DOING),
            "the member never wrote its numbers in 300 s: its window opened, and its brief never reached it (its task's first message is not delivered)"
        );
        assert_eq!(
            failure(300, None, &waiting, true, &numbers::DOING),
            "the member never wrote its numbers in 300 s: its brief reached its window, and its lane did not read working with numbers on its screen (its lane is below)"
        );
        assert_eq!(
            failure(300, None, &waiting, true, &tool::DOING),
            "the member never ran its command in 300 s: its brief reached its window, and its lane did not read working with its command running (its lane is below)"
        );
    }
}
