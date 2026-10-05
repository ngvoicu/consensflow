//! The human's Switch chief (`src/core/chief-switch.js`): now, or after the
//! chief's turn, with a note first asking it where things stand when the
//! human wants one. It takes a last look and copy, gives back what was in
//! flight, closes the old window as the engine's own, switches the chief in
//! the ledger and launches the new one with the handoff. It also picks a
//! chief's first message, and owns the record's pending switch.
//!
//! Landing C freezes what the dispatcher asks of it; a worker ports it.

use std::rc::Rc;

use cf_harness::contract::Observed;
use cf_ledger::{ParticipantView, ProjectView};

use crate::dispatcher::Dispatcher;
use crate::record::Record;
use crate::seams::EngineError;
use crate::windows::not_ported;

/// The chief a switch is to: a saved agent on its harness.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwitchTo {
    pub harness: String,
    pub agent: String,
}

/// A switch waiting for the end of the chief's turn, and the note it sent
/// first, if the human wanted one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PendingSwitch {
    pub(crate) to: SwitchTo,
    pub(crate) note: Option<i64>,
}

impl Dispatcher {
    /// A switch the human asked for after the chief's turn; with `note`,
    /// the chief is asked first where things stand.
    pub(crate) fn after_turn(
        &self,
        _project: i64,
        _record: &Record,
        _to: SwitchTo,
        _note: bool,
    ) -> Result<(), EngineError> {
        Err(not_ported("a switch after the chief's turn"))
    }

    /// A switch waiting for the chief's turn goes once the turn ends.
    pub(crate) async fn await_switch(
        self: &Rc<Self>,
        _project: &ProjectView,
        _chief: &ParticipantView,
        _record: &Rc<Record>,
        _observed: &Observed,
        _idle: bool,
    ) -> Result<(), EngineError> {
        Err(not_ported("a switch waiting for the chief's turn"))
    }

    /// The switch: the old window closed, the chief switched, the new one launched.
    pub(crate) async fn perform_switch(
        self: &Rc<Self>,
        _project: &ProjectView,
        _chief: &ParticipantView,
        _record: &Rc<Record>,
        _to: SwitchTo,
    ) -> Result<(), EngineError> {
        Err(not_ported("Switch chief"))
    }
}
