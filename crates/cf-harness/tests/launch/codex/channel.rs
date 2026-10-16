//! A message to a Codex window, as Node's Codex adapter suite and its channel
//! suite held it: a claim of the pane and then the supervisor's broker, which
//! takes the exact text for the thread it names, and what is refused before
//! either is asked. The broker here is scripted; `tests/channels/codex.rs`
//! holds the same cases against one on real sockets.

use std::cell::RefCell;
use std::rc::Rc;

use cf_harness::contract::{Admission, HostError, PaneHost, Readiness, Work, NOT_ADMITTED};
use cf_harness::testing::{finished, Answer, AnsweringHost, Driver, ScriptedHost};

use super::*;

#[test]
fn queues_a_message_through_the_real_channel_a_claim_then_the_broker() {
    let home = Home::new();
    let (adapter, fakes) = adapter(&home);
    let window = prepare(&adapter, &Request::resuming(THREAD))
        .unwrap()
        .window;
    let host = Rc::new(AnsweringHost::new(|_| Ok(json!({ "ok": true }))));
    let accepted = r#"{"ok":true,"admitted":true}"#;
    // Codex's app-server refuses half a character as the pane host does: a
    // Rust text holds none, and the rest is shown as Node showed it.
    for text in ["hi", "half  of it, \u{1b}[31mred\u{1b}[0m and 50%\r60%"] {
        fakes.loopback.serve("POST /deliver", [replies(accepted)]);
        assert_eq!(
            finished(window.deliver(&*host, &pane(), text)),
            Ok(Admission::Admitted { queued: true })
        );
    }
    let claim = (
        "pane.claim".to_owned(),
        json!({ "pane": "s1-diana", "generation": 2 }),
    );
    assert_eq!(*host.asked.borrow(), [claim.clone(), claim]);
    let asked = fakes.loopback.take_asked();
    let posted: Vec<Value> = asked
        .iter()
        .map(|request| serde_json::from_slice(request.body.as_ref().unwrap()).unwrap())
        .collect();
    assert!(asked
        .iter()
        .all(|request| request.url.ends_with("/deliver")));
    assert_eq!(
        (posted[0]["sessionId"].as_str(), posted[0]["text"].as_str()),
        (Some(THREAD), Some("hi"))
    );
    assert_eq!(
        posted[1]["text"],
        "half  of it, \u{241b}[31mred\u{241b}[0m and 50%\u{240d}60%"
    );
}

