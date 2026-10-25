//! One window of the fake adapter: its agent, by its launch, and the
//! conversation the window is followed on (the launch bag's
//! `nativeSession`), asked as the engine asks a harness's window. Every call
//! is written down in the Node traces' shape, its launch bag as the
//! JavaScript fake's was: the launch, its conversation, and the window's
//! process once the pane host named it.
//!
//! The turn an `await` of a call costs is the engine's. The one exception is
//! `ready`, which the JavaScript fake does not have unless a test gives it
//! one, where the engine awaits none: a `ready` of the test's own takes that
//! turn here, since the engine cannot tell it is there.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use cf_harness::contract::{
    Admission, Held, Observed, Pane, PaneHost, Readiness, Waiting, Window, Work, NOT_ADMITTED,
};
use cf_harness::records::{Item, Reading, Record, Role, Settlement};
use serde_json::{json, Value};

use super::adapter::{FakeAdapter, FakeAgent, Taking};
use super::holds::Wrapped;
use crate::runtime::next_turn;

pub(super) struct FakeWindow {
    seam: String,
    launch: String,
    session: RefCell<String>,
    pid: Cell<Option<u32>>,
    fake: Rc<FakeAdapter>,
    /// The first message it takes once it is up, where its adapter's windows
    /// seed it so (`FakeAdapter::seeds`), not with the launch.
    first: Option<String>,
}

impl FakeWindow {
    pub(super) fn new(
        seam: String,
        launch: String,
        session: String,
        fake: Rc<FakeAdapter>,
        first: Option<String>,
    ) -> Self {
        Self {
            seam,
            launch,
            session: RefCell::new(session),
            pid: Cell::new(None),
            fake,
            first,
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

    /// What a look at the agent's window finds, its launch on `session`.
    fn look(&self, session: &str) -> Observed {
        self.fake
            .of_launch(&self.launch, |agent| looked(agent, session))
    }
}

impl Window for FakeWindow {
    fn opened(&self, pid: Option<u32>) {
        self.pid.set(pid);
    }

    fn follow(&self, session: &str) {
        *self.session.borrow_mut() = session.to_owned();
    }

    fn started<'a>(&'a self, host: &'a dyn PaneHost) -> Work<'a, Result<Option<String>, String>> {
        let args = json!([{ "launch": self.bag() }]);
        let at = self.call("started", args.clone());
        let wrapped = self.fake.start_holds.wrap(&args);
        let started = self.fake.started.borrow().clone();
        // Made when called, or once the test lets a held start go.
        let early = (!matches!(wrapped, Wrapped::Held { .. }))
            .then(|| started.as_ref().map_or(Ok(()), |started| started()));
        Box::pin(async move {
            // A start the test named a conversation for gives it.
            if let Some(native) = wrapped.before().await {
                self.fake
                    .recorder
                    .answered(at, json!({ "nativeSession": native }));
                return Ok(Some(native));
            }
            // A first message that goes in once the window is up, as OpenCode's
            // through its server, is handed over only if the host admits it then.
            if let Some(first) = &self.first {
                if !host.admits() {
                    self.fake
                        .recorder
                        .answered(at, json!({ "$error": { "message": NOT_ADMITTED } }));
                    return Err(NOT_ADMITTED.to_owned());
                }
                let item = self.fake.item(Role::User, first);
                self.fake.of_launch(&self.launch, |agent| {
                    agent.items.push(item);
                    agent.settled = false;
                });
            }
            let answer = early.unwrap_or_else(|| started.map_or(Ok(()), |started| started()));
            let answered = match answer {
                Ok(()) => {
                    self.fake.recorder.answered(at, json!({}));
                    Ok(None)
                }
                Err(reason) => {
                    self.fake
                        .recorder
                        .answered(at, json!({ "$error": { "message": reason } }));
                    Err(reason)
                }
            };
            wrapped.after().await;
            answered
        })
    }

    /// The test's word, where it gave one: no call of Node's fake.
    fn came_up(&self) -> Work<'_, Option<Option<String>>> {
        let came_up = self.fake.came_up.borrow().clone();
        Box::pin(async move { came_up.and_then(|came_up| came_up()) })
    }

    fn ready<'a>(
        &'a self,
        _host: &'a dyn PaneHost,
        pane: &'a Pane,
    ) -> Work<'a, Result<Readiness, String>> {
        let ready = self.fake.ready.borrow().clone();
        let Some(ready) = ready else {
            return Box::pin(async { Ok(Readiness::Ready) });
        };
        let args = json!([{
            "launch": self.bag(),
            "pane": { "id": pane.id, "generation": pane.generation },
        }]);
        let at = self.call("ready", args.clone());
        let wrapped = self.fake.ready_holds.wrap(&args);
        // Made when called, or once the test lets a held call go.
        let early = (!matches!(wrapped, Wrapped::Held { .. })).then(|| ready());
        Box::pin(async move {
            wrapped.before().await;
            let answer = early.unwrap_or_else(|| ready());
            next_turn().await;
            let written = match &answer {
                Ok(Readiness::Ready) => json!(true),
                // A window that said no more than "not yet": the JavaScript fake answered `false`.
                Ok(Readiness::Held(Held::Unsaid)) => json!(false),
                Ok(Readiness::Held(held)) => json!(held.sentence()),
                Ok(Readiness::Unfit(why)) => json!({ "unfit": why }),
                Err(error) => json!({ "$error": { "message": error } }),
            };
            self.fake.recorder.answered(at, written);
            wrapped.after().await;
            answer
        })
    }

    fn deliver<'a>(
        &'a self,
        host: &'a dyn PaneHost,
        pane: &'a Pane,
        text: &'a str,
    ) -> Work<'a, Result<Admission, String>> {
        self.fake.cleared.borrow_mut().push(host.cleared());
        let args = json!([{
            "launch": self.bag(),
            "pane": { "id": pane.id, "generation": pane.generation },
            "text": text,
        }]);
        let at = self.call("deliver", args.clone());
        let wrapped = self.fake.deliver_holds.wrap(&args);
        let fake = Rc::clone(&self.fake);
        let (launch, given) = (self.launch.clone(), text.to_owned());
        let taking: Taking = Box::new(move || fake.take(&launch, &given));
        let own = self.fake.deliver.borrow().clone();
        // What a delivery does is done when it is called, as the JavaScript
        // fake's was, or once the test lets a held one go; a test's own
        // `deliver` does what it does in its place. Its hand-over, after any
        // wait the test holds it in, as every adapter's after its own waits
        // (Claude's keys that clear its input box, a native send's claim), goes
        // only if the host admits it then.
        let deliver = move || -> Work<'a, Result<Admission, String>> {
            if !host.admits() {
                return Box::pin(async {
                    Ok(Admission::Refused {
                        reason: NOT_ADMITTED.to_owned(),
                    })
                });
            }
            match own {
                Some(own) => own(taking),
                None => {
                    let admission = taking();
                    Box::pin(async move { Ok(admission) })
                }
            }
        };
        // `Err` is a delivery not made yet.
        let made = if matches!(wrapped, Wrapped::Held { .. }) {
            Err(deliver)
        } else {
            Ok(deliver())
        };
        Box::pin(async move {
            wrapped.before().await;
            let outcome = match made {
                Ok(now) => now.await,
                Err(later) => later().await,
            };
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
            wrapped.after().await;
            outcome
        })
    }

    fn observe(&self) -> Work<'_, Result<Observed, String>> {
        // JavaScript handed the fake the participant's conversation, which
        // this window follows: its session.
        let session = self.session.borrow().clone();
        let args = json!([{
            "launch": self.bag(),
            "conversation": { "nativeSession": session },
        }]);
        let at = self.call("observe", args.clone());
        let wrapped = self.fake.observe_holds.wrap(&args);
        // Made when called, or once the test lets a held look go.
        let early = (!matches!(wrapped, Wrapped::Held { .. })).then(|| self.look(&session));
        Box::pin(async move {
            // A look the test made fail fails once it is let go, as the
            // wrapper's throw did.
            if let Some(reason) = wrapped.before().await {
                return Err(reason);
            }
            let observed = early.unwrap_or_else(|| self.look(&session));
            self.fake.recorder.answered(at, observed_json(&observed));
            wrapped.after().await;
            Ok(observed)
        })
    }

    /// Not written down: the JavaScript fake was never asked.
    fn ready_while_drawing(&self) -> bool {
        self.fake.ready_while_drawing.get()
    }

    /// Not written down: the JavaScript fake was never asked, so the Node
    /// traces have no such call.
    fn took(&self) -> bool {
        self.fake.of_launch(&self.launch, |agent| agent.took)
    }

    /// Not written down, as `took` is not.
    fn held(&self) -> bool {
        self.fake.of_launch(&self.launch, |agent| agent.held)
    }

    /// Counted on the agent, and not written down: the JavaScript fake was
    /// never told, so the Node traces have no such call.
    fn interrupted(&self) {
        self.fake.of_launch(&self.launch, |agent| {
            agent.interrupted += 1;
            if agent.stops_when_interrupted {
                agent.settled = true;
                agent.took_back = agent.takes_back;
            }
            if agent.busy_when_interrupted {
                agent.over = true;
            }
        });
    }
}

