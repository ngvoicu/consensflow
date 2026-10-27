//! Who else shows a window's conversation, by Claude Code's own statuses (`sessions/<pid>.json`,
//! each of a live process): every Claude that shows a conversation writes its transcript.
//!
//! - A window opened again on its conversation where the host did not prove the program before
//!   it gone (a window open when the app quit, ended after the daemon had gone) is told where
//!   its own program began all the same when no Claude shows the conversation: nothing writes
//!   it but the new program, and what the old one had queued went with it (T-442's three, the
//!   `SessionStart` sign; T-526's shape).
//! - One that some Claude still shows (the old program the host could not end, or one the human
//!   started outside ConsensFlow, `claude --resume <id>`) is told no such place, and a window
//!   that sees another Claude show its conversation, before it opened or beside it after, says
//!   so on every look from then on (`Observed::shared`): that one's turns are written into the
//!   same record as the window's, and nothing in them tells them apart.

use cf_harness::contract::{Observed, Records};
use cf_proto::agents::Harness;

use super::*;

const SESSION: &str = "4b6e2a91-7c3d-4f5e-9a8b-1c2d3e4f5a6b";

/// What the chief's reopen gives the window, as the engine words it.
const REOPENED: &str = "[ConsensFlow m-2 · T-1 · task from @chief]\nRun it once more";

/// Claude's record of a message it queued at second `n`: a job's notification.
fn queued_line(n: u32) -> String {
    let content =
        "<task-notification>\n<task-id>bjob00002</task-id>\n<status>failed</status>\n</task-notification>";
    record(
        SESSION,
        n,
        &json!({ "type": "queue-operation", "operation": "enqueue", "content": content }),
    )
}

/// The record a closed window left: the worker's turn ended, then a job's notification was
/// queued, which the window's process never took.
fn left_queued() -> Vec<String> {
    vec![
        user_line(SESSION, 1, TASK),
        answer_line(SESSION, 2, "Done"),
        stop_line(SESSION, 3),
        queued_line(4),
    ]
}

/// The window opened again on the conversation with the reopen's message, the host's proof
/// `proven`, and its own turn answered after: what a look at it then finds, its own status
/// (this test's process) idle.
fn reopened(home: &Home, proven: bool) -> (Rc<dyn Window>, Observed) {
    let mut lines = left_queued();
    home.transcript(SESSION, &lines);
    let request = Request {
        message: Some(REOPENED.to_owned()),
        previous_gone: proven,
        ..Request::resumed(SESSION)
    };
    let window = window(&home.adapter(), &request);
    lines.extend([
        user_line(SESSION, 5, REOPENED),
        answer_line(SESSION, 6, "Done again"),
        stop_line(SESSION, 7),
    ]);
    home.transcript(SESSION, &lines);
    home.status(SESSION, &json!({ "status": "idle" }), me());
    let observed = finished(window.observe()).unwrap();
    (window, observed)
}

#[test]
fn a_window_opened_again_without_the_hosts_proof_is_told_where_its_program_began_when_no_claude_shows_the_conversation(
) {
    let home = Home::new();
    // Claude keeps its statuses, of another conversation's Claude alone.
    home.status(
        "0f1e2d3c-4b5a-4968-8776-655443322110",
        &json!({ "status": "idle" }),
        me(),
    );
    let (_, observed) = reopened(&home, false);
    assert!(
        observed.settled,
        "the old program's queued message went with it"
    );
    assert!(!observed.shared);
}

#[test]
fn a_window_opened_again_where_a_claude_still_shows_the_conversation_is_told_no_relaunch() {
    // The old program the host could not end, or a Claude the human started on the
    // conversation outside ConsensFlow: either may write past any boundary taken.
    for proven in [false, true] {
        let home = Home::new();
        let other = OtherProcess::start();
        home.status(SESSION, &json!({ "status": "idle" }), other.pid());
        let (_, observed) = reopened(&home, proven);
        assert!(
            !observed.settled,
            "the queued message holds the turn ({proven})"
        );
        assert!(observed.shared, "another Claude shows it ({proven})");
    }
}

#[test]
fn a_window_whose_conversation_another_claude_shows_beside_it_says_so_from_then_on() {
    let home = Home::new();
    let (window, observed) = reopened(&home, true);
    assert!(observed.settled && !observed.shared, "the window alone");
    let other = OtherProcess::start();
    home.status(SESSION, &json!({ "status": "busy" }), other.pid());
    let observed = finished(window.observe()).unwrap();
    assert!(observed.shared, "the human's Claude beside it");
    drop(other);
    let observed = finished(window.observe()).unwrap();
    assert!(
        observed.shared,
        "what it wrote stays in the record after it quits"
    );
}

