//! The adapters a test's engine is made with, and the records it looks at with
//! no window open: the fake adapter or adapters ([`FakeAdapter`]) under the
//! harnesses the test names, as `adapters` and `adapter.record` are in Node's
//! dispatcher suite.

use std::rc::Rc;
use std::sync::Arc;

use cf_harness::contract::{Adapter, Records, Work};
use cf_harness::records::{Options, Reading, Record, Settlement};
use cf_proto::agents::Harness;
use serde_json::json;

use super::adapter::{Asked, FakeAdapter};
use super::holds::Wrapped;
use crate::seams::Adapters;

/// The adapters the test's engine is made with: the fake under every
/// harness the test names, as `{ 'claude-code': adapter, opencode: adapter }`,
/// and a fake of its own under a harness the test gives one (`{ codex }`).
pub struct FakeAdapters {
    fakes: Vec<(String, Rc<FakeAdapter>)>,
}

impl FakeAdapters {
    pub fn new(fake: &Rc<FakeAdapter>, harnesses: &[&str]) -> Self {
        Self {
            fakes: harnesses
                .iter()
                .map(|harness| ((*harness).to_owned(), Rc::clone(fake)))
                .collect(),
        }
    }

    /// `fake` answers for `harness` too.
    #[must_use]
    pub fn with(mut self, harness: &str, fake: &Rc<FakeAdapter>) -> Self {
        self.fakes.push((harness.to_owned(), Rc::clone(fake)));
        self
    }

    /// The fake that answers for `harness`, if the test names it.
    fn fake(&self, harness: &str) -> Option<&Rc<FakeAdapter>> {
        self.fakes
            .iter()
            .find(|(named, _)| named == harness)
            .map(|(_, fake)| fake)
    }
}

impl Adapters for FakeAdapters {
    fn adapter(&self, harness: &str) -> Option<Rc<dyn Adapter>> {
        self.fake(harness)
            .map(|fake| Rc::new(Asked::new(harness, Rc::clone(fake))) as Rc<dyn Adapter>)
    }
}

/// The records as the fake's agents write them: the engine's look at a
/// conversation with no window (`adapter.record` in JavaScript), by the
/// adapter of the conversation's harness.
pub struct FakeRecords {
    adapters: Rc<FakeAdapters>,
}

impl FakeRecords {
    pub fn new(adapters: Rc<FakeAdapters>) -> Self {
        Self { adapters }
    }
}

/// What the record of `session` holds: the latest window's on it, its turn
/// in flight where the agent was not answered.
fn record_of(fake: &FakeAdapter, session: &str) -> Reading {
    match fake.record_of(session) {
        None => Reading::Unknown("unknown".to_owned()),
        Some((items, settled)) => Reading::Known(Record {
            items,
            in_flight: !settled,
            asking: false,
            failed: false,
            quota: None,
            settlement: if settled {
                Settlement::Settled
            } else {
                Settlement::InFlight
            },
            pending: Default::default(),
            monitors: Vec::new(),
        }),
    }
}

impl Records for FakeRecords {
    fn look<'a>(
        &'a self,
        harness: Harness,
        session: &'a str,
        _options: &'a Options,
    ) -> Work<'a, Arc<Reading>> {
        let seam = format!("adapter:{}", harness.kind());
        let Some(fake) = self.adapters.fake(harness.kind()) else {
            return Box::pin(async { Arc::new(Reading::Unknown("unknown".to_owned())) });
        };
        let at = fake.recorder.call(
            &seam,
            Some("record"),
            json!([{ "conversation": { "nativeSession": session } }]),
        );
        let wrapped = fake.record_holds.wrap(&json!([session]));
        // Made when called, or once the test lets a held look go.
        let early = (!matches!(wrapped, Wrapped::Held { .. })).then(|| record_of(fake, session));
        Box::pin(async move {
            wrapped.before().await;
            let reading = early.unwrap_or_else(|| record_of(fake, session));
            let written = match &reading {
                Reading::Unknown(_) => json!({ "unknown": true }),
                Reading::Known(record) => json!({ "items": record.items }),
            };
            fake.recorder.answered(at, written);
            wrapped.after().await;
            Arc::new(reading)
        })
    }

    fn has_transcript<'a>(
        &'a self,
        harness: Harness,
        session: &'a str,
    ) -> Work<'a, Result<bool, String>> {
        let known = self
            .adapters
            .fake(harness.kind())
            .is_some_and(|fake| !matches!(record_of(fake, session), Reading::Unknown(_)));
        Box::pin(async move { Ok(known) })
    }

    /// Whether the agent of `session` was never answered, and none where the
    /// fake has no record of it. Asked where Node's dispatcher asked nothing, so
    /// it is no call of the adapter in a trace. A read the test holds
    /// (`record_holds`, told `[{ conversation: { nativeSession } }, "cut"]`)
    /// answers once it is let go.
    fn turn_cut<'a>(&'a self, harness: Harness, session: &'a str) -> Work<'a, Option<bool>> {
        let Some(fake) = self.adapters.fake(harness.kind()) else {
            return Box::pin(async { None });
        };
        let wrapped = fake.cut_holds.wrap(&json!([session]));
        // Read when asked, or once the test lets a held ask go.
        let early = (!matches!(wrapped, Wrapped::Held { .. })).then(|| fake.cut_of(session));
        Box::pin(async move {
            wrapped.before().await;
            let cut = early.unwrap_or_else(|| fake.cut_of(session));
            wrapped.after().await;
            cut
        })
    }
}
