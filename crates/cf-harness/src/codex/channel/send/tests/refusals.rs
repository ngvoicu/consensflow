//! What a send refuses before the broker is asked: a claim the host refused,
//! a pane or a thread that cannot be named, and a deadline that has come; and
//! what is left of the deadline for the broker. The deadline is read on a
//! clock that reads the times a test gives it.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;

use super::*;
use crate::contract::{HostError, Work};

#[test]
fn a_claim_the_host_refuses_asks_the_broker_nothing_and_is_refused_in_the_host_s_words() {
    for (claimed, cause) in [
        (
            json!({ "ok": false, "admitted": false, "error": "stale-generation", "cause": "gone" }),
            "gone",
        ),
        (
            json!({ "ok": false, "error": "paste-in-flight" }),
            "paste-in-flight",
        ),
        (json!({ "ok": false, "cause": "", "error": "x" }), ""),
        (
            json!({ "ok": false, "cause": null, "error": "later" }),
            "later",
        ),
        // What is no words is written as a template writes it.
        (json!({ "ok": false, "cause": 0 }), "0"),
        (
            json!({ "ok": false, "error": { "a": 1 } }),
            "[object Object]",
        ),
        // A host that answered in a word is its own cause.
        (json!("stale"), "stale"),
        (json!({ "ok": false }), "claim-refused"),
        (json!({ "ok": 1 }), "claim-refused"),
        (json!({ "ok": "true" }), "claim-refused"),
        (json!(null), "claim-refused"),
        (json!([]), "claim-refused"),
        (json!(7), "claim-refused"),
    ] {
        let mut stage = Stage::new();
        stage.claim_with(claimed.clone());
        assert_eq!(
            stage.sends("stale message"),
            Ok(refused("failed-with-zero-bytes", Some(cause))),
            "{claimed}"
        );
        assert!(stage.fakes.loopback.take_asked().is_empty(), "{claimed}");
    }
}

#[test]
fn a_host_that_never_answered_is_a_claim_that_failed_in_its_cause_or_its_message() {
    for (error, cause) in [(Some("eof"), "eof"), (None, "bridge ended")] {
        let mut stage = Stage::new();
        stage.claim(Asked::Now(Err(HostError {
            error: error.map(str::to_owned),
            message: "bridge ended".to_owned(),
        })));
        assert_eq!(
            stage.sends("x"),
            Ok(refused("failed-with-zero-bytes", Some(cause)))
        );
    }
}

