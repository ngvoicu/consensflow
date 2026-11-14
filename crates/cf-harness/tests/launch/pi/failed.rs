//! A Pi window whose last turn failed, read from its own record by the adapter (T-781).
//!
//! The provider's error ends a Pi turn: Pi writes a step that failed, retries (at most a
//! minute apart) and writes another, and settles when it has no retry left, which its
//! extension (inside the window) says in a file beside the session. A window whose
//! extension left no word of it (a conversation followed with `/resume`, a launch folder
//! swept, a write that failed) read at work for as long as it was open: Pi's reader never
//! settled a failed turn by the record's quiet, as it does a turn that stopped or was
//! aborted, and the dispatcher publishes a look that is not settled as Working. The case is
//! the window of a launch, not one opened again on a conversation (that is `resumed`'s),
//! with the real reader over a record on disk.

use std::fs::File;
use std::time::{Duration, SystemTime};

use cf_harness::testing::EPOCH_MS;

use super::*;

const FAILURE: &str = "Connection error.";

/// The record of the conversation `session` on disk: the user's message and the step that
/// failed, written `quiet_for_ms` before the adapters' clock.
fn failed_turn(home: &Home, session: &str, quiet_for_ms: i64) {
    turn(
        home,
        session,
        quiet_for_ms,
        json!({
            "role": "assistant", "stopReason": "error", "errorMessage": FAILURE, "content": []
        }),
    );
}

/// The same session with a made assistant message.
fn turn(home: &Home, session: &str, quiet_for_ms: i64, assistant: Value) {
    let folder = Path::new(&home.root)
        .join("home")
        .join(".pi")
        .join("agent")
        .join("sessions")
        .join("--work--");
    fs::create_dir_all(&folder).unwrap();
    let file = folder.join(format!("2026-10-13T10-00-00-000Z_{session}.jsonl"));
    let at = "2026-10-13T10:00:01.000Z";
    let records = [
        json!({ "type": "session", "version": 3, "id": session, "timestamp": "2026-10-13T10:00:00.000Z" }),
        json!({ "type": "message", "id": "u1", "timestamp": at, "message": {
            "role": "user", "content": [{ "type": "text", "text": "Count to forty" }] } }),
        json!({ "type": "message", "id": "a1", "timestamp": at, "message": assistant }),
    ];
    let lines: Vec<String> = records.iter().map(Value::to_string).collect();
    fs::write(&file, format!("{}\n", lines.join("\n"))).unwrap();
    let written = EPOCH_MS - quiet_for_ms;
    File::options()
        .write(true)
        .open(&file)
        .unwrap()
        .set_modified(
            SystemTime::UNIX_EPOCH + Duration::from_millis(u64::try_from(written).unwrap()),
        )
        .unwrap();
}

#[test]
fn a_window_whose_reply_was_cut_waits_for_its_end_then_reports_the_limit() {
    let home = Home::new();
    let (adapter, _fakes) = adapter(&home);
    let plan = prepare(&adapter, &Request::default()).unwrap();
    let (window, session) = (plan.window, plan.native_session.unwrap());
    shows(&home, &session);
    let cut = || {
        json!({ "role": "assistant", "stopReason": "length", "content": [
        { "type": "thinking", "thinking": "Inspecting." },
        { "type": "text", "text": "The parser appears to" }
    ] })
    };
    turn(&home, &session, 10_000, cut());
    let working = finished(window.observe()).unwrap();
    assert!(!working.failed && !working.settled, "{working:?}");
    turn(&home, &session, 121_000, cut());
    let ended = finished(window.observe()).unwrap();
    assert!(ended.failed && ended.settled, "{ended:?}");
    let Some(Reading::Known(record)) = ended.reading.as_deref() else {
        panic!("the window did not read its Pi session: {ended:?}");
    };
    assert_eq!(record.failure_kind.as_deref(), Some("the member's reply was cut at the model's output limit; nothing was taken as its result"));
    assert!(!record.items.last().unwrap().complete);
}

#[test]
fn a_window_whose_turn_failed_is_at_work_while_pi_may_retry_and_at_rest_with_its_failure_once_the_record_is_quiet_whatever_the_extension_wrote(
) {
    let home = Home::new();
    let (adapter, _fakes) = adapter(&home);
    let plan = prepare(&adapter, &Request::default()).unwrap();
    let (window, session) = (plan.window, plan.native_session.unwrap());
    // The extension named the conversation, and no more: no word of the turn's end.
    shows(&home, &session);

    // Just failed: Pi may retry, so the window is at work.
    failed_turn(&home, &session, 10_000);
    let observed = finished(window.observe()).unwrap();
    assert!(observed.failed && !observed.settled, "{observed:?}");

    // Past two retries' worth of quiet nothing is coming: the turn is over, and failed.
    failed_turn(&home, &session, 121_000);
    let observed = finished(window.observe()).unwrap();
    assert!(
        observed.failed && observed.settled,
        "the window read at work for good: {observed:?}"
    );
    let Some(reading) = observed.reading.as_deref() else {
        panic!("a look reads a record");
    };
    let Reading::Known(record) = reading else {
        panic!("a record that was read: {reading:?}");
    };
    assert_eq!(
        record.failure.as_deref(),
        Some(FAILURE),
        "Pi's words reach the board"
    );
}
