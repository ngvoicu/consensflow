//! What goes into a window and what comes back out (`src/core/deliveries.js`):
//! into a window, ready, `beginDelivery`, the hand-over, then the record
//! watched for the message's header to confirm it, try it again or fail it,
//! with one more Enter for a paste left unsent; at start, what was in flight
//! is settled. Out of a window, a worker's answer becomes its task's result.
//! It owns the record's delivery part.
//!
//! Landing C freezes what the dispatcher asks of it; landing D ports it.

use std::rc::Rc;

use cf_harness::contract::Observed;
use cf_ledger::{MessageView, ParticipantView, ProjectView};

use crate::dispatcher::Dispatcher;
use crate::record::Record;
use crate::seams::EngineError;
use crate::windows::not_ported;

/// The one message on its way into a window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Delivering {
    pub(crate) message: i64,
    /// Its header's start, which proves it arrived (`markerOf`).
    pub(crate) marker: String,
    /// When it was handed over, on the engine's clock.
    pub(crate) since: i64,
    /// It is a launch's first message.
    pub(crate) launch: bool,
    /// It is the chief's first message: a handoff, most often.
    pub(crate) chief: bool,
    /// The harness's own queue took it.
    pub(crate) queued: bool,
    /// Enter was pressed once more for it.
    pub(crate) entered_again: bool,
}

/// The record's delivery part.
#[derive(Debug, Default)]
pub(crate) struct DeliveryPart {
    pub(crate) delivering: Option<Delivering>,
    /// A message waits for what the human typed and has not sent.
    pub(crate) unsent: bool,
}

impl Dispatcher {
    /// Hands `message` to a window that is idle.
    pub(crate) async fn deliver(
        self: &Rc<Self>,
        _record: &Rc<Record>,
        _message: MessageView,
    ) -> Result<(), EngineError> {
        Err(not_ported("a delivery"))
    }

    /// Watches the record for what is on its way: confirmed, tried again, or failed.
    pub(crate) async fn watch_arrival(
        self: &Rc<Self>,
        _record: &Rc<Record>,
        _observed: &Observed,
    ) -> Result<(), EngineError> {
        Err(not_ported("a delivery's watch"))
    }

    /// A delivery's last look before its window shows another conversation.
    pub(crate) fn confirm_arrival(
        &self,
        _record: &Record,
        _observed: &Observed,
    ) -> Result<(), EngineError> {
        Err(not_ported("a delivery's confirmation"))
    }

    /// A delivery that failed: tried again (`retry`) or failed for good.
    pub(crate) fn settle_failure(
        &self,
        _delivering: Delivering,
        _because: &str,
        _retry: bool,
    ) -> Result<(), EngineError> {
        Err(not_ported("a failed delivery"))
    }

    /// A chief's first message given back, to wait for its next window.
    pub(crate) fn give_back(
        &self,
        _delivering: Delivering,
        _because: &str,
    ) -> Result<(), EngineError> {
        Err(not_ported("a delivery given back"))
    }

    /// Once, at start: what was on its way when the daemon stopped.
    pub(crate) async fn settle_in_flight(self: &Rc<Self>) -> Result<(), EngineError> {
        Err(not_ported("the deliveries in flight at start"))
    }

    /// A worker's answer, after its task's latest message, becomes the result.
    pub(crate) fn collect(
        &self,
        _project: &ProjectView,
        _participant: &ParticipantView,
        _observed: &Observed,
    ) -> Result<(), EngineError> {
        Err(not_ported("a result's collection"))
    }
}