#[test]
fn claims_the_pane_then_hands_the_exact_text_to_the_broker_for_the_thread_it_names() {
    let home = Home::new();
    let (adapter, fakes) = adapter(&home);
    let window = prepare(&adapter, &Request::resuming(THREAD))
        .unwrap()
        .window;
    let host = Rc::new(ScriptedClaims::default());
    let delivered = Rc::new(RefCell::new(Vec::new()));
    fakes
        .loopback
        .serve("POST /deliver", [replies(r#"{"ok":true,"admitted":true}"#)]);
    let mut driver = Driver::default();
    let (deliver, claims) = (Rc::clone(&window), Rc::clone(&host));
    let log = Rc::clone(&delivered);
    driver.begin(0, async move {
        let sent = deliver
            .deliver(&*claims, &pane(), "a message with spaces\nand newlines")
            .await;
        log.borrow_mut().push(sent);
    });
    driver.run();
    assert_eq!(
        *delivered.borrow(),
        [Ok(Admission::Admitted { queued: true })]
    );
    assert_eq!(
        *host.claims.borrow(),
        [json!({ "pane": "s1-diana", "generation": 2 })]
    );
    let asked = fakes.loopback.take_asked();
    let body: Value = serde_json::from_slice(asked[0].body.as_ref().unwrap()).unwrap();
    assert_eq!(
        (
            body["launchId"].as_str(),
            body["sessionId"].as_str(),
            body["text"].as_str()
        ),
        (
            Some(LAUNCH),
            Some(THREAD),
            Some("a message with spaces\nand newlines")
        )
    );
}

/// Sol's review of 2026-10-16, round 4: the project closed while the delivery
/// waited for the pane's claim, and the text went to the broker once the claim
/// was answered. Once it is, the host is asked whether it still admits the send.
#[test]
fn a_send_the_host_stops_admitting_while_its_claim_waits_never_reaches_the_broker() {
    let home = Home::new();
    let (adapter, fakes) = adapter(&home);
    let window = prepare(&adapter, &Request::resuming(THREAD))
        .unwrap()
        .window;
    let host = Rc::new(ScriptedHost::default());
    host.answer("pane.claim", [Answer::Held]);
    let mut driver = Driver::default();
    let (deliver, claims) = (Rc::clone(&window), Rc::clone(&host));
    driver.begin(0, async move {
        deliver.deliver(&*claims, &pane(), "late").await
    });
    assert!(driver.run().is_empty(), "the claim waits for the host");
    host.refuse_hand_overs();
    assert!(host.release("pane.claim", Ok(json!({ "ok": true }))));
    assert_eq!(
        driver.run(),
        [(
            0,
            Ok(Admission::Refused {
                reason: NOT_ADMITTED.to_owned()
            })
        )]
    );
    assert!(
        fakes.loopback.take_asked().is_empty(),
        "the broker was asked nothing"
    );
}

/// Astra's review of 2026-10-16, round 5: the claim was answered and admitted,
/// and the project closed while the request to the broker waited for its
/// connection. The host is asked once the connection is up, before a byte goes.
#[test]
fn a_send_the_host_stops_admitting_while_its_connection_is_made_never_reaches_the_broker() {
    let home = Home::new();
    let (adapter, fakes) = adapter(&home);
    let window = prepare(&adapter, &Request::resuming(THREAD))
        .unwrap()
        .window;
    let host = Rc::new(ScriptedHost::default());
    host.answer("pane.claim", [Answer::Now(Ok(json!({ "ok": true })))]);
    fakes.loopback.hold_connection("POST /deliver");
    fakes
        .loopback
        .serve("POST /deliver", [replies(r#"{"ok":true,"admitted":true}"#)]);
    let mut driver = Driver::default();
    let (deliver, claims) = (Rc::clone(&window), Rc::clone(&host));
    driver.begin(0, async move {
        deliver.deliver(&*claims, &pane(), "late").await
    });
    assert!(driver.run().is_empty(), "the connection is being made");
    host.refuse_hand_overs();
    assert!(fakes.loopback.connect("POST /deliver"));
    assert_eq!(
        driver.run(),
        [(
            0,
            Ok(Admission::Refused {
                reason: NOT_ADMITTED.to_owned()
            })
        )]
    );
    assert!(
        fakes.loopback.take_asked().is_empty(),
        "the broker was asked nothing"
    );
}

/// A pane host that admits every claim and keeps what it was asked.
#[derive(Default)]
struct ScriptedClaims {
    claims: RefCell<Vec<Value>>,
}

impl PaneHost for ScriptedClaims {
    fn request<'a>(&'a self, _op: &'a str, body: Value) -> Work<'a, Result<Value, HostError>> {
        self.claims.borrow_mut().push(body);
        Box::pin(async { Ok(json!({ "ok": true })) })
    }
}

#[test]
fn rejects_a_thread_that_is_no_canonical_uuid_before_claiming() {
    let home = Home::new();
    let (adapter, fakes) = adapter(&home);
    let window = prepare(&adapter, &Request::resuming(THREAD))
        .unwrap()
        .window;
    let host = ScriptedClaims::default();
    for session in [
        "",
        "not-a-uuid",
        "aaaaaaaa-bbbb-4ccc-8ddd-40940940940",
        &format!("{THREAD}\n"),
    ] {
        window.follow(session);
        let refused = finished(window.deliver(&host, &pane(), "invalid session"));
        assert_eq!(
            refused,
            Err("codex-queue delivery needs a canonical native session UUID".to_owned()),
            "{session:?}"
        );
    }
    assert!(host.claims.borrow().is_empty(), "nothing was claimed");
    assert!(fakes.loopback.take_asked().is_empty(), "nobody was asked");
}

#[test]
fn rejects_a_malformed_pane_before_claiming() {
    let home = Home::new();
    let (adapter, fakes) = adapter(&home);
    let window = prepare(&adapter, &Request::resuming(THREAD))
        .unwrap()
        .window;
    let host = ScriptedClaims::default();
    for malformed in [
        Pane {
            id: String::new(),
            generation: 3,
        },
        Pane {
            id: "codex-pane".to_owned(),
            generation: 0,
        },
    ] {
        assert_eq!(
            finished(window.deliver(&host, &malformed, "invalid caller")),
            Err("codex-queue delivery needs pane {id, generation}".to_owned())
        );
    }
    assert!(host.claims.borrow().is_empty(), "nothing was claimed");
    assert!(fakes.loopback.take_asked().is_empty());
}

#[test]
fn turns_a_stale_native_claim_into_an_affirmative_zero_byte_refusal_without_asking_the_broker() {
    let home = Home::new();
    let (adapter, fakes) = adapter(&home);
    let window = prepare(&adapter, &Request::resuming(THREAD))
        .unwrap()
        .window;
    let host = AnsweringHost::new(|_| Ok(json!({ "ok": false, "error": "stale" })));
    assert_eq!(
        finished(window.deliver(&host, &pane(), "stale message")),
        Ok(Admission::Refused {
            reason: "stale".to_owned()
        })
    );
    assert!(
        fakes.loopback.take_asked().is_empty(),
        "the broker was not asked"
    );
}

#[test]
fn uses_the_owned_bridge_for_exact_identity_and_rejects_a_session_switch_after_the_pane_claim() {
    let home = Home::new();
    let (adapter, fakes) = adapter(&home);
    let window = prepare(&adapter, &Request::resuming(THREAD))
        .unwrap()
        .window;
    let host = ScriptedClaims::default();
    fakes
        .loopback
        .serve("GET /session", [shows(Some(THREAD), true)]);
    assert_eq!(finished(window.ready(&host, &pane())), Ok(Readiness::Ready));
    // The pane is claimed, and then the broker's window shows another thread.
    let changed =
        r#"{"ok":false,"admitted":false,"bytesWritten":0,"error":"native-session-changed"}"#;
    fakes.loopback.serve("POST /deliver", [replies(changed)]);
    assert_eq!(
        finished(window.deliver(&host, &pane(), "complete reply")),
        Ok(Admission::Refused {
            reason: "native-session-changed".to_owned()
        })
    );
    window.follow(NEXT);
    fakes
        .loopback
        .serve("POST /deliver", [replies(r#"{"ok":true,"admitted":true}"#)]);
    assert_eq!(
        finished(window.deliver(&host, &pane(), "complete reply")),
        Ok(Admission::Admitted { queued: true })
    );
    let asked = fakes.loopback.take_asked();
    let sent: Vec<Value> = asked
        .iter()
        .filter(|request| request.url.ends_with("/deliver"))
        .map(|request| serde_json::from_slice(request.body.as_ref().unwrap()).unwrap())
        .collect();
    assert_eq!(sent[0]["sessionId"], THREAD);
    assert_eq!(
        (sent[1]["sessionId"].as_str(), sent[1]["text"].as_str()),
        (Some(NEXT), Some("complete reply"))
    );
}
