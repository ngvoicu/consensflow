//! Codex's channel to its broker, against a broker of the test's own on the
//! machine's clock and loopback (Node's `describe("Codex's channel to its
//! broker")`): the pane is claimed first, and the exact text goes to the broker
//! for the thread it names, which admits it for that thread alone. The cases are
//! held once more on a clock and a loopback a test moves by hand, in
//! `tests/launch/codex/channel.rs` and the channel's unit tests.

use std::cell::RefCell;
use std::rc::Rc;

use cf_harness::codex::{send, Answer, Channel, Session, Shown, Target};
use cf_harness::contract::{Pane, PaneHost};
use cf_harness::seams::{SystemLoopback, SystemTime};
use cf_harness::testing::server::{Reply, Request, Server};
use cf_harness::testing::AnsweringHost;
use serde_json::{json, Value};

use crate::support::{matches, pane, run};

const SESSION: &str = "01a0817b-e6b0-7f32-8e11-370dc000cbc0";

/// A stand-in for the supervisor's broker (`cf codex-session`), the only way
/// into a Codex window: it shows one thread and admits a message for that
/// thread alone.
struct Broker {
    server: Server,
    selected: Rc<RefCell<String>>,
}

impl Broker {
    async fn start() -> Self {
        let selected = Rc::new(RefCell::new(SESSION.to_owned()));
        let shown = Rc::clone(&selected);
        let server = Server::start(move |request: &Request| {
            if request.target == "/session" {
                return Reply::json(
                    200,
                    &json!({ "launchId": "owned", "sessionId": *shown.borrow(), "available": true }),
                );
            }
            if request.json()["sessionId"] == *shown.borrow() {
                Reply::json(200, &json!({ "ok": true, "admitted": true }))
            } else {
                Reply::json(
                    200,
                    &json!({
                        "ok": false,
                        "admitted": false,
                        "bytesWritten": 0,
                        "error": "native-session-changed",
                    }),
                )
            }
        })
        .await;
        Self { server, selected }
    }

    /// The launch's channel to it.
    fn channel(&self) -> Channel {
        Channel::new(
            "owned",
            self.server.endpoint(),
            "private-owned-bridge-token-123".to_owned(),
        )
    }

    /// The window shows `thread` from now on.
    fn select(&self, thread: &str) {
        *self.selected.borrow_mut() = thread.to_owned();
    }

    /// What was posted to be delivered, in order.
    fn received(&self) -> Vec<Value> {
        self.server
            .calls()
            .iter()
            .filter(|call| call.target != "/session")
            .map(Request::json)
            .collect()
    }
}

/// A pane host that grants every claim.
fn granting() -> AnsweringHost<impl Fn(&str) -> Result<Value, cf_harness::contract::HostError>> {
    AnsweringHost::new(|_| Ok(json!({ "ok": true })))
}

/// `text` sent through the broker's channel for `thread`, from `pane`.
async fn sent(
    broker: &Broker,
    thread: Option<&str>,
    pane: &Pane,
    host: &dyn PaneHost,
    text: &str,
) -> Result<Answer, String> {
    let channel = broker.channel();
    let target = Target {
        channel: &channel,
        thread,
        pane,
        host,
    };
    send(&SystemTime, &SystemLoopback, &target, text).await
}

/// The answer to a message the broker took.
fn admitted() -> Answer {
    Answer {
        ok: true,
        admitted: Some(true),
        error: None,
        zero_bytes: false,
        cause: None,
    }
}

#[test]
fn claims_the_pane_then_hands_the_exact_text_to_the_broker_for_the_thread_it_names() {
    run(async {
        let broker = Broker::start().await;
        let host = granting();
        let answer = sent(
            &broker,
            Some(SESSION),
            &pane("codex-pane", 3),
            &host,
            "a message with spaces\nand newlines",
        )
        .await
        .unwrap();
        assert_eq!(answer, admitted());
        assert_eq!(
            *host.asked.borrow(),
            [(
                "pane.claim".to_owned(),
                json!({ "pane": "codex-pane", "generation": 3 })
            )]
        );
        let received = broker.received();
        assert_eq!(
            [
                &received[0]["launchId"],
                &received[0]["sessionId"],
                &received[0]["text"]
            ],
            [
                &json!("owned"),
                &json!(SESSION),
                &json!("a message with spaces\nand newlines")
            ]
        );
    });
}

