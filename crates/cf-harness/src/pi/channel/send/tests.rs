//! A send played on a clock that moves when the test moves it: the record it
//! writes and how it looks for the extension's verdict (`record`), what each
//! verdict and each failure after the hand-over make of the answer
//! (`verdicts`), and what is refused before it (`refusals`). Node's
//! `tests/pi-message.test.mjs` holds the same channel against the real
//! extension on the machine's own clock (and against this one's build,
//! through `pi-send`).

mod record;
mod refusals;
mod verdicts;

use std::cell::Cell;
use std::fs;
use std::rc::Rc;

use cf_base::env::Env;
use serde_json::json;
use tempfile::TempDir;

use super::*;
use crate::contract::{HostError, Work};
use crate::testing::{AnsweringHost, Driver, Fakes, ManualTime, ScriptedEntropy};

const LAUNCH: &str = "launch-pi-test";

/// How many times the clock may be read with no wait between before the
/// loop that reads it is said to be one that does not wait.
const PATIENCE: u32 = 10_000;

/// The stage's clock, which fails the test where a loop reads it again and
/// again with no sleep between: such a loop never ends on a clock that moves
/// only with timers, and a test would hang in it instead of failing.
struct Impatient {
    inner: Rc<ManualTime>,
    reads: Cell<u32>,
}

impl Time for Impatient {
    fn wall_ms(&self) -> i64 {
        self.reads.set(self.reads.get() + 1);
        assert!(
            self.reads.get() < PATIENCE,
            "the clock was read {PATIENCE} times with no wait between"
        );
        self.inner.wall_ms()
    }

    fn sleep(&self, duration: Duration) -> Work<'_, ()> {
        self.reads.set(0);
        self.inner.sleep(duration)
    }
}

/// How the pane host answers a claim.
type Claim = Box<dyn Fn() -> Result<Value, HostError>>;

/// A host that answers a claim as a [`Claim`] says.
type Host = AnsweringHost<Box<dyn Fn(&str) -> Result<Value, HostError>>>;

/// What a send settled with, by the work that began it.
type Settled = Vec<(usize, Result<Answer, String>)>;

/// A send in a folder of its own, its host, clock and randomness the fakes'.
struct Stage {
    dir: TempDir,
    /// Where the inbox is, when it is not in the folder of the stage.
    inbox_at: Option<String>,
    time: Rc<ManualTime>,
    entropy: Rc<ScriptedEntropy>,
    host: Rc<Host>,
    driver: Driver<Result<Answer, String>>,
}

impl Stage {
    /// A stage whose host answers each claim with `claim`.
    fn answering(claim: Claim) -> Self {
        let fakes = Fakes::new(&Env::default());
        Self {
            dir: tempfile::tempdir().unwrap(),
            inbox_at: None,
            time: Rc::clone(&fakes.time),
            entropy: Rc::clone(&fakes.entropy),
            host: Rc::new(AnsweringHost::new(Box::new(move |_: &str| claim()))),
            driver: Driver::default(),
        }
    }

    fn admitting() -> Self {
        Self::answering(Box::new(|| Ok(json!({ "ok": true }))))
    }

    fn inbox(&self) -> String {
        self.inbox_at
            .clone()
            .unwrap_or_else(|| path::join(&[&self.dir.path().to_string_lossy(), "inbox"]))
    }

    fn ack(&self) -> String {
        path::join(&[&self.dir.path().to_string_lossy(), "ack"])
    }

    /// Begins sending `text` to `session` through a pane of `generation`, the
    /// extension given `timeout` milliseconds.
    fn begin(&mut self, op: usize, text: &str, session: &str, generation: u64, timeout: u64) {
        let time = Impatient {
            inner: Rc::clone(&self.time),
            reads: Cell::new(0),
        };
        let (entropy, host) = (Rc::clone(&self.entropy), Rc::clone(&self.host));
        let (inbox, ack) = (self.inbox(), self.ack());
        let (text, session) = (text.to_owned(), session.to_owned());
        self.driver.begin(op, async move {
            let pane = Pane {
                id: "s1-zeus".to_owned(),
                generation,
            };
            let target = Target {
                launch_id: LAUNCH,
                inbox: &inbox,
                ack: &ack,
                ack_timeout_ms: timeout,
                session: &session,
                pane: &pane,
                host: &*host,
            };
            send(&time, &*entropy, &target, &text).await
        });
    }

    /// Sends `text` as the window's own, to its session, and runs the work
    /// until it waits.
    fn send(&mut self, text: &str) -> Settled {
        self.begin(0, text, "native-pi-session", 1, 30_000);
        self.driver.run()
    }

    /// The timers of the send, in milliseconds from now.
    fn waits(&self) -> Vec<i64> {
        self.time.waits(0)
    }

    /// Fires the next timer of the send, if one is due by `within`
    /// milliseconds from now: what then settled.
    fn fire(&mut self, within: i64) -> Option<Settled> {
        let fired = self.time.fire_next(self.time.wall_ms() + within);
        fired.then(|| self.driver.run())
    }

    /// Fires the timers of the send until it settles: its answer.
    fn until_answered(&mut self) -> Answer {
        loop {
            let settled = self.fire(i64::MAX / 2).expect("a timer left");
            if !settled.is_empty() {
                return answered(settled);
            }
        }
    }

    /// The names in `folder`, sorted.
    fn names(&self, folder: &str) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(folder)
            .map(|entries| {
                entries
                    .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }

    /// The extension's verdict on the message in the inbox, written as it
    /// writes one.
    fn acknowledges(&self, fields: &Value) -> String {
        let name = self.names(&self.inbox()).remove(0);
        let id = name.strip_suffix(".json").unwrap().to_owned();
        let mut verdict = json!({ "id": id });
        verdict
            .as_object_mut()
            .unwrap()
            .extend(fields.as_object().unwrap().clone());
        fs::create_dir_all(self.ack()).unwrap();
        fs::write(path::join(&[&self.ack(), &name]), verdict.to_string()).unwrap();
        id
    }
}

/// The answer a send settled with, which must be Ok.
fn answered(settled: Settled) -> Answer {
    match settled.into_iter().next() {
        Some((_, Ok(answer))) => answer,
        other => panic!("not answered: {other:?}"),
    }
}

/// What `admission` would read of an answer: its `ok`, whether it was
/// refused, its words.
fn read(answer: &Answer) -> (bool, bool, Option<&str>, Option<&str>) {
    let sent = answer.reading();
    (sent.ok, sent.refused, answer.cause.as_deref(), answer.error)
}
