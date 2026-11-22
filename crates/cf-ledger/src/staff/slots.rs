//! The staff as slots: a role at a tier holds one member, and the image designer's one slot
//! has no tier (`architecture/staff-slots.md`). One check, run inside the write it guards:
//! the tier is offered to each role the write gives a member (`tier-not-offered`), and the
//! slot is free (`slot-taken`). A member here is a row that has not left, is no session and
//! is no backup; a backup holds no slot.

use rusqlite::params;

use crate::model::LedgerError;
use crate::roles::{self, Role};
use crate::store::Store;

/// The members that hold `role` at `tier` (the image designer's slot: any tier), in join
/// order, but `except`: id and handle.
pub(crate) fn holders(
    store: &Store,
    project_id: i64,
    role: &str,
    tier: Option<&str>,
    except: Option<i64>,
) -> Result<Vec<(i64, String)>, LedgerError> {
    let found = store
        .db
        .prepare(
            "SELECT p.id, p.handle FROM participant p
              WHERE p.project_id = ?1 AND p.left_at IS NULL AND p.member_id IS NULL
                AND p.backup_of IS NULL AND p.id IS NOT ?2
                AND (?3 IS NULL OR p.tier = ?3)
                AND EXISTS (SELECT 1 FROM json_each(p.roles) r WHERE r.value = ?4)
              ORDER BY p.id",
        )?
        .query_map(params![project_id, except, tier, role], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(found)
}

/// No other member holds a slot that `held` at `tier` takes: each role at the tier, and the
/// image designer's slot at any tier. `except` is the member itself.
pub(crate) fn require_free<S: AsRef<str>>(
    store: &Store,
    project_id: i64,
    except: Option<i64>,
    held: &[S],
    tier: &str,
) -> Result<(), LedgerError> {
    #[cfg(any(test, feature = "test-support"))]
    if overflow::allowed() {
        return Ok(());
    }
    for role in held.iter().filter_map(|role| roles::find(role.as_ref())) {
        let at = role.tiered().then_some(tier);
        if let Some((_, handle)) = holders(store, project_id, role.id, at, except)?.first() {
            return Err(taken(role, tier, handle));
        }
    }
    Ok(())
}

fn taken(role: &Role, tier: &str, handle: &str) -> LedgerError {
    if role.tiered() {
        LedgerError::refused_with(
            "slot-taken",
            format!(
                "the {tier} {} slot is @{handle}'s: a role and tier holds one member. Pick another tier",
                role.name
            ),
            409,
        )
    } else {
        LedgerError::refused_with(
            "slot-taken",
            format!("the {} slot is @{handle}'s: a project has one", role.name),
            409,
        )
    }
}

/// New project's list, checked whole: each entry's tier is offered to each of its roles, and
/// no slot is named twice. `entries` are each member's name (what it runs on, for the words),
/// roles and tier.
pub(crate) fn check_list(
    entries: &[(String, Vec<&'static str>, String)],
) -> Result<(), LedgerError> {
    #[cfg(any(test, feature = "test-support"))]
    if overflow::allowed() {
        return Ok(());
    }
    for (index, (name, held, tier)) in entries.iter().enumerate() {
        for role in held.iter().filter_map(|role| roles::find(role)) {
            let twin = entries[..index].iter().find(|(_, other, other_tier)| {
                other.contains(&role.id) && (!role.tiered() || other_tier == tier)
            });
            if let Some((first, _, _)) = twin {
                let what = if role.tiered() {
                    format!("two {tier} {}s", role.name)
                } else {
                    format!("two {}s", role.name)
                };
                let why = if role.tiered() {
                    "a role and tier holds one member"
                } else {
                    "a project has one"
                };
                return Err(LedgerError::refused_with(
                    "slot-taken",
                    format!("the staff names {what}, {first} and {name}: {why}"),
                    400,
                ));
            }
        }
    }
    Ok(())
}

/// The kit's way to seat several members in one slot, as the update leaves a slot: the list of
/// a new project is not refused for it while `run` runs on this thread.
#[cfg(any(test, feature = "test-support"))]
pub(crate) mod overflow {
    use std::cell::Cell;

    thread_local! {
        static ALLOWED: Cell<bool> = const { Cell::new(false) };
    }

    pub(crate) fn allowed() -> bool {
        ALLOWED.with(Cell::get)
    }

    /// The rule off (or back on) on this thread: what it was before.
    pub(crate) fn set(value: bool) -> bool {
        ALLOWED.with(|cell| cell.replace(value))
    }

    pub(crate) fn allowing<T>(run: impl FnOnce() -> T) -> T {
        let before = ALLOWED.with(|cell| cell.replace(true));
        let result = run();
        ALLOWED.with(|cell| cell.set(before));
        result
    }
}
