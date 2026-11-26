//! The kit's way to seat members in slots the rule now refuses, as the update leaves a slot
//! several hold (`architecture/staff-slots.md`, section 3).

/// Seats member `handle` at `tier` the way the update leaves a slot several hold: the row
/// is written as a ledger of the version before had it, which the rule never refused then.
/// For a kit staff whose subject is not the slot (the members stand in for one another, or
/// the test measures windows at once): the ledger now refuses to seat two in one slot.
pub fn seat_in_slot(
    ledger: &crate::Ledger,
    project: i64,
    handle: &str,
    tier: &str,
) -> rusqlite::Result<()> {
    ledger.sqlite().execute(
        "UPDATE participant SET tier = ?1
          WHERE project_id = ?2 AND (handle = ?3 OR member_id IN
                (SELECT id FROM participant WHERE project_id = ?2 AND handle = ?3))",
        rusqlite::params![tier, project, handle],
    )?;
    Ok(())
}

/// The update to the slots' number among [`crate::MIGRATIONS`].
const SLOTS: usize = 45;

impl crate::Ledger {
    /// [`crate::Ledger::add_member`] into a slot another member holds, as the update leaves a
    /// slot several hold: for a kit staff whose subject is not its slots.
    pub fn add_member_overflowing(
        &mut self,
        project: i64,
        member: &crate::NewMember,
    ) -> Result<crate::ParticipantView, crate::LedgerError> {
        crate::staff::slots::overflow::allowing(|| self.add_member(project, member))
    }

    /// [`crate::Ledger::create_project`] for a kit staff that seats several members in one
    /// slot, as the update leaves a slot several hold: the ledger refuses such a list now.
    /// For a staff whose subject is not its slots.
    pub fn create_project_overflowing(
        &mut self,
        request: &crate::NewProject,
    ) -> Result<crate::ProjectView, crate::LedgerError> {
        crate::staff::slots::overflow::allowing(|| self.create_project(request))
    }
}

/// Runs `run` with the slot rule off on this thread: for the kit's recordings of ledgers that
/// never had it (Node's traces), which seat members the rule refuses.
pub fn without_the_slot_rule<T>(run: impl FnOnce() -> T) -> T {
    crate::staff::slots::overflow::allowing(run)
}

/// The slot rule off on this thread until the guard is dropped: for a test whose work goes on
/// across `.await`s on a runtime of this one thread (the kit's replays of Node's traces).
#[must_use = "the rule is on again as soon as this is dropped"]
pub fn slot_rule_off() -> SlotRuleOff {
    SlotRuleOff(crate::staff::slots::overflow::set(true))
}

/// The rule as it was, put back on drop.
pub struct SlotRuleOff(bool);

impl Drop for SlotRuleOff {
    fn drop(&mut self) {
        crate::staff::slots::overflow::set(self.0);
    }
}

/// Opens `file` as a version without the slots opens it: the update to the slots (0045) is
/// left out, by taking the file through every earlier migration and marking that one as
/// applied; the later ones, and every line of [`crate::open_ledger`] after it, are the
/// product's. The gate of `architecture/staff-slots.md` 9.5 compares what the update leaves
/// with this.
pub fn open_ledger_without_the_slots_update(
    file: &std::path::Path,
    options: crate::Options,
) -> Result<crate::Ledger, crate::LedgerError> {
    let db = rusqlite::Connection::open(file)?;
    crate::migrate(&db, &crate::MIGRATIONS[..SLOTS - 1])?;
    db.pragma_update(None, "user_version", SLOTS as i64)?;
    drop(db);
    crate::open_ledger(file, options)
}
