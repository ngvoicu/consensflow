//! A plan and several decisions: each is one ledger transaction.

use cf_proto::ledger::TaskView;
use rusqlite::{params, OptionalExtension};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};

use super::{accept_failed_task, accept_task, cancel_task, create_task, NewTask};
use crate::model::LedgerError;
use crate::store::Store;

/// One named section and the references to other sections. Numeric references
/// stay on `task`, where the ordinary task creation checks them.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PlanTask {
    pub name: String,
    pub task: NewTask,
    pub needs: Vec<String>,
    pub before: Vec<String>,
}

/// One section's answer: its name and task number, the tier it went to (its effective tier,
/// kept with the key, so `cf` never calls a Complex request that fell to Standard Complex),
/// and, where it fell, the tier asked and the members of the tier it went to. An answer an
/// earlier version kept has the name and the number alone.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanSection {
    pub name: String,
    pub number: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tier: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asked: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub held_by: Vec<String>,
}

/// A saved answer, in the form it was kept: `[name, number]` before the slots, an object since.
#[derive(serde::Deserialize)]
#[serde(untagged)]
enum SavedSection {
    Old((String, i64)),
    #[serde(rename_all = "camelCase")]
    New {
        name: String,
        number: i64,
        tier: Option<String>,
        asked: Option<String>,
        #[serde(default)]
        held_by: Vec<String>,
    },
}

impl From<SavedSection> for PlanSection {
    fn from(saved: SavedSection) -> Self {
        match saved {
            SavedSection::Old((name, number)) => Self {
                name,
                number,
                tier: None,
                asked: None,
                held_by: Vec::new(),
            },
            SavedSection::New {
                name,
                number,
                tier,
                asked,
                held_by,
            } => Self {
                name,
                number,
                tier,
                asked,
                held_by,
            },
        }
    }
}

/// The sections in the order the plan gave them, and whether it was a retry.
#[derive(Debug)]
pub struct PlanCreated {
    pub tasks: Vec<PlanSection>,
    pub repeated: bool,
}

fn section(name: &str, error: LedgerError) -> LedgerError {
    match error {
        LedgerError::Refused(mut refusal) => {
            refusal.message = format!("section {name}: {}", refusal.message);
            LedgerError::Refused(refusal)
        }
        other => other,
    }
}

/// The ledger's fingerprint of a plan: its sections as read and the file's own.
fn plan_fingerprint(tasks: &[PlanTask], fingerprint: &str) -> Result<String, LedgerError> {
    let encoded = serde_json::to_vec(&(tasks, fingerprint))?;
    Ok(format!("{:x}", Sha256::digest(encoded)))
}

