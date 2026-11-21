//! The staff as slots: a role at a tier holds one member, and the image designer's one slot
//! has no tier (`architecture/staff-slots.md`). One check, run inside the write it guards:
//! the tier is offered to each role the write gives a member (`tier-not-offered`), and the
//! slot is free (`slot-taken`). A member here is a row that has not left, is no session and
//! is no backup; a backup holds no slot.

use rusqlite::params;

use crate::model::LedgerError;
use crate::queue::{send, Sent};
use crate::roles::{self, Role};
use crate::store::Store;

/// Every role of `held` is offered at `tier`: a worker's tiers are complex, standard and
/// light, since critical work is not coding. The image designer, which has no tier, offers
/// whatever the member is stored at.
pub(crate) fn require_offered<S: AsRef<str>>(held: &[S], tier: &str) -> Result<(), LedgerError> {
    #[cfg(any(test, feature = "test-support"))]
    if overflow::allowed() {
        return Ok(());
    }
    for role in held.iter().filter_map(|role| roles::find(role.as_ref())) {
        if role.tiered() && !role.offers(tier) {
            return Err(not_offered(role, tier));
        }
    }
    Ok(())
}

fn not_offered(role: &Role, tier: &str) -> LedgerError {
    let tiers: Vec<&str> = role.tiers.to_vec();
    let list = match tiers.as_slice() {
        [rest @ .., last] => format!("{} or {last}", rest.join(", ")),
        [] => String::new(),
    };
    LedgerError::refused_with(
        "tier-not-offered",
        format!(
            "{} is {list}, never {tier}: {} Pick one of those, or add this model again as a second member",
            role.a_name(),
            if tier == "critical" {
                "critical work is not coding."
            } else {
                "that is not one of its tiers."
            }
        ),
        400,
    )
}

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
    for (_, held, tier) in entries {
        require_offered(held, tier)?;
    }
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

/// A task's tier that its role has no slot at: `tier-not-offered`, in the task's words.
pub(crate) fn not_offered_for(pool: &str, tier: &str) -> LedgerError {
    let Some(role) = roles::find(pool) else {
        return LedgerError::refused("invalid-pool", format!("a pool is not {pool}"));
    };
    let list = match role.tiers {
        [rest @ .., last] => format!("{} or {last}", rest.join(", ")),
        [] => String::new(),
    };
    let why = if tier == "critical" {
        "critical work is not coding"
    } else {
        "that is not one of its tiers"
    };
    LedgerError::refused_with(
        "tier-not-offered",
        format!("{} task is {list}: {why}", role.a_names()),
        400,
    )
}

/// What the update made of members at Critical, told to the human once: each project where
/// migration 0044's step 2 moved a member gets one note naming each moved member and the
/// Critical slots that emptied. The moves told are marked by an event naming them
/// (`member.settled.told`), so a later open sends nothing. Step 1 tells nobody.
pub(crate) fn note_settled_moves(store: &mut Store) -> Result<(), LedgerError> {
    store.write(|store| {
        let moves: Vec<(i64, i64, serde_json::Value)> = store
            .db
            .prepare(
                "SELECT e.id, e.project_id, e.data FROM event e
                  WHERE e.kind = 'member.settled' AND json_extract(e.data, '$.step') = 2
                    AND NOT EXISTS (SELECT 1 FROM event t, json_each(t.data, '$.settled') s
                                     WHERE t.kind = 'member.settled.told' AND s.value = e.id)
                  ORDER BY e.project_id, e.id",
            )?
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    serde_json::from_str(&row.get::<_, String>(2)?).unwrap_or_default(),
                ))
            })?
            .collect::<rusqlite::Result<_>>()?;
        let mut project = None;
        let mut group: Vec<(i64, serde_json::Value)> = Vec::new();
        let mut groups = Vec::new();
        for (id, project_id, data) in moves {
            if project != Some(project_id) {
                if let Some(done) = project {
                    groups.push((done, std::mem::take(&mut group)));
                }
                project = Some(project_id);
            }
            group.push((id, data));
        }
        if let Some(done) = project {
            groups.push((done, group));
        }
        for (project_id, group) in groups {
            let human: Option<String> = store
                .db
                .query_row(
                    "SELECT handle FROM participant WHERE project_id = ? AND role = 'human' AND left_at IS NULL ORDER BY id LIMIT 1",
                    [project_id],
                    |row| row.get(0),
                )
                .ok();
            let ids: Vec<i64> = group.iter().map(|(id, _)| *id).collect();
            if let Some(human) = human {
                let mut said = Vec::new();
                for (_, data) in &group {
                    let handle = data["handle"].as_str().unwrap_or_default();
                    let held: String = store
                        .db
                        .query_row(
                            "SELECT roles FROM participant WHERE project_id = ? AND handle = ?",
                            params![project_id, handle],
                            |row| row.get(0),
                        )
                        .unwrap_or_default();
                    let held: Vec<String> = serde_json::from_str(&held).unwrap_or_default();
                    let coding = held
                        .iter()
                        .filter_map(|role| roles::find(role))
                        .find(|role| !role.offers("critical"))
                        .map_or_else(|| "a coding role".to_owned(), Role::a_name);
                    let mut line = format!(
                        "The update made @{handle} complex: {coding} is never critical."
                    );
                    let emptied: Vec<&str> = data["emptied"]
                        .as_array()
                        .map(|list| list.iter().filter_map(|role| role.as_str()).collect())
                        .unwrap_or_default();
                    if !emptied.is_empty() {
                        let names: Vec<String> = emptied
                            .iter()
                            .filter_map(|role| roles::find(role))
                            .map(|role| role.name.to_owned())
                            .collect();
                        let slot = if names.len() == 1 { "slot it held is" } else { "slots it held are" };
                        let adds: Vec<String> =
                            names.iter().map(|name| format!("a critical {name}")).collect();
                        let nearest: Vec<String> = emptied
                            .iter()
                            .filter_map(|role| roles::find(role))
                            .map(|role| {
                                let work = match role.id {
                                    "reviewer" => "critical reviews go",
                                    "tester" => "critical tests go",
                                    "advisor" => "critical advice goes",
                                    "architect" => "critical designs go",
                                    "researcher" => "critical research goes",
                                    _ => "critical work goes",
                                };
                                format!("{work} to the nearest {} held", role.name)
                            })
                            .collect();
                        line.push_str(&format!(
                            " The critical {} {slot} empty now: add {} under Project settings. Until then, {}.",
                            names.join(" and "),
                            adds.join(" and "),
                            nearest.join(", and "),
                        ));
                    }
                    said.push(line);
                }
                send(
                    store,
                    project_id,
                    &Sent {
                        to: &human,
                        kind: "note",
                        body: &said.join(" "),
                        ..Sent::default()
                    },
                )?;
            }
            store.log(
                project_id,
                "member.settled.told",
                serde_json::json!({ "settled": ids }),
            )?;
        }
        Ok(())
    })
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