/// What a look at `agent`'s window finds, its launch on `session`.
fn looked(agent: &FakeAgent, session: &str) -> Observed {
    let reading = |items: Vec<Item>| {
        if agent.unread {
            return Some(Arc::new(Reading::Unknown(
                "the transcript could not be read".to_owned(),
            )));
        }
        Some(Arc::new(Reading::Known(Record {
            items,
            in_flight: false,
            asking: false,
            failed: false,
            failure: agent.failure.clone(),
            quota: None,
            settlement: Settlement::Unknown,
            pending: agent.pending,
            monitors: agent.monitors.clone(),
        })))
    };
    // A window that shows another conversation than its launch's: that
    // record's last look, and which session it shows now.
    if let Some(shows) = agent.shows.as_ref().filter(|shows| *shows != session) {
        return Observed {
            reading: reading(agent.records.get(session).cloned().unwrap_or_default()),
            settled: false,
            over: false,
            waiting: None,
            failed: agent.failed,
            quota: agent.quota.clone(),
            switched: Some(shows.clone()),
            unnamed: false,
            took_back: false,
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
        over: agent.over,
        waiting,
        failed: agent.failed,
        quota: agent.quota.clone(),
        switched: None,
        unnamed: agent.unnamed.is_some(),
        took_back: agent.took_back,
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
    if observed.took_back {
        written["tookBack"] = json!(true);
    }
    written
}