#[test]
fn a_pane_the_host_cannot_name_is_refused_before_anyone_is_asked() {
    let sentence = "codex-queue delivery needs pane {id, generation}";
    let pane = |id: &str, generation: u64| Pane {
        id: id.to_owned(),
        generation,
    };
    for refused in [
        pane("", 1),
        pane("p1", 0),
        pane("p1", 1 << 53),
        pane("p1", u64::MAX),
    ] {
        let mut stage = Stage::new();
        stage.begin(0, Some(THREAD), refused.clone(), "x");
        assert_eq!(
            stage.driver.run().remove(0).1.err().as_deref(),
            Some(sentence),
            "{refused:?}"
        );
        assert!(stage.host.take_asked().is_empty());
        assert!(stage.fakes.loopback.take_asked().is_empty());
    }
    // The first and the last generation a double holds exactly.
    for named in [pane("p1", 1), pane("p1", (1 << 53) - 1)] {
        let mut stage = Stage::new();
        stage.claim_with(json!({ "ok": true }));
        stage.replies(200, r#"{"ok":true,"admitted":true}"#);
        stage.begin(0, Some(THREAD), named, "x");
        assert_eq!(stage.driver.run().remove(0).1, Ok(admitted()));
    }
}

#[test]
fn a_thread_that_is_no_id_is_refused_before_anyone_is_asked_and_after_the_pane() {
    let sentence = "codex-queue delivery needs a canonical native session UUID";
    for thread in [
        None,
        Some(""),
        Some("not-a-uuid"),
        Some("aaaaaaaa-bbbb-4ccc-8ddd-40940940940"),
        Some("0f8fad5b-d9cb-469f-a165-70867728950e\n"),
    ] {
        let mut stage = Stage::new();
        let pane = stage.pane.clone();
        stage.begin(0, thread, pane, "invalid session");
        assert_eq!(
            stage.driver.run().remove(0).1.err().as_deref(),
            Some(sentence),
            "{thread:?}"
        );
        assert!(stage.host.take_asked().is_empty());
        assert!(stage.fakes.loopback.take_asked().is_empty());
    }
    let mut stage = Stage::new();
    stage.begin(
        0,
        None,
        Pane {
            id: String::new(),
            generation: 0,
        },
        "x",
    );
    let refused = stage.driver.run().remove(0).1.unwrap_err();
    assert!(
        refused.contains("pane"),
        "the pane is looked at first: {refused}"
    );
}

#[test]
fn a_claim_that_lasts_past_the_deadline_leaves_the_message_unsent_and_expired() {
    let mut stage = Stage::new();
    stage.claim(Asked::Held);
    let pane = stage.pane.clone();
    stage.begin(0, Some(THREAD), pane, "too late");
    assert!(stage.driver.run().is_empty());
    stage.fakes.time.settle_at(EPOCH_MS + 3000);
    assert!(stage.host.release("pane.claim", Ok(json!({ "ok": true }))));
    assert_eq!(stage.driver.run(), [(0, Ok(refused("expired", None)))]);
    assert!(stage.fakes.loopback.take_asked().is_empty());
}

#[test]
fn the_broker_has_what_is_left_of_the_deadline_with_a_millisecond_at_least() {
    for (late, left) in [(0, 3000), (1000, 2000), (2999, 1)] {
        let mut stage = Stage::new();
        stage.claim(Asked::Held);
        stage.fakes.loopback.serve("POST /deliver", [Served::Held]);
        let pane = stage.pane.clone();
        stage.begin(0, Some(THREAD), pane, "x");
        assert!(stage.driver.run().is_empty());
        stage.fakes.time.settle_at(EPOCH_MS + late);
        assert!(stage.host.release("pane.claim", Ok(json!({ "ok": true }))));
        assert!(stage.driver.run().is_empty());
        assert_eq!(stage.fakes.time.waits(0), [left], "{late}");
        // The deadline comes, and the message may be in the broker's hands.
        assert!(stage.fakes.time.fire_next(EPOCH_MS + 3000));
        assert_eq!(
            stage.driver.run(),
            [(0, Ok(uncertain("native-queue-transport")))]
        );
        assert!(stage.fakes.time.waits(0).is_empty());
    }
}

/// A clock that reads the times it is given, one a read, and the last from
/// then on; its waits are a manual clock's.
struct Reads {
    times: RefCell<VecDeque<i64>>,
    last: Cell<i64>,
    inner: Rc<crate::testing::ManualTime>,
}

impl Reads {
    /// A clock that reads `elapsed` milliseconds after the scenario's start,
    /// one a read.
    fn after(stage: &Stage, elapsed: &[i64]) -> Rc<Self> {
        Rc::new(Self {
            times: RefCell::new(elapsed.iter().map(|millis| EPOCH_MS + millis).collect()),
            last: Cell::new(EPOCH_MS),
            inner: stage.fakes.time.clone(),
        })
    }
}

impl Time for Reads {
    fn wall_ms(&self) -> i64 {
        if let Some(next) = self.times.borrow_mut().pop_front() {
            self.last.set(next);
        }
        self.last.get()
    }

    fn sleep(&self, duration: std::time::Duration) -> Work<'_, ()> {
        self.inner.sleep(duration)
    }
}

impl Stage {
    /// Begins a send of `x` to the thread of the window on the clock `time`.
    fn begin_on(&mut self, id: usize, time: Rc<Reads>) {
        let (loopback, host, channel) = (
            self.fakes.loopback.clone(),
            Rc::clone(&self.host),
            self.channel.clone(),
        );
        let pane = self.pane.clone();
        self.driver.begin(id, async move {
            let target = Target {
                channel: &channel,
                thread: Some(THREAD),
                pane: &pane,
                host: &*host,
            };
            send(&*time, &*loopback, &target, "x").await
        });
    }
}

#[test]
fn the_broker_has_a_millisecond_though_the_clock_passed_the_deadline_between_asking_and_arming() {
    let mut stage = Stage::new();
    stage.claim_with(json!({ "ok": true }));
    stage.fakes.loopback.serve("POST /deliver", [Served::Held]);
    // The reads of a send: its start, the three looks at the deadline, and the
    // one that says what is left.
    let time = Reads::after(&stage, &[0, 0, 0, 2999, 3000]);
    stage.begin_on(0, time);
    assert!(stage.driver.run().is_empty());
    assert_eq!(stage.fakes.time.waits(0), [1]);
}

#[test]
fn a_clock_that_passed_the_deadline_at_any_look_leaves_the_message_unsent_and_expired() {
    // The looks are before the claim, after it, and as the message is handed
    // over: a clock at the deadline at the first asks nobody, at the others
    // asks the host and not the broker.
    for (reads, claims) in [
        (&[0, 3000][..], 0),
        (&[0, 0, 3000], 1),
        (&[0, 0, 0, 3000], 1),
    ] {
        let mut stage = Stage::new();
        stage.claim_with(json!({ "ok": true }));
        let time = Reads::after(&stage, reads);
        stage.begin_on(0, time);
        assert_eq!(
            stage.driver.run(),
            [(0, Ok(refused("expired", None)))],
            "{reads:?}"
        );
        assert_eq!(stage.host.take_asked().len(), claims, "{reads:?}");
        assert!(stage.fakes.loopback.take_asked().is_empty(), "{reads:?}");
    }
}
