//! The ledger's staff and their sessions, as its callers hold them (`index.js`, participants).

use super::Ledger;
use crate::{
    staff, Candidate, LedgerError, MemberView, NewMember, ParticipantView, ProjectView,
    RemovedMember, StaffMember, TierChange,
};

impl Ledger {
    /// A member joins the staff, or rejoins it in the roles, harness,
    /// designer flag and tier given now.
    pub fn add_member(
        &mut self,
        project_id: i64,
        member: &NewMember,
    ) -> Result<ParticipantView, LedgerError> {
        staff::add_member(&mut self.store, project_id, member)
    }

    /// Each active member's tier becomes its saved agent's now (`tier_of`
    /// answers it, or none for an agent the roster no longer has): the members that changed.
    pub fn refresh_member_tiers(
        &mut self,
        tier_of: impl FnMut(&str) -> Option<String>,
    ) -> Result<Vec<TierChange>, LedgerError> {
        staff::refresh_member_tiers(&mut self.store, tier_of)
    }

    /// A member's roles change in place; a role it gains must fit its agent.
    pub fn set_roles<S: AsRef<str>>(
        &mut self,
        project_id: i64,
        handle: &str,
        roles: &[S],
    ) -> Result<ParticipantView, LedgerError> {
        staff::set_roles(&mut self.store, project_id, handle, roles)
    }

    /// A member leaves the staff, its open tasks cancelled.
    pub fn remove_member(
        &mut self,
        project_id: i64,
        handle: &str,
    ) -> Result<RemovedMember, LedgerError> {
        staff::remove_member(&mut self.store, project_id, handle)
    }

    /// The members of the newest project that has any: the staff a new project starts from.
    pub fn last_staff(&self) -> Result<Vec<StaffMember>, LedgerError> {
        staff::last_staff(&self.store)
    }

    /// Whether a participant's window has a task in hand (queued, working or
    /// waiting, or paused, whose window stays for the resumption): what keeps
    /// a member's window open. A follow-up waiting on the board for what it
    /// needs is not in hand; it keeps the session from being given another
    /// task, which is not the window's question.
    pub fn has_task_in_hand(&self, participant_id: i64) -> Result<bool, LedgerError> {
        staff::has_task_in_hand(&self.store, participant_id)
    }

    /// The active members an open task may go to, with what the daemon ranks them by.
    pub fn candidates(&self, project_id: i64, number: i64) -> Result<Vec<Candidate>, LedgerError> {
        staff::candidates(&self.store, project_id, number)
    }

    /// The active members of one role, in join order.
    pub fn members(&self, project_id: i64, role: &str) -> Result<Vec<MemberView>, LedgerError> {
        staff::members(&self.store, project_id, Some(role))
    }

    /// The human ends a session that holds no work, `by` saying who did.
    pub fn end_session(
        &mut self,
        project_id: i64,
        handle: &str,
        by: &str,
    ) -> Result<ProjectView, LedgerError> {
        staff::end_session(&mut self.store, project_id, handle, by)
    }

    /// A member out of quota takes no work until `until`, an ISO time.
    pub fn mark_out(
        &mut self,
        participant_id: i64,
        until: &str,
        reason: &str,
    ) -> Result<ParticipantView, LedgerError> {
        staff::mark_out(&mut self.store, participant_id, until, reason)
    }

    /// A member out of quota is back before its reset; the tasks held for it go on.
    pub fn mark_back(
        &mut self,
        participant_id: i64,
        because: &str,
    ) -> Result<ParticipantView, LedgerError> {
        staff::mark_back(&mut self.store, participant_id, because)
    }
}
