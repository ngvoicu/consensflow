//! One window of the fake adapter: its agent, by its launch, and the
//! conversation the window is followed on (the launch bag's
//! `nativeSession`), asked as the engine asks a harness's window. Every call
//! is written down in the Node traces' shape, its launch bag as the
//! JavaScript fake's was: the launch, its conversation, and the window's
//! process once the pane host named it.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use cf_harness::contract::{Admission, Observed, Pane, PaneHost, Readiness, Waiting, Window, Work};
use cf_harness::records::{Item, Reading, Record, Settlement};
use serde_json::{json, Value};

use super::adapter::{FakeAdapter, FakeAgent};
use super::executor::next_turn;

pub(super) struct FakeWindow {
    seam: String,
    launch: String,
    session: RefCell<String>,
    pid: Cell<Option<u32>>,
    fake: Rc<FakeAdapter>,
}

impl FakeWindow {
    pub(super) fn new(
        seam: String,
        launch: String,
        session: String,
        fake: Rc<FakeAdapter>,
    ) -> Self {
        Self {
            seam,
            launch,
            session: RefCell::new(session),
            pid: Cell::new(None),
            fake,
        }
    }

    fn call(&self, method: &str, args: Value) -> usize {
        self.fake.recorder.call(&self.seam, Some(method), args)
    }

    /// The launch bag as the engine holds it: the launch, the conversation
    /// it is on, and the window's process.
    fn bag(&self) -> Value {
        let mut bag = json!({
            "launchId": self.launch,
            "nativeSession": *self.session.borrow(),
        });
        if let Some(pid) = self.pid.get() {
            bag["pid"] = json!(pid);
        }
        bag
    }
}

impl Window for FakeWindow {
    fn opened(&self, pid: Option<u32>) {
        self.pid.set(pid);
    }

    fn follow(&self, session: &str) {
        *self.session.borrow_mut() = session.to_owned();
    }

    fn started(&self) -> Work<'_, Result<Option<String>, String>> {
        let at = self.call("started", json!([{ "launch": self.bag() }]));
        let failure = self.fake.fail_started.borrow().clone();
        Box::pin(async move {
            next_turn().await;
            match failure {
                Some(reason) => {
                    self.fake
                        .recorder
                        .answered(at, json!({ "$error": { "message": reason } }));
                    Err(reason)
                }
                None => {
                    self.fake.recorder.answered(at, json!({}));
                    Ok(None)
                }
            }
        })
    }

    fn ready<'a>(
        &'a self,
        _host: &'a dyn PaneHost,
        _pane: &'a Pane,
    ) -> Work<'a, Result<Readiness, String>> {
        let ready = self.fake.ready.borrow().clone();
        let Some(ready) = ready else {
            return Box::pin(async { Ok(Readiness::Ready) });
        };
        let at = self.call("ready", json!([{ "launch": self.bag() }]));
        let answer = ready();
        Box::pin(async move {
            next_turn().await;
            let written = match &answer {
                Ok(Readiness::Ready) => json!(true),
                Ok(Readiness::Held(held)) => json!(held.sentence()),
                Err(error) => json!({ "$error": { "message": error } }),
            };
            self.fake.recorder.answered(at, written);
            answer
        })
    }

    fn deliver<'a>(
        &'a self,
        _host: &'a dyn PaneHost,
        pane: &'a Pane,
        text: &'a str,
    ) -> Work<'a, Result<Admission, String>> {
        let at = self.call(
            "deliver",
            json!([{
                "launch": self.bag(),
                "pane": { "id": pane.id, "generation": pane.generation },
                "text": text,
            }]),
        );
        let fake = Rc::clone(&self.fake);
        let (launch, given) = (self.launch.clone(), text.to_owned());
        let taking: super::adapter::Taking = Box::new(move || fake.take(&launch, &given));
        let own = self.fake.deliver.borrow().clone();
        // What a delivery does is done when it is called, as the JavaScript
        // fake's was; a test's own `deliver` does what it does in its place.
        let outcome: Work<'a, Result<Admission, String>> = match own {
            Some(own) => own(taking),
            None => {
                let admission = taking();
                Box::pin(async move { Ok(admission) })
            }
        };
        Box::pin(async move {
            let outcome = outcome.await;
            next_turn().await;
            let written = match &outcome {
                Ok(Admission::Admitted { queued: true }) => {
                    json!({ "admitted": true, "queued": true })
                }
                Ok(Admission::Admitted { queued: false }) => json!({ "admitted": true }),
                Ok(Admission::Refused { reason }) => json!({ "admitted": false, "reason": reason }),
                Ok(Admission::Uncertain { reason }) => {
                    json!({ "admitted": null, "reason": reason })
                }
                Err(error) => json!({ "$error": { "message": error } }),
            };
            self.fake.recorder.answered(at, written);
            outcome
        })
    }

    fn observe(&self) -> Work<'_, Result<Observed, String>> {
        // JavaScript handed the fake the participant's conversation, which
        // this window follows: its session.
        let session = self.session.borrow().clone();
        let at = self.call(
            "observe",
            json!([{
                "launch": self.bag(),
                "conversation": { "nativeSession": session },
            }]),
        );
        let observed = self
            .fake
            .of_launch(&self.launch, |agent| looked(agent, &session));
        Box::pin(async move {
            next_turn().await;
            self.fake.recorder.answered(at, observed_json(&observed));
            Ok(observed)
        })
    }
}

/// What a look at `agent`'s window finds, its launch on `session`.
fn looked(agent: &FakeAgent, session: &str) -> Observed {
    let reading = |items: Vec<Item>| {
        Some(Arc::new(Reading::Known(Record {
            items,
            in_flight: false,
            asking: false,
            failed: false,
            quota: None,
            settlement: Settlement::Unknown,
        })))
    };
    // A window that shows another conversation than its launch's: that
    // record's last look, and which session it shows now.
    if let Some(shows) = agent.shows.as_ref().filter(|shows| *shows != session) {
        return Observed {
            reading: reading(agent.records.get(session).cloned().unwrap_or_default()),
            settled: false,
            waiting: None,
            failed: agent.failed,
            quota: agent.quota.clone(),
            switched: Some(shows.clone()),
            unnamed: false,
        };
    }
    let waiting = match &agent.unnamed {
        Some(reason) => Some(Waiting {
            reason: Some(reason.clone()),
        }),
        None => agent.waiting.clone(),
    };
    Observed {
        reading: reading(agent.items.clone()),
        settled: agent.settled,
        waiting,
        failed: agent.failed,
        quota: agent.quota.clone(),
        switched: None,
        unnamed: agent.unnamed.is_some(),
    }
}

/// A look as the Node traces write the fake's answer.
fn observed_json(observed: &Observed) -> Value {
    let mut written = json!({
        "items": observed.items(),
        "settled": observed.settled,
        "waiting": observed.waiting.as_ref().map(|waiting| json!({ "reason": waiting.reason })),
        "quota": observed.quota.as_deref(),
        "failed": observed.failed,
    });
    if let Some(session) = &observed.switched {
        written["switched"] = json!({ "nativeSession": session });
    }
    if observed.unnamed {
        written["unnamed"] = json!(true);
    }
    written
}
