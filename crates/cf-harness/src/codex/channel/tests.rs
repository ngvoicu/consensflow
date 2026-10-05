//! What the broker of a Codex window says of it: which thread the TUI shows
//! and whether it would take a message, within a second, or nothing.

use serde_json::json;

use super::*;
use crate::seams::loopback::BodyFailed;
use crate::testing::{finished, Driver, Fakes, Sent, Served};

const LAUNCH: &str = "0a1b2c3d-4e5f-4061-8a7b-9c0d1e2f3a4b";
const THREAD: &str = "0f8fad5b-d9cb-469f-a165-70867728950e";
const TOKEN: &str = "AwoRGB8mLTQ7QklQV15lbHN6gYiPlp2k";

#[test]
fn a_thread_is_a_uuid_of_version_one_to_eight_in_either_case() {
    assert!(is_thread(THREAD));
    assert!(is_thread(&THREAD.to_uppercase()));
    assert!(is_thread("01a0817b-e6b0-7f32-8e11-370dc000cbc0"));
    assert!(is_thread("01a0817b-e6b0-1f32-bE11-370dc000cbc0"));
    for not in [
        "",
        "not-a-uuid",
        // Its version is 1 to 8, its variant 8, 9, a or b.
        "01a0817b-e6b0-0f32-8e11-370dc000cbc0",
        "01a0817b-e6b0-9f32-8e11-370dc000cbc0",
        "01a0817b-e6b0-7f32-7e11-370dc000cbc0",
        "01a0817b-e6b0-7f32-ce11-370dc000cbc0",
        // The whole of it, and nothing else.
        "01a0817b-e6b0-7f32-8e11-370dc000cbc",
        "01a0817b-e6b0-7f32-8e11-370dc000cbc00",
        "01a0817b-e6b0-7f32-8e11-370dc000cbc0\n",
        " 01a0817b-e6b0-7f32-8e11-370dc000cbc0",
        "01a0817be6b0-7f32-8e11-370dc000cbc0-",
        "g1a0817b-e6b0-7f32-8e11-370dc000cbc0",
        "01a0817b-e6b0-7f32-8e11-370dc000cbc\u{e9}",
    ] {
        assert!(!is_thread(not), "{not:?}");
    }
}

#[test]
fn a_session_is_a_thread_only_when_it_is_the_one_the_window_has() {
    assert!(Session::Thread(THREAD.to_owned()).is(Some(THREAD)));
    assert!(!Session::Thread(THREAD.to_owned()).is(Some(&THREAD.to_uppercase())));
    assert!(!Session::Thread(THREAD.to_owned()).is(None));
    // A window with no thread yet shows none, and null is null.
    assert!(Session::Unnamed.is(None));
    assert!(!Session::Unnamed.is(Some(THREAD)));
    assert!(!Session::Wrapped.is(Some(THREAD)));
    assert!(!Session::Wrapped.is(None));
}

/// A channel to a broker the test scripts, on the fakes of its own.
struct Stage {
    fakes: Fakes,
    channel: Channel,
}

impl Stage {
    fn new() -> Self {
        Self::at("http://127.0.0.1:41000")
    }

    fn at(endpoint: &str) -> Self {
        Self {
            fakes: Fakes::new(&cf_base::env::Env::default()),
            channel: Channel::new(LAUNCH, endpoint.to_owned(), TOKEN.to_owned()),
        }
    }

    /// The broker answers the next question with `body`, in `status`.
    fn says(&self, body: &str, status: u16) {
        let body = Sent::Now(body.as_bytes().to_vec());
        self.fakes
            .loopback
            .serve("GET /session", [Served::Head { status, body }]);
    }

    fn shown(&self) -> Option<Shown> {
        finished(Box::pin(
            self.channel.shown(&*self.fakes.time, &*self.fakes.loopback),
        ))
    }
}

/// What the broker says of a window that shows `session`.
fn word(session: serde_json::Value, available: bool) -> String {
    json!({ "launchId": LAUNCH, "sessionId": session, "available": available }).to_string()
}