/// The answer saved under `key`, replayed for the same plan; a different plan under it is
/// `key-used`; none where the key is new.
fn saved_plan(
    store: &Store,
    project: i64,
    key: &str,
    fingerprint: &str,
) -> Result<Option<PlanCreated>, LedgerError> {
    let existing: Option<(String, String)> = store
        .db
        .query_row(
            "SELECT fingerprint, tasks FROM task_plan_key WHERE project_id = ? AND key = ?",
            params![project, key],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let Some((saved, answer)) = existing else {
        return Ok(None);
    };
    if saved != fingerprint {
        return Err(LedgerError::refused_with(
            "key-used",
            format!("key {key} was already used for a different plan"),
            409,
        ));
    }
    let saved: Vec<SavedSection> = serde_json::from_str(&answer)?;
    Ok(Some(PlanCreated {
        tasks: saved.into_iter().map(PlanSection::from).collect(),
        repeated: true,
    }))
}

/// The answer a plan's key holds, looked up before any rule of now is checked: the same plan
/// replays it, a different one is `key-used`, a new key is none.
pub(crate) fn replayed_plan(
    store: &Store,
    project: i64,
    tasks: &[PlanTask],
    key: &str,
    fingerprint: &str,
) -> Result<Option<PlanCreated>, LedgerError> {
    saved_plan(store, project, key, &plan_fingerprint(tasks, fingerprint)?)
}

/// The same single-task operations, nested in one outer write. The key and its
/// answer commit with the tasks: a retry cannot see a partial plan.
pub(crate) fn create_plan(
    store: &mut Store,
    project: i64,
    tasks: &[PlanTask],
    key: Option<&str>,
    fingerprint: &str,
) -> Result<PlanCreated, LedgerError> {
    let fingerprint = plan_fingerprint(tasks, fingerprint)?;
    store.write_batch(|store| {
        if let Some(key) = key {
            if key.is_empty() || key.chars().any(|c| c.is_whitespace() || c.is_control() || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')) || key.len() > 256 {
                return Err(LedgerError::refused("invalid-key", "--key takes one word of at most 256 bytes"));
            }
            if let Some(saved) = saved_plan(store, project, key, &fingerprint)? {
                return Ok(saved);
            }
        }
        if tasks.is_empty() {
            return Err(LedgerError::refused("empty-plan", "the plan has no sections"));
        }
        let mut names = HashMap::new();
        for (index, task) in tasks.iter().enumerate() {
            if task.name.is_empty() || task.name.chars().any(|c| c.is_whitespace() || c.is_control() || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')) {
                return Err(LedgerError::refused("invalid-name", format!("section {}: a name is one word", task.name)));
            }
            if names.insert(task.name.as_str(), index).is_some() {
                return Err(LedgerError::refused("duplicate-name", format!("section {}: this name is used twice", task.name)));
            }
        }
        let mut needs = vec![Vec::<usize>::new(); tasks.len()];
        for (index, task) in tasks.iter().enumerate() {
            // A T-number means a task already on the board, never a number
            // accidentally assigned to an earlier section of this same plan.
            for number in task.task.needs.iter().chain(&task.task.before) {
                let number = i64::try_from(*number).map_err(|_| LedgerError::refused("invalid-needs", "not a task number"))?;
                store.task_row(project, number).map_err(|error| section(&task.name, error))?;
            }
            for (references, reverse) in [(&task.needs, false), (&task.before, true)] {
                for name in references {
                    let Some(&other) = names.get(name.as_str()) else {
                        return Err(LedgerError::refused("unknown-name", format!("section {}: no section named {name}", task.name)));
                    };
                    let (waiter, needed) = if reverse { (other, index) } else { (index, other) };
                    if !needs[waiter].contains(&needed) {
                        needs[waiter].push(needed);
                    }
                }
            }
        }
        let mut made: Vec<Option<PlanSection>> = vec![None; tasks.len()];
        for _ in 0..tasks.len() {
            let Some(index) = (0..tasks.len()).find(|&index| made[index].is_none() && needs[index].iter().all(|&need| made[need].is_some())) else {
                let index = made.iter().position(Option::is_none).unwrap_or_default();
                return Err(LedgerError::refused("circular-needs", format!("section {}: a plan has no circles of needs", tasks[index].name)));
            };
            let mut request = tasks[index].task.clone();
            for &need in &needs[index] {
                let number = made[need].as_ref().map(|section| section.number).ok_or_else(|| LedgerError::refused("circular-needs", "a plan has no circles of needs"))?;
                let number = u64::try_from(number).map_err(|_| LedgerError::refused("invalid-needs", "not a task number"))?;
                if !request.needs.contains(&number) { request.needs.push(number); }
            }
            let created = create_task(store, project, &request).map_err(|error| section(&tasks[index].name, error))?;
            let held_by = match (&created.asked, created.task.pool.as_deref(), created.task.tier.as_deref()) {
                (Some(_), Some(pool), Some(tier)) => crate::staff::holders_of_tier(store, project, pool, tier)?,
                _ => Vec::new(),
            };
            made[index] = Some(PlanSection {
                name: tasks[index].name.clone(),
                number: created.task.number,
                tier: created.task.tier.clone(),
                asked: created.asked.clone(),
                held_by,
            });
        }
        let answer: Vec<PlanSection> = made.into_iter().map(|section| section.ok_or_else(|| LedgerError::refused("circular-needs", "a plan has no circles of needs"))).collect::<Result<_, _>>()?;
        if let Some(key) = key {
            store.db.execute(
                "INSERT INTO task_plan_key (project_id, key, fingerprint, tasks) VALUES (?, ?, ?, ?)",
                params![project, key, fingerprint, serde_json::to_string(&answer)?],
            )?;
        }
        Ok(PlanCreated { tasks: answer, repeated: false })
    })
}

/// Accept or cancel each number with the existing operation and commit only if
/// every one succeeds. Failed tasks use the same checked words as one accept.
pub(crate) fn decide_tasks(
    store: &mut Store,
    project: i64,
    numbers: &[i64],
    by: &str,
    action: &str,
    words: &str,
) -> Result<Vec<TaskView>, LedgerError> {
    store.write_batch(|store| {
        if numbers.is_empty() {
            return Err(LedgerError::refused("no-tasks", "name at least one task"));
        }
        let mut unique = HashSet::new();
        if let Some(number) = numbers.iter().find(|number| !unique.insert(**number)) {
            return Err(LedgerError::refused(
                "duplicate-task",
                format!("T-{number} is named twice"),
            ));
        }
        // A task that waits for another can be cancelled in the same batch.
        // Cancel those waiters first, whichever order the chief named them in;
        // a waiter outside the batch still causes the ordinary refusal.
        let mut order: Vec<usize> = (0..numbers.len()).collect();
        if action == "cancel" {
            let waits = numbers
                .iter()
                .map(|&number| {
                    let task = store.task_row(project, number)?;
                    super::finishing::waiters(store, &task)
                })
                .collect::<Result<Vec<_>, _>>()?;
            let mut left = order;
            order = Vec::with_capacity(left.len());
            while !left.is_empty() {
                let next = left
                    .iter()
                    .position(|&index| {
                        !waits[index]
                            .iter()
                            .any(|waiter| left.iter().any(|&other| numbers[other] == *waiter))
                    })
                    .unwrap_or(0);
                order.push(left.remove(next));
            }
        }
        let mut result = vec![None; numbers.len()];
        for index in order {
            let number = numbers[index];
            let moved = match action {
                "accept" => {
                    let task = store.task_row(project, number)?;
                    if task.state == "failed" {
                        accept_failed_task(store, project, number, by, words)
                    } else {
                        accept_task(store, project, number, by)
                    }
                }
                "cancel" => cancel_task(store, project, number, by),
                _ => Err(LedgerError::refused(
                    "invalid-action",
                    format!("cannot decide tasks with {action}"),
                )),
            }
            .map_err(|error| match error {
                LedgerError::Refused(mut refusal) => {
                    refusal.message = format!("T-{number}: {}", refusal.message);
                    LedgerError::Refused(refusal)
                }
                other => other,
            })?;
            result[index] = Some(moved);
        }
        Ok(result.into_iter().flatten().collect())
    })
}
