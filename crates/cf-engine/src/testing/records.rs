//! The records as the fake's agents write them: the engine's look at a
//! conversation with no window (`adapter.record` in JavaScript).

use std::rc::Rc;
use std::sync::Arc;

use cf_harness::contract::{Records, Work};
use cf_harness::records::{Options, Reading, Record, Settlement};
use cf_proto::agents::Harness;
use serde_json::json;

use super::adapter::FakeAdapter;
use crate::runtime::next_turn;

pub struct FakeRecords {
    fake: Rc<FakeAdapter>,
}

impl FakeRecords {
    pub fn new(fake: Rc<FakeAdapter>) -> Self {
        Self { fake }
    }

    /// What the record of `session` holds: the latest window's on it.
    fn record_of(&self, session: &str) -> Reading {
        match self.fake.items_of(session) {
            None => Reading::Unknown("unknown".to_owned()),
            Some(items) => Reading::Known(Record {
                items,
                in_flight: false,
                asking: false,
                failed: false,
                quota: None,
                settlement: Settlement::Unknown,
            }),
        }
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
        let at = self.fake.recorder.call(
            &seam,
            Some("record"),
            json!([{ "conversation": { "nativeSession": session } }]),
        );
        let reading = self.record_of(session);
        Box::pin(async move {
            next_turn().await;
            let written = match &reading {
                Reading::Unknown(_) => json!({ "unknown": true }),
                Reading::Known(record) => json!({ "items": record.items }),
            };
            self.fake.recorder.answered(at, written);
            Arc::new(reading)
        })
    }

    fn has_transcript<'a>(
        &'a self,
        _harness: Harness,
        session: &'a str,
    ) -> Work<'a, Result<bool, String>> {
        let known = !matches!(self.record_of(session), Reading::Unknown(_));
        Box::pin(async move { Ok(known) })
    }
}