#[test]
fn the_broker_is_asked_with_the_launch_s_token_and_nothing_else() {
    let stage = Stage::new();
    stage.says(&word(json!(THREAD), true), 200);
    assert_eq!(
        stage.shown(),
        Some(Shown {
            session: Session::Thread(THREAD.to_owned()),
            available: true,
        })
    );
    let asked = stage.fakes.loopback.take_asked();
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].method, Method::Get);
    assert_eq!(asked[0].url, "http://127.0.0.1:41000/session");
    assert_eq!(
        asked[0].headers,
        [("authorization".to_owned(), format!("Bearer {TOKEN}"))]
    );
    assert_eq!(asked[0].body, None);
}

#[test]
fn a_url_is_written_as_new_url_writes_it() {
    for (endpoint, url) in [
        // The default port of a scheme is not written.
        ("http://127.0.0.1:80", "http://127.0.0.1/session"),
        ("http://127.0.0.1:41000/", "http://127.0.0.1:41000/session"),
        // A route with a slash is from the root.
        (
            "http://127.0.0.1:41000/a/b",
            "http://127.0.0.1:41000/session",
        ),
    ] {
        let stage = Stage::at(endpoint);
        assert_eq!(stage.shown(), None);
        assert_eq!(stage.fakes.loopback.take_asked()[0].url, url, "{endpoint}");
    }
    let nowhere = Stage::at("not a url");
    assert_eq!(nowhere.shown(), None);
    assert!(
        nowhere.fakes.loopback.take_asked().is_empty(),
        "nothing was asked"
    );
}

#[test]
fn a_word_is_read_as_a_web_reply_is_read() {
    let stage = Stage::new();
    let said = word(json!(THREAD), true);
    let shown = Some(Shown {
        session: Session::Thread(THREAD.to_owned()),
        available: true,
    });
    // A byte order mark is taken off, and any white space around it is JSON's.
    stage.says(&format!("\u{feff}{said}"), 200);
    assert_eq!(stage.shown(), shown);
    stage.says(&format!(" \n{said}\r\n\t"), 200);
    assert_eq!(stage.shown(), shown);
    for status in [200, 201, 204, 299] {
        stage.says(&said, status);
        assert_eq!(stage.shown(), shown, "{status}");
    }
    for status in [199, 300, 301, 404, 500] {
        stage.says(&said, status);
        assert_eq!(stage.shown(), None, "{status}");
    }
    // Two byte order marks are a document that begins with a mark.
    stage.says(&format!("\u{feff}\u{feff}{said}"), 200);
    assert_eq!(stage.shown(), None);
}

#[test]
fn a_window_available_is_one_the_broker_says_is_available_in_a_flag() {
    let stage = Stage::new();
    for (available, expected) in [
        (json!(true), true),
        (json!(false), false),
        (json!(1), false),
        (json!("true"), false),
        (json!(null), false),
    ] {
        let body = json!({ "launchId": LAUNCH, "sessionId": THREAD, "available": available });
        stage.says(&body.to_string(), 200);
        assert_eq!(stage.shown().map(|shown| shown.available), Some(expected));
    }
    stage.says(
        &json!({ "launchId": LAUNCH, "sessionId": null }).to_string(),
        200,
    );
    assert_eq!(
        stage.shown(),
        Some(Shown {
            session: Session::Unnamed,
            available: false,
        })
    );
}

#[test]
fn a_word_that_is_not_the_broker_s_on_this_launch_names_no_thread() {
    let stage = Stage::new();
    let other = "1b2c3d4e-5f60-4172-8b9c-0d1e2f3a4b5c";
    let this = |session: serde_json::Value| json!({ "launchId": LAUNCH, "sessionId": session });
    for body in [
        json!({ "launchId": other, "sessionId": THREAD }),
        json!({ "launchId": "", "sessionId": THREAD }),
        json!({ "launchId": 7, "sessionId": THREAD }),
        json!({ "sessionId": THREAD }),
        json!({ "launchId": LAUNCH }),
        this(json!("not-a-uuid")),
        this(json!(format!("{THREAD}\n"))),
        this(json!(7)),
        this(json!(true)),
        this(json!({ "id": THREAD })),
        // A list of two is no id; a list of one is wrapped, below.
        this(json!([THREAD, null])),
        this(json!([])),
        json!(null),
        json!([]),
        json!(7),
        json!("text"),
    ] {
        stage.says(&body.to_string(), 200);
        assert_eq!(stage.shown(), None, "{body}");
    }
    for body in ["", "not json", "{\"launchId\":", "{}{}"] {
        stage.says(body, 200);
        assert_eq!(stage.shown(), None, "{body:?}");
    }
}