#[test]
fn a_claude_of_another_conversation_or_a_parked_job_on_this_one_shares_nothing() {
    let home = Home::new();
    let (window, _) = reopened(&home, true);
    let other = OtherProcess::start();
    home.status(
        "0f1e2d3c-4b5a-4968-8776-655443322110",
        &json!({ "status": "busy" }),
        other.pid(),
    );
    assert!(
        !finished(window.observe()).unwrap().shared,
        "another conversation"
    );
    home.status(
        SESSION,
        &json!({ "status": "busy", "parkedJobId": "job-1" }),
        other.pid(),
    );
    assert!(!finished(window.observe()).unwrap().shared, "a parked job");
}

/// What the Resume asks of the record with no window open: whether its turn was cut.
fn turn_cut(home: &Home) -> Option<bool> {
    let records = Fakes::new(&home.env()).records;
    finished(records.turn_cut(Harness::Claude, SESSION))
}

#[test]
fn a_record_no_claude_shows_was_cut_in_no_turn_for_a_message_its_program_queued_and_never_took() {
    // A Claude that quit idle, a notification queued: no sign of the quit is in its record.
    let home = Home::new();
    home.transcript(SESSION, &left_queued());
    assert_eq!(
        turn_cut(&home),
        Some(true),
        "Claude keeps no statuses here: none can say"
    );
    home.status(
        "0f1e2d3c-4b5a-4968-8776-655443322110",
        &json!({ "status": "idle" }),
        me(),
    );
    assert_eq!(turn_cut(&home), Some(false), "no Claude shows it");
    let other = OtherProcess::start();
    home.status(SESSION, &json!({ "status": "idle" }), other.pid());
    assert_eq!(
        turn_cut(&home),
        Some(true),
        "a Claude shows it, whose queue it may be"
    );
}

#[test]
fn a_turn_begun_and_never_ended_is_cut_though_no_claude_shows_the_record() {
    let home = Home::new();
    home.status(
        "0f1e2d3c-4b5a-4968-8776-655443322110",
        &json!({ "status": "idle" }),
        me(),
    );
    home.transcript(
        SESSION,
        &[
            user_line(SESSION, 1, TASK),
            answer_line(SESSION, 2, "Done"),
            stop_line(SESSION, 3),
            user_line(SESSION, 4, REOPENED),
        ],
    );
    assert_eq!(turn_cut(&home), Some(true));
}

#[test]
fn a_window_followed_to_another_conversation_shares_it_only_if_another_claude_shows_that_one() {
    let home = Home::new();
    let other = OtherProcess::start();
    home.status(SESSION, &json!({ "status": "idle" }), other.pid());
    let (window, observed) = reopened(&home, true);
    assert!(observed.shared);
    let cleared = "9e8d7c6b-5a4f-4e3d-8c2b-1a0f9e8d7c6b";
    window.follow(cleared);
    home.transcript(
        cleared,
        &[
            user_line(cleared, 1, "Go on"),
            answer_line(cleared, 2, "Done"),
            stop_line(cleared, 3),
        ],
    );
    home.status(cleared, &json!({ "status": "idle" }), me());
    assert!(
        !finished(window.observe()).unwrap().shared,
        "the window alone shows this one"
    );
}

/// The window's own Claude is found by the conversation it shows where the host did not name
/// it (on Windows the pane's program is not Claude's own process): a Claude that showed the
/// conversation before the window opened is not it, whatever order Claude's statuses are listed
/// in. Taken for the window's, its status stood for the window's on every look: the window read
/// at its prompt while its own Claude worked, and a window followed to another conversation read
/// as switched (zeewin2, 2 of 5: the relaunch plant missed, integration B round 2, T-800).
#[test]
fn a_claude_that_showed_the_conversation_before_the_window_opened_never_speaks_for_its_state() {
    let home = Home::new();
    let other = OtherProcess::start();
    home.status(SESSION, &json!({ "status": "idle" }), other.pid());
    home.transcript(SESSION, &left_queued());
    let request = Request {
        message: Some(REOPENED.to_owned()),
        ..Request::resumed(SESSION)
    };
    let window = window(&home.adapter(), &request);
    // The window's own Claude has said nothing yet: the other's idle is not the window's.
    assert!(!finished(window.observe()).unwrap().at_prompt);
    home.status(SESSION, &json!({ "status": "busy" }), me());
    assert!(
        !finished(window.observe()).unwrap().at_prompt,
        "the window's own Claude works"
    );
    home.status(SESSION, &json!({ "status": "idle" }), me());
    assert!(finished(window.observe()).unwrap().at_prompt);
}

#[test]
fn a_claude_that_showed_the_conversation_before_the_window_opened_is_never_taken_for_the_windows() {
    // The window's own Claude has written no status yet: the one that names the conversation
    // is the one that showed it before the window existed.
    let home = Home::new();
    let other = OtherProcess::start();
    home.status(SESSION, &json!({ "status": "idle" }), other.pid());
    home.transcript(SESSION, &left_queued());
    let request = Request {
        message: Some(REOPENED.to_owned()),
        ..Request::resumed(SESSION)
    };
    let window = window(&home.adapter(), &request);
    assert!(finished(window.observe()).unwrap().shared);
}
