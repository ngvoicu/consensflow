//! What the broker's reply, or the lack of one, makes of a message: that it
//! was taken, refused with nothing written, or is uncertain; and how the
//! answer reads to an adapter.

use super::*;
use crate::seams::loopback::BodyFailed;

#[test]
fn a_broker_that_gives_no_head_or_breaks_off_its_body_is_a_failure_of_the_transport() {
    let mut stage = Stage::new();
    for served in [
        Served::NoHead,
        Served::Head {
            status: 200,
            body: Body::Cut,
        },
    ] {
        stage.claim_with(json!({ "ok": true }));
        stage.fakes.loopback.serve("POST /deliver", [served]);
        assert_eq!(stage.sends("x"), Ok(uncertain("native-queue-transport")));
    }
    // A body the connection broke while the test held it.
    stage.claim_with(json!({ "ok": true }));
    let held = Served::Head {
        status: 200,
        body: Body::Held,
    };
    stage.fakes.loopback.serve("POST /deliver", [held]);
    let pane = stage.pane.clone();
    stage.begin(1, Some(THREAD), pane, "x");
    assert!(stage.driver.run().is_empty());
    assert!(stage
        .fakes
        .loopback
        .release_body("POST /deliver", Err(BodyFailed::Cut)));
    assert_eq!(
        stage.driver.run(),
        [(1, Ok(uncertain("native-queue-transport")))]
    );
}

#[test]
fn a_url_that_is_none_is_a_failure_of_the_transport_after_the_claim() {
    let mut stage = Stage::new();
    stage.channel = Channel::new(LAUNCH, "not a url".to_owned(), TOKEN.to_owned());
    stage.claim_with(json!({ "ok": true }));
    assert_eq!(stage.sends("x"), Ok(uncertain("native-queue-transport")));
    assert_eq!(stage.host.take_asked().len(), 1, "the pane was claimed");
}

#[test]
fn a_reply_past_the_size_or_that_is_null_or_no_json_is_a_failure_of_the_transport() {
    let ok = r#"{"ok":true,"admitted":true}"#;
    let padded = |size: usize| format!("{ok}{}", " ".repeat(size - ok.len()));
    let mut stage = Stage::new();
    for (body, answer) in [
        (padded(BODY_LIMIT), admitted()),
        (padded(BODY_LIMIT + 1), uncertain("native-queue-transport")),
        ("null".to_owned(), uncertain("native-queue-transport")),
        (String::new(), uncertain("native-queue-transport")),
        ("{\"ok\":".to_owned(), uncertain("native-queue-transport")),
        (format!("\u{feff}{ok}"), admitted()),
        (
            format!("\u{feff}\u{feff}{ok}"),
            uncertain("native-queue-transport"),
        ),
    ] {
        stage.claim_with(json!({ "ok": true }));
        stage.replies(200, &body);
        assert_eq!(stage.sends("x"), Ok(answer));
    }
}

#[test]
fn a_reply_is_a_yes_only_in_a_success_status_with_both_flags_true() {
    for (status, body, answer) in [
        (200, r#"{"ok":true,"admitted":true}"#, admitted()),
        (201, r#"{"ok":true,"admitted":true,"extra":1}"#, admitted()),
        (299, r#"{"ok":true,"admitted":true}"#, admitted()),
        (
            199,
            r#"{"ok":true,"admitted":true}"#,
            uncertain("native-queue-admission"),
        ),
        (
            300,
            r#"{"ok":true,"admitted":true}"#,
            uncertain("native-queue-admission"),
        ),
        (
            500,
            r#"{"ok":true,"admitted":true}"#,
            uncertain("native-queue-admission"),
        ),
        (
            200,
            r#"{"ok":true,"admitted":false}"#,
            uncertain("native-queue-admission"),
        ),
        (
            200,
            r#"{"ok":false,"admitted":true}"#,
            uncertain("native-queue-admission"),
        ),
        (
            200,
            r#"{"ok":true,"admitted":"yes"}"#,
            uncertain("native-queue-admission"),
        ),
        (
            200,
            r#"{"ok":"true","admitted":true}"#,
            uncertain("native-queue-admission"),
        ),
        (200, r#"{"ok":true}"#, uncertain("native-queue-admission")),
        (200, "{}", uncertain("native-queue-admission")),
        (200, "[]", uncertain("native-queue-admission")),
        (200, "7", uncertain("native-queue-admission")),
        (200, r#""admitted""#, uncertain("native-queue-admission")),
        (200, "true", uncertain("native-queue-admission")),
    ] {
        let mut stage = Stage::new();
        stage.claim_with(json!({ "ok": true }));
        stage.replies(status, body);
        assert_eq!(stage.sends("x"), Ok(answer), "{status} {body}");
    }
}

#[test]
fn a_refusal_with_nothing_written_is_the_brokers_whatever_the_status() {
    let changed =
        r#"{"ok":false,"admitted":false,"bytesWritten":0,"error":"native-session-changed"}"#;
    for (status, body, error) in [
        (200, changed, "native-session-changed"),
        (409, changed, "native-session-changed"),
        (500, changed, "native-session-changed"),
        (
            200,
            r#"{"admitted":false,"bytesWritten":0.0,"error":"x"}"#,
            "x",
        ),
        (
            200,
            r#"{"admitted":false,"bytesWritten":-0,"error":"x"}"#,
            "x",
        ),
        (200, r#"{"admitted":false,"bytesWritten":0,"error":""}"#, ""),
    ] {
        let mut stage = Stage::new();
        stage.claim_with(json!({ "ok": true }));
        stage.replies(status, body);
        assert_eq!(
            stage.sends("x"),
            Ok(refused(error, None)),
            "{status} {body}"
        );
    }
    for body in [
        r#"{"admitted":false,"bytesWritten":3,"error":"x"}"#,
        r#"{"admitted":false,"bytesWritten":"0","error":"x"}"#,
        r#"{"admitted":false,"error":"x"}"#,
        r#"{"admitted":false,"bytesWritten":0,"error":7}"#,
        r#"{"admitted":false,"bytesWritten":0}"#,
        r#"{"admitted":null,"bytesWritten":0,"error":"x"}"#,
        r#"{"admitted":"false","bytesWritten":0,"error":"x"}"#,
    ] {
        let mut stage = Stage::new();
        stage.claim_with(json!({ "ok": true }));
        stage.replies(200, body);
        assert_eq!(
            stage.sends("x"),
            Ok(uncertain("native-queue-admission")),
            "{body}"
        );
    }
}

#[test]
fn an_answer_is_read_by_an_adapter_as_it_is_in_what_became_of_the_message() {
    let reading = |answer: Answer| {
        let sent = answer.reading();
        (sent.ok, sent.refused, sent.cause, sent.error)
    };
    assert_eq!(reading(admitted()), (true, false, None, None));
    assert_eq!(
        reading(refused("expired", None)),
        (false, true, None, Some("expired".to_owned()))
    );
    assert_eq!(
        reading(refused("failed-with-zero-bytes", Some("stale"))),
        (
            false,
            true,
            Some("stale".to_owned()),
            Some("failed-with-zero-bytes".to_owned())
        )
    );
    assert_eq!(
        reading(uncertain("native-queue-transport")),
        (
            false,
            false,
            Some("native-queue-transport".to_owned()),
            Some("uncertain".to_owned())
        )
    );
}
