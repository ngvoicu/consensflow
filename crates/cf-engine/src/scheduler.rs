//! Who takes which task, and who is out of quota (`src/core/scheduler.js`):
//! each open tiered task goes to the best free member, its requester told
//! once when nobody is free; a window's refusal takes its member out until
//! the reset, its work held or given back; held work goes on, and work of a
//! member whose agent is gone goes back to the board. It owns the record's
//! quota part and the tasks whose requesters were told they wait.
//!
//! Landing C freezes what the dispatcher asks of it, with the readings that
//! are one line each; a worker ports the rest.

use std::sync::Arc;

use cf_base::time;
use cf_harness::contract::Observed;
use cf_harness::records::{Level, Quota};
use cf_ledger::{ParticipantView, ProjectView};

use crate::deliveries::Delivering;
use crate::dispatcher::Dispatcher;
use crate::record::Record;
use crate::seams::EngineError;
use crate::windows::not_ported;

/// The record's quota part.
#[derive(Debug, Default)]
pub(crate) struct QuotaPart {
    /// What the window's harness last said of its quota.
    pub(crate) reported: Option<Arc<Quota>>,
    /// The refusal this window acted on: the same one is history after.
    pub(crate) handled: Option<Arc<Quota>>,
}

impl Dispatcher {
    /// The member a session belongs to; a member or the chief is its own.
    pub(crate) fn member_of(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
    ) -> ParticipantView {
        participant
            .member_id
            .and_then(|member| project.participants.iter().find(|p| p.id == member))
            .unwrap_or(participant)
            .clone()
    }

    /// Whether a member is out of quota now.
    pub(crate) fn is_out(&self, member: &ParticipantView) -> bool {
        member
            .out_until
            .as_deref()
            .and_then(time::parse)
            .is_some_and(|until| until > self.now())
    }

    /// Whether a window's last turn ended in a refusal its member is past now.
    pub(crate) fn cut_short(&self, observed: &Observed) -> bool {
        observed.failed
            && observed
                .quota
                .as_deref()
                .is_some_and(|quota| quota.level() == Level::Exhausted)
    }

    /// Gives each open task of `project` to the best free member: whether it
    /// gave any out.
    pub(crate) fn assign_open_tasks(&self, _project: &ProjectView) -> Result<bool, EngineError> {
        Err(not_ported("the scheduler"))
    }

    /// Held tasks whose reset came go on.
    pub(crate) fn resume_held(&self) -> Result<(), EngineError> {
        Err(not_ported("the held tasks"))
    }

    /// A participant whose saved agent the human has since deleted.
    pub(crate) fn agent_gone(&self, _participant: &ParticipantView) -> bool {
        false
    }

    /// A member's work goes back to the board, its agent gone.
    pub(crate) fn without_agent(
        &self,
        _project: &ProjectView,
        _participant: &ParticipantView,
        _delivering: Option<Delivering>,
    ) -> Result<(), EngineError> {
        Err(not_ported("work of a member whose agent is gone"))
    }

    /// What a window's harness says of its quota, as of this look.
    pub(crate) fn record_quota(
        &self,
        _record: &Record,
        _owner: &ParticipantView,
        _quota: Option<Arc<Quota>>,
    ) {
    }

    /// Whether the refusal a window shows is news for it.
    pub(crate) fn refused_here(&self, _record: &Record, _owner: &ParticipantView) -> bool {
        false
    }

    /// Marks the refusal a window shows as acted on.
    pub(crate) fn handled(&self, record: &Record) {
        let mut quota = record.quota.borrow_mut();
        let reported = quota.reported.clone();
        quota.handled = reported;
    }

    /// Whether a window of a member out of quota answered after it was marked out.
    pub(crate) fn answered_since(&self, _owner: &ParticipantView, _observed: &Observed) -> bool {
        false
    }

    /// When a quota resets: the time its harness names, or an hour from now.
    pub(crate) fn reset_of(&self, _quota: Option<&Quota>) -> Result<String, EngineError> {
        Err(not_ported("a quota's reset"))
    }

    /// The work a window of a member out of quota holds, held or given back.
    pub(crate) fn hold_or_release(
        &self,
        _project: &ProjectView,
        _participant: &ParticipantView,
        _owner: &ParticipantView,
        _until: &str,
    ) -> Result<(), EngineError> {
        Err(not_ported("work held for a member out of quota"))
    }

    /// A turn a refusal cut short, its member past it now: its task goes on.
    pub(crate) fn go_on(
        &self,
        _project: &ProjectView,
        _participant: &ParticipantView,
    ) -> Result<(), EngineError> {
        Err(not_ported("a turn cut short"))
    }

    /// A deleted project's open tasks wait for nobody now.
    pub(crate) fn forget_project(&self, _project: i64) {}
}
