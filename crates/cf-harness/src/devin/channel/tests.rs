//! A message pasted into Devin's window, as `tests/adapter-devin.test.mjs`
//! holds Node's: only into the conversation it knows.

use std::fs;

use serde_json::{json, Value};

use super::*;
use crate::contract::{Admission, HostError};
use crate::shared::admission::admission;
use crate::testing::{finished, paste_answers, AnsweringHost};

/// The line Devin's log gains when its window configures a conversation it opens.
fn shows(session: &str) -> String {
    let record = json!({
        "sessionId": session,
        "update": { "sessionUpdate": "config_option_update", "configOptions": [{ "id": "mode" }] },
    });
    format!("{record}\n")
}

fn pane() -> Pane {
    Pane {
        id: "s1-zeus".to_owned(),
        generation: 2,
    }
}

/// A host that takes every paste.
fn taking() -> AnsweringHost<impl Fn(&str) -> Result<Value, HostError>> {
    AnsweringHost::new(|_| Ok(json!({ "ok": true })))
}

fn send_into(host: &dyn PaneHost, wire: &Path, session: Option<&str>, text: &str) -> Sent {
    finished(Box::pin(send(host, &pane(), wire, session, text)))
}

#[test]
fn a_message_is_pasted_into_the_conversation_the_log_says_the_window_shows() {
    let dir = tempfile::tempdir().unwrap();
    let wire = dir.path().join("wire.jsonl");
    fs::write(
        &wire,
        format!("{}{}", shows("another-one"), shows("mild-coin")),
    )
    .unwrap();
    let host = taking();
    let sent = send_into(&host, &wire, Some("mild-coin"), "hi");
    assert!(sent.ok);
    assert_eq!(
        host.asked.borrow().last().unwrap(),
        &(
            "pane.write_paste".to_owned(),
            json!({ "id": "s1-zeus", "generation": 2, "body": "hi" })
        )
    );
}

#[test]
fn a_message_is_refused_before_it_is_pasted_while_the_window_shows_another_conversation() {
    let dir = tempfile::tempdir().unwrap();
    let wire = dir.path().join("wire.jsonl");
    fs::write(&wire, shows("another-one")).unwrap();
    let host = taking();
    let sent = send_into(&host, &wire, Some("mild-coin"), "hi");
    assert_eq!(
        sent,
        Sent {
            ok: false,
            refused: true,
            cause: None,
            error: Some("Devin is displaying another conversation".to_owned()),
        }
    );
    assert!(host.asked.borrow().is_empty(), "nothing reached the pane");
}

#[test]
fn a_window_that_has_named_no_conversation_has_none_a_message_is_for() {
    let dir = tempfile::tempdir().unwrap();
    let wire = dir.path().join("wire.jsonl");
    fs::write(&wire, shows("mild-coin")).unwrap();
    let host = taking();
    for session in [None, Some("")] {
        let sent = send_into(&host, &wire, session, "hi");
        assert!(sent.refused, "{session:?}");
        assert_eq!(
            sent.error.as_deref(),
            Some("Devin is displaying another conversation")
        );
    }
    // And a log that says nothing is no conversation to match.
    fs::write(&wire, "").unwrap();
    let sent = send_into(&host, &wire, Some("mild-coin"), "hi");
    assert_eq!(
        sent.error.as_deref(),
        Some("Devin is displaying another conversation")
    );
    assert!(host.asked.borrow().is_empty());
}

#[test]
fn a_log_that_is_not_there_or_cannot_be_read_is_a_conversation_unavailable() {
    let dir = tempfile::tempdir().unwrap();
    let wire = dir.path().join("wire.jsonl");
    let host = taking();
    for log in [
        None,
        Some("null\n".to_owned()),
        Some("not json\n".to_owned()),
        Some(format!("\u{feff}{}", shows("mild-coin"))),
        Some(format!(
            "{}not json\n{}",
            shows("mild-coin"),
            shows("mild-coin")
        )),
    ] {
        match &log {
            Some(text) => fs::write(&wire, text).unwrap(),
            None => drop(fs::remove_file(&wire)),
        }
        let sent = send_into(&host, &wire, Some("mild-coin"), "hi");
        assert_eq!(
            (sent.refused, sent.error.as_deref()),
            (true, Some("Devin conversation is unavailable")),
            "{log:?}"
        );
    }
    assert!(host.asked.borrow().is_empty());
}

#[test]
fn devins_send_is_read_as_the_delivery_contract_has_it_in_every_row() {
    let dir = tempfile::tempdir().unwrap();
    let wire = dir.path().join("wire.jsonl");
    fs::write(&wire, shows("mild-coin")).unwrap();
    // The window shows the conversation: every way a pane host can answer the paste.
    for (way, answer, read) in paste_answers() {
        let host = AnsweringHost::new(move |_| answer.clone());
        let sent = send_into(&host, &wire, Some("mild-coin"), "hi");
        assert_eq!(admission(&sent, "refused", false), read, "{way}");
    }
    // The ways only Devin has, each refused before the host is asked anything.
    let host = taking();
    for (way, log, session, reason) in [
        (
            "Devin shows another conversation",
            Some(shows("another-one")),
            "mild-coin",
            "Devin is displaying another conversation",
        ),
        (
            "Devin has no wire log to say which conversation it shows",
            None,
            "mild-coin",
            "Devin conversation is unavailable",
        ),
    ] {
        match log {
            Some(text) => fs::write(&wire, text).unwrap(),
            None => fs::remove_file(&wire).unwrap(),
        }
        let sent = send_into(&host, &wire, Some(session), "hi");
        assert_eq!(
            admission(&sent, "refused", false),
            Admission::Refused {
                reason: reason.to_owned()
            },
            "{way}"
        );
    }
    assert!(host.asked.borrow().is_empty(), "nothing reached the pane");
}

#[test]
fn what_the_pane_host_says_of_the_paste_is_what_the_send_says() {
    let dir = tempfile::tempdir().unwrap();
    let wire = dir.path().join("wire.jsonl");
    fs::write(&wire, shows("mild-coin")).unwrap();
    let refusing = AnsweringHost::new(|_| {
        Ok(json!({ "ok": false, "admitted": false, "error": "stale", "cause": "stale pane" }))
    });
    let sent = send_into(&refusing, &wire, Some("mild-coin"), "hi");
    assert_eq!(
        sent,
        Sent {
            ok: false,
            refused: true,
            cause: Some("stale pane".to_owned()),
            error: Some("stale".to_owned()),
        }
    );
    let lost = AnsweringHost::new(|_| {
        Err(HostError {
            error: Some("eof".to_owned()),
            message: "bridge ended".to_owned(),
        })
    });
    let sent = send_into(&lost, &wire, Some("mild-coin"), "hi");
    assert_eq!(
        (
            sent.ok,
            sent.refused,
            sent.cause.as_deref(),
            sent.error.as_deref()
        ),
        (false, false, Some("bridge ended"), Some("eof"))
    );
}
