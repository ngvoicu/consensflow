//! A Claude window opened again on a conversation whose window closed with a
//! message queued in Claude's memory: the process that held it is gone, though
//! its record still says it is queued (T-442, Claude Code 2.1.302). The window
//! is prepared before its program starts, when the transcript is as the old
//! process left it, and what the transcript holds then is the old process's
//! (`Options::relaunched_at`): its own turn ends as any, the queued message no
//! part of it.

use super::*;

const SESSION: &str = "7d2c4f1e-0b9a-4e3d-8c6f-5a1b2c3d4e5f";

/// What the chief's reopen gives the window, as the engine words it.
const REOPENED: &str = "[ConsensFlow m-2 · T-1 · task from @chief]\nRun it once more";

/// Claude's record of a message it queued at second `n`: a job's notification.
fn queued_line(session: &str, n: u32) -> String {
    let content =
        "<task-notification>\n<task-id>bjob00001</task-id>\n<status>failed</status>\n</task-notification>";
    record(
        session,
        n,
        &json!({ "type": "queue-operation", "operation": "enqueue", "content": content }),
    )
}

/// Whether a look at `window` reads it at rest.
fn at_rest(window: &Rc<dyn Window>) -> bool {
    finished(window.observe()).unwrap().settled
}

#[test]
fn a_window_opened_again_reads_its_own_turn_at_rest_though_the_closed_process_had_a_message_queued()
{
    let home = Home::new();
    // The worker's turn ended, a job's notification was queued, and the window
    // closed before Claude took it.
    let mut lines = vec![
        user_line(SESSION, 1, TASK),
        answer_line(SESSION, 2, "Done"),
        stop_line(SESSION, 3),
        queued_line(SESSION, 4),
    ];
    home.transcript(SESSION, &lines);
    let request = Request {
        message: Some(REOPENED.to_owned()),
        ..Request::resumed(SESSION)
    };
    let window = window(&home.adapter(), &request);
    home.status(SESSION, &json!({ "status": "busy" }), me());
    assert!(!at_rest(&window), "busy");

    // The new process takes the message it was given and answers it.
    lines.extend([
        user_line(SESSION, 5, REOPENED),
        answer_line(SESSION, 6, "Done again"),
        stop_line(SESSION, 7),
    ]);
    home.transcript(SESSION, &lines);
    assert!(!at_rest(&window), "busy still");
    home.status(SESSION, &json!({ "status": "idle" }), me());
    assert!(at_rest(&window), "its turn is over");

    // What the new process queues itself holds its window as any.
    lines.push(queued_line(SESSION, 8));
    home.transcript(SESSION, &lines);
    assert!(!at_rest(&window), "a message of its own is queued");
}

#[test]
fn a_window_opened_with_no_message_is_at_rest_on_a_record_that_ended_with_a_message_queued() {
    let home = Home::new();
    home.transcript(
        SESSION,
        &[
            user_line(SESSION, 1, TASK),
            answer_line(SESSION, 2, "Done"),
            stop_line(SESSION, 3),
            queued_line(SESSION, 4),
        ],
    );
    let window = window(&home.adapter(), &Request::resumed(SESSION));
    home.status(SESSION, &json!({ "status": "idle" }), me());
    assert!(at_rest(&window));
    // A message pasted and answered leaves it at rest again.
    home.transcript(
        SESSION,
        &[
            user_line(SESSION, 1, TASK),
            answer_line(SESSION, 2, "Done"),
            stop_line(SESSION, 3),
            queued_line(SESSION, 4),
            user_line(SESSION, 5, REOPENED),
            answer_line(SESSION, 6, "Done again"),
            stop_line(SESSION, 7),
        ],
    );
    assert!(at_rest(&window));
}

#[test]
fn a_window_opened_again_after_a_turn_that_left_agents_running_is_not_at_rest_before_its_first_prompt(
) {
    // The old process's last turn ended with a background agent running, which
    // Claude's `busy` says of a window at its prompt. The agent went with the
    // process: the new one's `busy` is its own turn, taking the message it was
    // given, which its record does not show yet.
    let home = Home::new();
    let duration = record(
        SESSION,
        4,
        &json!({
            "type": "system", "subtype": "turn_duration", "isSidechain": false,
            "durationMs": 42_000, "messageCount": 3, "pendingBackgroundAgentCount": 1,
        }),
    );
    let mut lines = vec![
        user_line(SESSION, 1, TASK),
        answer_line(SESSION, 2, "Started an agent; waiting for it."),
        stop_line(SESSION, 3),
        duration,
    ];
    home.transcript(SESSION, &lines);
    let request = Request {
        message: Some(REOPENED.to_owned()),
        ..Request::resumed(SESSION)
    };
    let window = window(&home.adapter(), &request);
    home.status(SESSION, &json!({ "status": "busy" }), me());
    let observed = finished(window.observe()).unwrap();
    assert!(!observed.settled, "busy with its own turn");
    assert!(!observed.agents_pending(), "no agent of its own runs");

    lines.extend([
        user_line(SESSION, 5, REOPENED),
        answer_line(SESSION, 6, "Done again"),
        stop_line(SESSION, 7),
    ]);
    home.transcript(SESSION, &lines);
    home.status(SESSION, &json!({ "status": "idle" }), me());
    assert!(at_rest(&window), "its turn is over");
}

#[test]
fn a_window_followed_to_another_conversation_tells_it_no_relaunch() {
    // Where the window's program began writing is a place in the transcript it
    // was opened on: a `/clear` in the window goes on in the same process.
    let home = Home::new();
    let long = TASK.repeat(40);
    home.transcript(
        SESSION,
        &[
            user_line(SESSION, 1, &long),
            answer_line(SESSION, 2, "Done"),
            stop_line(SESSION, 3),
        ],
    );
    let window = window(&home.adapter(), &Request::resumed(SESSION));
    let cleared = "9e8d7c6b-5a4f-4e3d-8c2b-1a0f9e8d7c6b";
    window.follow(cleared);
    home.transcript(
        cleared,
        &[
            user_line(cleared, 1, "Go on"),
            queued_line(cleared, 2),
            answer_line(cleared, 3, "Done"),
            stop_line(cleared, 4),
        ],
    );
    home.status(cleared, &json!({ "status": "idle" }), me());
    assert!(!at_rest(&window), "the message it queued is still to come");
}

#[test]
fn a_window_on_a_new_conversation_tells_no_relaunch() {
    // A window that starts its conversation afresh has no process before it: a
    // message its first process queued holds it.
    let home = Home::new();
    let request = Request::default();
    let prepared = prepare(&home.adapter(), &request).unwrap();
    let session = prepared.native_session.clone().unwrap();
    home.transcript(
        &session,
        &[
            user_line(&session, 1, TASK),
            queued_line(&session, 2),
            answer_line(&session, 3, "Done"),
            stop_line(&session, 4),
        ],
    );
    home.status(&session, &json!({ "status": "idle" }), me());
    assert!(!at_rest(&prepared.window));
}