#[test]
fn rejects_malformed_native_session_uuids_before_claiming() {
    run(async {
        let broker = Broker::start().await;
        for session in [
            "",
            "not-a-uuid",
            "aaaaaaaa-bbbb-4ccc-8ddd-40940940940",
            &format!("{SESSION}\n"),
        ] {
            let host = granting();
            let failed = sent(
                &broker,
                Some(session),
                &pane("codex-pane", 3),
                &host,
                "invalid session",
            )
            .await
            .unwrap_err();
            assert!(matches(&failed, "UUID"), "{failed}");
            assert_eq!(host.asked.borrow().len(), 0, "claims, for {session:?}");
        }
        assert_eq!(broker.received(), Vec::<Value>::new());
    });
}

/// A send from `pane` is refused before it claims anything.
async fn rejected_before_claiming(pane: Pane) {
    let broker = Broker::start().await;
    let host = granting();
    let failed = sent(&broker, Some(SESSION), &pane, &host, "invalid caller")
        .await
        .unwrap_err();
    assert!(matches(&failed, r"pane \{id, generation\}"), "{failed}");
    assert_eq!(host.asked.borrow().len(), 0, "claims");
    assert_eq!(broker.received(), Vec::<Value>::new());
}

#[test]
fn rejects_a_pane_without_an_id_before_claiming() {
    run(rejected_before_claiming(pane("", 3)));
}

#[test]
fn rejects_a_pane_of_generation_zero_before_claiming() {
    run(rejected_before_claiming(pane("codex-pane", 0)));
}

#[test]
fn turns_a_stale_native_claim_into_an_affirmative_zero_byte_refusal_without_asking_the_broker() {
    run(async {
        let broker = Broker::start().await;
        let host = AnsweringHost::new(|_| Ok(json!({ "ok": false, "error": "stale" })));
        let answer = sent(
            &broker,
            Some(SESSION),
            &pane("codex-pane", 3),
            &host,
            "stale message",
        )
        .await
        .unwrap();
        assert_eq!(
            answer,
            Answer {
                ok: false,
                admitted: Some(false),
                error: Some("failed-with-zero-bytes".to_owned()),
                zero_bytes: true,
                cause: Some("stale".to_owned()),
            }
        );
        assert_eq!(broker.received(), Vec::<Value>::new());
    });
}

#[test]
fn uses_the_owned_bridge_for_exact_identity_and_rejects_a_session_switch_after_the_pane_claim() {
    run(async {
        let broker = Broker::start().await;
        assert_eq!(
            broker.channel().shown(&SystemTime, &SystemLoopback).await,
            Some(Shown {
                session: Session::Thread(SESSION.to_owned()),
                available: true,
            })
        );
        let switched = "01a09094-a559-7db0-bf50-e2309856c3c0";
        let host = AnsweringHost::new(|_| {
            broker.select(switched);
            Ok(json!({ "ok": true }))
        });
        let pane = pane("codex-pane", 3);
        let answer = sent(&broker, Some(SESSION), &pane, &host, "complete reply")
            .await
            .unwrap();
        assert_eq!(
            answer,
            Answer {
                ok: false,
                admitted: Some(false),
                error: Some("native-session-changed".to_owned()),
                zero_bytes: true,
                cause: None,
            }
        );
        assert_eq!(broker.received()[0]["sessionId"], SESSION);
        let answer = sent(&broker, Some(switched), &pane, &host, "complete reply")
            .await
            .unwrap();
        assert_eq!(answer, admitted());
        assert_eq!(broker.received()[1]["text"], "complete reply");
    });
}
