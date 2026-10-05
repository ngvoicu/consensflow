//! One window of the fake adapter (`FakeAdapter`): the harness's window its
//! agent is in, as the engine looks at it, hands it a message and waits for
//! its first. Each call does and reads what it does when called, is answered
//! a turn later, as the JavaScript fake's promises were, and is written down
//! in the Node traces' shape.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use cf_harness::contract::{Admission, Observed, Pane, PaneHost, Readiness, Waiting, Window, Work};
use cf_harness::records::{Item, Reading, Record, Role, Settlement};
use serde_json::{json, Value};

use super::adapter::{FakeAdapter, FakeAgent};
use super::executor::next_turn;

/// One window of the fake: its agent, by its launch, and the conversation
/// the window is followed on (the launch bag's `nativeSession`).
pub(super) struct FakeWindow {
    seam: String,
    launch: String,
    session: RefCell<String>,
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
            fake,
        }
    }

    fn call(&self, method: &str, args: Value) -> usize {
        self.fake.recorder.call(&self.seam, Some(method), args)
    }

    /// What a delivery of `text` does to the agent: it is taken or refused.
    fn take(&self, text: &str) -> Admission {
        self.fake.of_launch(&self.launch, |agent| {
            if !agent.admit {
                return Admission::Refused {
                    reason: "refused by the test".to_owned(),
                };
            }
            if agent.arrive {
                agent.items.push(self.fake.item(Role::User, text));
                agent.settled = false;
                // A message that arrives starts a turn: the one before it ended as it did.
                agent.failed = false;
            }
            Admission::Admitted {
                queued: agent.queued,
            }
        })
    }

    /// What a look at the agent's window finds, its launch on `session`.
    fn look(&self, session: &str) -> Observed {
        self.fake
            .of_launch(&self.launch, |agent| looked(agent, session))
    }
}

impl Window for FakeWindow {
    fn opened(&self, _pid: Option<u32>) {}

    fn follow(&self, session: &str) {
        *self.session.borrow_mut() = session.to_owned();
    }

    fn started(&self) -> Work<'_, Result<Option<String>, String>> {
        let at = self.call(
            "started",
            json!([{ "launch": { "launchId": self.launch } }]),
        );
        let started = self.fake.started.borrow().clone();
        let answer = started.map_or(Ok(None), |started| started());
        Box::pin(async move {
            next_turn().await;
            let written = match &answer {
                Ok(Some(native)) => json!({ "nativeSession": native }),
                Ok(None) => json!({}),
                Err(error) => json!({ "$error": { "message": error } }),
            };
            self.fake.recorder.answered(at, written);
            answer
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
        let at = self.call("ready", json!([{ "launch": { "launchId": self.launch } }]));
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
                "launch": { "launchId": self.launch },
                "pane": { "id": pane.id, "generation": pane.generation },
                "text": text,
            }]),
        );
        // Made when called, or once the test lets a held delivery go.
        let held = self.fake.hold_deliveries.borrow().clone();
        let early = held.is_none().then(|| self.take(text));
        Box::pin(async move {
            if let Some(gate) = held {
                gate.wait().await;
            }
            let admission = early.unwrap_or_else(|| self.take(text));
            next_turn().await;
            let written = match &admission {
                Admission::Admitted { queued: true } => json!({ "admitted": true, "queued": true }),
                Admission::Admitted { queued: false } => json!({ "admitted": true }),
                Admission::Refused { reason } | Admission::Uncertain { reason } => {
                    json!({ "admitted": false, "reason": reason })
                }
            };
            self.fake.recorder.answered(at, written);
            Ok(admission)
        })
    }

    fn observe(&self) -> Work<'_, Result<Observed, String>> {
        // JavaScript handed the fake the participant's conversation, which
        // this window follows: its session.
        let session = self.session.borrow().clone();
        let at = self.call(
            "observe",
            json!([{
                "launch": { "launchId": self.launch },
                "conversation": { "nativeSession": session },
            }]),
        );
        // Made when called, or once the test lets a held look go.
        let held = self
            .fake
            .hold_observes
            .borrow()
            .clone()
            .filter(|(launch, _)| *launch == self.launch)
            .map(|(_, gate)| gate);
        let early = held.is_none().then(|| self.look(&session));
        Box::pin(async move {
            if let Some(gate) = held {
                gate.wait().await;
            }
            let observed = early.unwrap_or_else(|| self.look(&session));
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
