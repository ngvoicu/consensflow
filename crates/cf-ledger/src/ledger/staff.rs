//! The ledger's staff and their sessions, as its callers hold them.

use super::Ledger;
use crate::{
    staff, Candidate, HoldKind, LedgerError, MemberEdit, MemberView, NewMember, ParticipantView,
    ProjectView, RemovedMember, StaffMember,
};
use rusqlite::params;

impl Ledger {
    /// Whether this staff row is a backup rather than an independent member.
    pub fn is_backup(&self, project: i64, handle: &str) -> Result<bool, LedgerError> {
        Ok(self
            .store
            .participant_by_handle(project, handle)?
            .backup_of
            .is_some())
    }

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

    /// Why a participant is out, as the ledger kept it with its mark: none when it is not out,
    /// or its mark has no kind (a timed mark from before the kind was kept).
    pub fn hold_kind(&self, participant: i64) -> Result<Option<HoldKind>, LedgerError> {
        staff::hold_kind(&self.store, participant)
    }

    /// The generation of a member's configured harness and model.
    pub fn member_setup_generation(&self, participant: i64) -> Result<i64, LedgerError> {
        Ok(self.store.db.query_row(
            "SELECT setup_generation FROM participant WHERE id = ?",
            [participant],
            |row| row.get(0),
        )?)
    }

    /// A fresh hold increments this even when its timestamps match an older hold.
    pub fn member_hold_generation(&self, participant: i64) -> Result<i64, LedgerError> {
        Ok(self.store.db.query_row(
            "SELECT hold_generation FROM participant WHERE id = ?",
            [participant],
            |row| row.get(0),
        )?)
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

    /// Return the never-opened tasks of `unopened` sessions of a staff row on Vacation. It
    /// never sets the mark, and does nothing once the row has been brought back.
    pub fn release_unopened(
        &mut self,
        project: i64,
        member: &str,
        unopened: &[i64],
    ) -> Result<ParticipantView, LedgerError> {
        staff::release_unopened(&mut self.store, project, member, unopened)
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

    /// Mark a refusal and keep the login's metadata fingerprint with the new hold.
    pub fn mark_out_with_login(
        &mut self,
        participant: i64,
        until: &str,
        reason: &str,
        kind: HoldKind,
        revision: Option<u64>,
    ) -> Result<ParticipantView, LedgerError> {
        self.store.write(|store| {
            let old = store.participant_row(participant)?;
            let now = store.at();
            let already_out = old
                .out_until
                .as_deref()
                .and_then(cf_base::time::parse)
                .zip(cf_base::time::parse(&now))
                .is_some_and(|(end, now)| end > now);
            let previous: Option<String> = store.db.query_row(
                "SELECT hold_login_revision FROM participant WHERE id = ?",
                [participant],
                |row| row.get(0),
            )?;
            let current = revision.map(|value| value.to_string());
            let login_changed = already_out
                && staff::account_hold(store, &old)?
                && matches!(kind, HoldKind::Quota | HoldKind::Account)
                && previous
                    .as_ref()
                    .zip(current.as_ref())
                    .is_some_and(|(a, b)| a != b);
            if login_changed {
                staff::mark_back(store, participant, "login metadata changed")?;
            }
            let marked = staff::mark_out_kind(store, participant, until, reason, Some(kind))?;
            if (!already_out || login_changed)
                && matches!(kind, HoldKind::Quota | HoldKind::Account)
            {
                store.db.execute(
                    "UPDATE participant SET hold_login_revision = ?1 WHERE id = ?2",
                    params![current, participant],
                )?;
            }
            Ok(marked)
        })
    }

    /// Observe a held setup only if the hold and member still match the look begun.
    #[allow(clippy::too_many_arguments)]
    pub fn observe_hold_login(
        &mut self,
        participant: i64,
        since: Option<&str>,
        until: &str,
        harness: &str,
        model: Option<&str>,
        generation: i64,
        hold_generation: i64,
        revision: u64,
    ) -> Result<bool, LedgerError> {
        self.store.write(|store| {
            let row = store.participant_row(participant)?;
            let now = store.at();
            if row.left_at.is_some()
                || store.db.query_row(
                    "SELECT setup_generation FROM participant WHERE id = ?",
                    [participant],
                    |r| r.get::<_, i64>(0),
                )? != generation
                || store.db.query_row(
                    "SELECT hold_generation FROM participant WHERE id = ?",
                    [participant],
                    |r| r.get::<_, i64>(0),
                )? != hold_generation
                || row.harness.as_deref() != Some(harness)
                || row.model.as_deref() != model
                || row.out_since.as_deref() != since
                || row.out_until.as_deref() != Some(until)
                || cf_base::time::parse(until)
                    .zip(cf_base::time::parse(&now))
                    .is_none_or(|(end, now)| end <= now)
                || !staff::account_hold(store, &row)?
            {
                return Ok(false);
            }
            let previous: Option<String> = store.db.query_row(
                "SELECT hold_login_revision FROM participant WHERE id = ?",
                [participant],
                |r| r.get(0),
            )?;
            let revision = revision.to_string();
            if previous.as_deref().is_some_and(|old| old != revision) {
                staff::mark_back(store, participant, "login metadata changed")?;
                return Ok(true);
            }
            if previous.is_none() {
                store.db.execute(
                    "UPDATE participant SET hold_login_revision = ? WHERE id = ?",
                    params![revision, participant],
                )?;
            }
            Ok(false)
        })
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

    /// Candidates for a fresh task of a role and tier, before there is a task.
    pub fn candidates_for(
        &self,
        project_id: i64,
        role: &str,
        tier: Option<&str>,
    ) -> Result<Vec<Candidate>, LedgerError> {
        staff::candidates_for(&self.store, project_id, role, tier)
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
