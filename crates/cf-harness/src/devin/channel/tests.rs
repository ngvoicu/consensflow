//! A message pasted into Devin's window, as Node's Devin adapter suite held it:
//! only into the conversation it knows.

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
    finished(Box::pin(send(
        host,
        &pane(),
        wire,
        session,
        Guard::Configured,
        text,
        None,
    )))
}

/// A line of a window's replay of `session`, which configures nothing.
fn names(session: &str) -> String {
    let record = json!({ "sessionId": session,
        "update": { "sessionUpdate": "session_info_update", "title": "A sleep" } });
    format!("{record}\n")
}

#[test]
fn a_log_that_configured_none_takes_a_paste_only_where_the_adapter_says_the_window_is_unannounced_on_its_conversation(
) {
    // T-1432: a Devin 3000.12.43 window opened again silent names its conversation on every line
    // and configures none. Only a paste the adapter put behind /bypass goes into it.
    let dir = tempfile::tempdir().unwrap();
    let wire = dir.path().join("wire.jsonl");
    let send_as = |host: &dyn PaneHost, guard| {
        finished(Box::pin(send(
            host,
            &pane(),
            &wire,
            Some("mild-coin"),
            guard,
            "/bypass hi",
            None,
        )))
    };
    fs::write(
        &wire,
        format!("{{}}\n{}{}", names("mild-coin"), names("mild-coin")),
    )
    .unwrap();
    let host = taking();
    assert!(send_as(&host, Guard::Configured).refused);
    assert!(send_as(&host, Guard::Unannounced).ok);
    // Another conversation named, two, or a configuration of another: refused all the same.
    for log in [
        names("fresh-leaf"),
        format!("{}{}", names("mild-coin"), names("fresh-leaf")),
        format!("{}{}", names("mild-coin"), shows("fresh-leaf")),
    ] {
        fs::write(&wire, log).unwrap();
        let host = taking();
        assert!(send_as(&host, Guard::Unannounced).refused);
        assert!(host.asked.borrow().is_empty());
    }
    // A configuration of its own takes it, as any paste.
    fs::write(
        &wire,
        format!("{}{}", names("mild-coin"), shows("mild-coin")),
    )
    .unwrap();
    assert!(send_as(&taking(), Guard::Unannounced).ok);
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
        Some(format!("{}null\n", shows("mild-coin"))),
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
fn a_torn_line_of_the_log_refuses_no_paste_into_the_conversation_after_it() {
    // A short write left half a record of the log, and the next append made one
    // line of it and itself, before the window configured the conversation it shows.
    let dir = tempfile::tempdir().unwrap();
    let wire = dir.path().join("wire.jsonl");
    let half = &shows("another-one")[..30];
    fs::write(
        &wire,
        format!(
            "{}{half}{}{}",
            shows("another-one"),
            shows("another-one"),
            shows("mild-coin")
        ),
    )
    .unwrap();
    let host = taking();
    let sent = send_into(&host, &wire, Some("mild-coin"), "hi");
    assert!(sent.ok, "{sent:?}");
    // A line that is not JSON says nothing either way: what the log said before it holds.
    for (log, error) in [
        (
            "not json\n".to_owned(),
            "Devin is displaying another conversation",
        ),
        (
            format!("\u{feff}{}", shows("mild-coin")),
            "Devin is displaying another conversation",
        ),
    ] {
        fs::write(&wire, &log).unwrap();
        let sent = send_into(&host, &wire, Some("mild-coin"), "hi");
        assert_eq!(sent.error.as_deref(), Some(error), "{log:?}");
    }
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

#[test]
fn a_paste_into_a_window_that_configured_none_goes_in_only_while_the_human_has_not_typed_into_it() {
    // A window that says nothing could be on another conversation only by a person's keys
    // (T-1474): the paste the adapter puts behind /bypass asks the host to go in only while
    // none has reached it, and the host checks that as it pastes. A paste the log allows by its
    // configuration asks nothing more.
    let dir = tempfile::tempdir().unwrap();
    let wire = dir.path().join("wire.jsonl");
    let untyped = |host: &AnsweringHost<_>| {
        host.asked
            .borrow()
            .iter()
            .filter(|(op, _)| op == "pane.write_paste")
            .map(|(_, body)| body.get(cf_proto::panes::UNTYPED).cloned())
            .collect::<Vec<_>>()
    };
    let send_as = |host: &dyn PaneHost, guard| {
        finished(Box::pin(send(
            host,
            &pane(),
            &wire,
            Some("mild-coin"),
            guard,
            "/bypass hi",
            None,
        )))
    };
    fs::write(&wire, names("mild-coin")).unwrap();
    let host = taking();
    assert!(send_as(&host, Guard::Unannounced).ok);
    assert_eq!(untyped(&host), [Some(json!(true))]);
    // The host's word that a key of the human's came first: refused, nothing written.
    let typed_into = AnsweringHost::new(|_| {
        Ok(json!({ "ok": false, "admitted": false, "bytesWritten": 0,
            "error": cf_proto::panes::TYPED_INTO,
            "cause": "the human has typed into the window since it opened" }))
    });
    let sent = send_as(&typed_into, Guard::Unannounced);
    assert!(sent.refused, "{sent:?}");
    assert_eq!(sent.error.as_deref(), Some(cf_proto::panes::TYPED_INTO));
    // Configured, by its own: a paste as any, whichever the guard.
    fs::write(
        &wire,
        format!("{}{}", names("mild-coin"), shows("mild-coin")),
    )
    .unwrap();
    for guard in [Guard::Unannounced, Guard::Configured] {
        let host = taking();
        assert!(send_as(&host, guard).ok);
        assert_eq!(untyped(&host), [None], "{guard:?}");
    }
}
