//! A message sent to a Codex window's broker, on a clock that moves when the
//! test moves it: what is asked of the pane host and of the broker and in
//! what order (here), what is refused before either is asked and what the
//! deadline leaves (`refusals`), and what each reply of the broker makes of
//! the message (`verdicts`). `tests/codex-channel.test.mjs` holds the same
//! channel against a broker of its own on the machine's clock.

mod refusals;
mod verdicts;

use std::rc::Rc;

use cf_base::env::Env;
use serde_json::json;

use super::*;
use crate::seams::loopback::Method;
use crate::testing::{
    Answer as Asked, Driver, Fakes, ScriptedHost, Sent as Body, Served, EPOCH_MS,
};

const LAUNCH: &str = "0a1b2c3d-4e5f-4061-8a7b-9c0d1e2f3a4b";
const THREAD: &str = "0f8fad5b-d9cb-469f-a165-70867728950e";
const TOKEN: &str = "AwoRGB8mLTQ7QklQV15lbHN6gYiPlp2k";

/// What a send settled with, by the work that began it.
type Settled = Vec<(usize, Result<Answer, String>)>;

/// A send to a broker the test scripts, from a pane the host scripts.
struct Stage {
    fakes: Fakes,
    host: Rc<ScriptedHost>,
    channel: Channel,
    pane: Pane,
    driver: Driver<Result<Answer, String>>,
}

impl Stage {
    fn new() -> Self {
        Self {
            fakes: Fakes::new(&Env::default()),
            host: Rc::new(ScriptedHost::default()),
            channel: Channel::new(
                LAUNCH,
                "http://127.0.0.1:41000".to_owned(),
                TOKEN.to_owned(),
            ),
            pane: Pane {
                id: "p1-diana".to_owned(),
                generation: 3,
            },
            driver: Driver::default(),
        }
    }

    /// The pane host answers the next claim.
    fn claim(&self, answer: Asked) {
        self.host.answer("pane.claim", [answer]);
    }

    fn claim_with(&self, answer: serde_json::Value) {
        self.claim(Asked::Now(Ok(answer)));
    }

    /// The broker answers the next message with `body` in `status`.
    fn replies(&self, status: u16, body: &str) {
        let body = Body::Now(body.as_bytes().to_vec());
        self.fakes
            .loopback
            .serve("POST /deliver", [Served::Head { status, body }]);
    }

    /// Begins sending `text` to `thread` through `pane`.
    fn begin(&mut self, id: usize, thread: Option<&str>, pane: Pane, text: &str) {
        let (time, loopback) = (self.fakes.time.clone(), self.fakes.loopback.clone());
        let (host, channel) = (Rc::clone(&self.host), self.channel.clone());
        let (thread, text) = (thread.map(str::to_owned), text.to_owned());
        self.driver.begin(id, async move {
            let target = Target {
                channel: &channel,
                thread: thread.as_deref(),
                pane: &pane,
                host: &*host,
            };
            send(&*time, &*loopback, &target, &text).await
        });
    }

    /// A send to the thread of the window, which settles at once.
    fn sends(&mut self, text: &str) -> Result<Answer, String> {
        let pane = self.pane.clone();
        self.begin(0, Some(THREAD), pane, text);
        let mut settled: Settled = self.driver.run();
        assert_eq!(settled.len(), 1, "settled at once");
        settled.remove(0).1
    }
}

fn admitted() -> Answer {
    Answer {
        ok: true,
        admitted: Some(true),
        error: None,
        zero_bytes: false,
        cause: None,
    }
}

/// Refused with nothing written, `cause` the words the host gave for it.
fn refused(error: &str, cause: Option<&str>) -> Answer {
    Answer {
        ok: false,
        admitted: Some(false),
        error: Some(error.to_owned()),
        zero_bytes: true,
        cause: cause.map(str::to_owned),
    }
}

fn uncertain(cause: &str) -> Answer {
    Answer {
        ok: false,
        admitted: None,
        error: Some("uncertain".to_owned()),
        zero_bytes: false,
        cause: Some(cause.to_owned()),
    }
}

#[test]
fn a_message_is_claimed_first_and_then_handed_to_the_broker_for_its_thread_with_its_expiry() {
    let mut stage = Stage::new();
    stage.claim(Asked::Held);
    stage.replies(200, r#"{"ok":true,"admitted":true}"#);
    let pane = stage.pane.clone();
    stage.begin(0, Some(THREAD), pane, "a message with spaces\nand newlines");
    assert!(stage.driver.run().is_empty(), "waits for the claim");
    assert!(
        stage.fakes.loopback.take_asked().is_empty(),
        "the broker is not asked first"
    );
    assert_eq!(
        stage.host.take_asked(),
        [(
            "pane.claim".to_owned(),
            json!({ "pane": "p1-diana", "generation": 3 })
        )]
    );
    assert!(stage.host.release("pane.claim", Ok(json!({ "ok": true }))));
    assert_eq!(stage.driver.run().len(), 1);
    let asked = stage.fakes.loopback.take_asked();
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].method, Method::Post);
    assert_eq!(asked[0].url, "http://127.0.0.1:41000/deliver");
    assert_eq!(
        asked[0].headers,
        [
            ("authorization".to_owned(), format!("Bearer {TOKEN}")),
            ("content-type".to_owned(), "application/json".to_owned()),
        ]
    );
    let body = String::from_utf8(asked[0].body.clone().unwrap()).unwrap();
    assert_eq!(
        body,
        format!(
            r#"{{"launchId":"{LAUNCH}","sessionId":"{THREAD}","text":"a message with spaces\nand newlines","expiresAt":{}}}"#,
            EPOCH_MS + 3000
        )
    );
}

#[test]
fn a_message_the_broker_takes_is_admitted_as_it_was_written_and_nothing_is_left_armed() {
    let mut stage = Stage::new();
    stage.claim_with(json!({ "ok": true }));
    stage.replies(200, r#"{"ok":true,"admitted":true}"#);
    let text = "  \n a message with space around it \n\n";
    assert_eq!(stage.sends(text), Ok(admitted()));
    let asked = stage.fakes.loopback.take_asked();
    let body: Value = serde_json::from_slice(asked[0].body.as_ref().unwrap()).unwrap();
    assert_eq!(body["text"], text);
    assert!(stage.fakes.time.waits(0).is_empty());
}
