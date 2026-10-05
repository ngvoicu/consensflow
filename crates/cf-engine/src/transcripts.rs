//! ConsensFlow's copy of each window's conversation
//! (`src/core/transcripts.js`): at every look, what is new in the window's
//! record goes into the ledger (the last item again, and from the first when
//! the record shrank); a window the human switched to another conversation is
//! followed there. It owns the record's count of what is copied.
//!
//! Landing C freezes what the dispatcher asks of it; landing D ports it.

use cf_harness::contract::Observed;
use cf_ledger::ParticipantView;

use crate::dispatcher::Dispatcher;
use crate::record::Record;
use crate::seams::EngineError;
use crate::windows::not_ported;

impl Dispatcher {
    /// What is new in the window's record, copied into the ledger.
    pub(crate) fn copy(
        &self,
        _participant: &ParticipantView,
        _record: &Record,
        _observed: &Observed,
    ) -> Result<(), EngineError> {
        Err(not_ported("a transcript's copy"))
    }

    /// The window shows `session` now: its conversation is followed there.
    pub(crate) fn follow(
        &self,
        _participant: &ParticipantView,
        _record: &Record,
        _session: &str,
    ) -> Result<(), EngineError> {
        Err(not_ported("a conversation followed"))
    }
}
