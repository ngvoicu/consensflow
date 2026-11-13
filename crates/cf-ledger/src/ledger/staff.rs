//! The ledger's staff and their sessions, as its callers hold them.

use super::Ledger;
use crate::{
    staff, Candidate, HoldKind, LedgerError, MemberEdit, MemberView, NewMember, ParticipantView,
    ProjectView, RemovedMember, StaffMember,
};

impl Ledger {
    /// Add a dedicated backup for a member.
    pub fn add_backup(
        &mut self,
        project: i64,
        member: &str,
        backup: &NewMember,
    ) -> Result<ParticipantView, LedgerError> {
        staff::add_backup(&mut self.store, project, member, backup)
    }

    /// Replace a member's backup; the old one's work is cancelled as it leaves.
    pub fn change_backup(
        &mut self,
        project: i64,
        member: &str,
        backup: &NewMember,
    ) -> Result<ParticipantView, LedgerError> {
        staff::change_backup(&mut self.store, project, member, backup)
    }

    /// Remove a member's backup.
    pub fn remove_backup(
        &mut self,
        project: i64,
        member: &str,
    ) -> Result<ParticipantView, LedgerError> {
        staff::remove_backup(&mut self.store, project, member)
    }

    /// Put a staff row on Vacation. `unopened` identifies its sessions the caller
    /// has proved have no window or open in flight, while holding its engine step.
    pub fn send_on_vacation(
        &mut self,
        project: i64,
        member: &str,
        unopened: &[i64],
    ) -> Result<ParticipantView, LedgerError> {
        staff::send_on_vacation(&mut self.store, project, member, unopened)
    }

    /// Bring a staff row back from Vacation without changing its quota hold.
    pub fn bring_back(
        &mut self,
        project: i64,
        member: &str,
    ) -> Result<ParticipantView, LedgerError> {
        staff::return_from_vacation(&mut self.store, project, member)
    }

    /// Mark a member out for a typed reason, kept with its reset.
    pub fn mark_out_with_kind(
        &mut self,
        participant: i64,
        until: &str,
        reason: &str,
        kind: HoldKind,
    ) -> Result<ParticipantView, LedgerError> {
        staff::mark_out_kind(&mut self.store, participant, until, reason, Some(kind))
    }

    /// A member joins the staff, named by its model, or one that left on the same
    /// harness, model and effort rejoins it in the roles and tier given now.
    pub fn add_member(
        &mut self,
        project_id: i64,
        member: &NewMember,
    ) -> Result<ParticipantView, LedgerError> {
        staff::add_member(&mut self.store, project_id, member)
    }

    /// A member runs on another model, effort or tier from its next window (T-1143); so
    /// do its sessions. Its handle stays.
    pub fn edit_member(
        &mut self,
        project_id: i64,
        handle: &str,
        edit: &MemberEdit,
    ) -> Result<ParticipantView, LedgerError> {
        staff::edit_member(&mut self.store, project_id, handle, edit)
    }

    /// A member's roles change in place; a role it gains must fit it, and its tier follows
    /// them: one with tiers gained by a member given none comes with `tier`.
    pub fn set_roles<S: AsRef<str>>(
        &mut self,
        project_id: i64,
        handle: &str,
        roles: &[S],
        tier: Option<&str>,
    ) -> Result<ParticipantView, LedgerError> {
        staff::set_roles(&mut self.store, project_id, handle, roles, tier)
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