#[test]
fn an_id_in_a_list_is_read_as_the_id_by_javascript_s_test_and_is_no_thread_of_the_window() {
    let stage = Stage::new();
    for session in [json!([THREAD]), json!([[THREAD]])] {
        stage.says(&word(session, true), 200);
        assert_eq!(
            stage.shown(),
            Some(Shown {
                session: Session::Wrapped,
                available: true,
            })
        );
    }
}

#[test]
fn a_reply_is_read_whole_within_a_size_and_not_past_it() {
    let stage = Stage::new();
    let said = word(json!(THREAD), true);
    // JSON's white space after the document: the size is exactly the limit.
    let padded = |size: usize| format!("{said}{}", " ".repeat(size - said.len()));
    stage.says(&padded(BODY_LIMIT), 200);
    assert!(stage.shown().is_some());
    stage.says(&padded(BODY_LIMIT + 1), 200);
    assert_eq!(stage.shown(), None);
}

#[test]
fn nobody_answering_is_no_word_and_leaves_no_timer_armed() {
    let stage = Stage::new();
    let mut driver = Driver::default();
    let (time, loopback, channel) = (
        stage.fakes.time.clone(),
        stage.fakes.loopback.clone(),
        stage.channel.clone(),
    );
    driver.begin(0, async move { channel.shown(&*time, &*loopback).await });
    assert_eq!(driver.run(), [(0, None)]);
    assert!(stage.fakes.time.waits(0).is_empty());
}

#[test]
fn the_broker_has_a_second_for_its_head_and_its_body_together() {
    let stage = Stage::new();
    let (time, loopback, channel) = (
        stage.fakes.time.clone(),
        stage.fakes.loopback.clone(),
        stage.channel.clone(),
    );
    let mut driver = Driver::default();
    let begin = |driver: &mut Driver<Option<Shown>>, id: usize| {
        let (time, loopback, channel) = (time.clone(), loopback.clone(), channel.clone());
        driver.begin(id, async move { channel.shown(&*time, &*loopback).await });
    };
    // A head that never comes.
    stage.fakes.loopback.serve("GET /session", [Served::Held]);
    begin(&mut driver, 0);
    assert!(driver.run().is_empty());
    assert_eq!(stage.fakes.time.waits(0), [1000]);
    assert_eq!(stage.fakes.loopback.waits(0), ["GET /session"]);
    assert!(stage
        .fakes
        .time
        .fire_next(stage.fakes.time.wall_ms() + 1000));
    assert_eq!(driver.run(), [(0, None)]);
    // A head that comes at once and a body that waits: the second is one for both.
    let held = Served::Head {
        status: 200,
        body: Sent::Held,
    };
    stage.fakes.loopback.serve("GET /session", [held.clone()]);
    begin(&mut driver, 1);
    assert!(driver.run().is_empty());
    assert_eq!(stage.fakes.time.waits(1), [1000]);
    stage.fakes.time.settle_at(stage.fakes.time.wall_ms() + 400);
    assert_eq!(stage.fakes.time.waits(1), [600]);
    let said = word(json!(THREAD), true).into_bytes();
    assert!(stage.fakes.loopback.release_body("GET /session", Ok(said)));
    let [(1, Some(shown))] = &driver.run()[..] else {
        panic!("not heard");
    };
    assert!(shown.available);
    assert!(
        stage.fakes.time.waits(1).is_empty(),
        "the timer went with the request"
    );
    // A body the connection broke.
    stage.fakes.loopback.serve("GET /session", [held]);
    begin(&mut driver, 2);
    assert!(driver.run().is_empty());
    assert!(stage
        .fakes
        .loopback
        .release_body("GET /session", Err(BodyFailed::Cut)));
    assert_eq!(driver.run(), [(2, None)]);
}
