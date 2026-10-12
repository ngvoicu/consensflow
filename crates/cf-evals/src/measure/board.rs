//! The board as the ledger file holds it: the project, who is in it, what the
//! chief asked for and how much of it ran side by side, what it said to the
//! human. Every number here is SQL over the tables as they are, and none of the
//! ledger's own code: the board is one of the things a run checks, so it cannot be
//! what the tasks are counted from.

use rusqlite::{Connection, OptionalExtension};
use serde_json::Value;

use super::metrics::{ProjectRef, Task};
use crate::{Error, Result};

/// A participant of the project.
#[derive(Debug, Clone)]
pub(super) struct Participant {
    pub id: i64,
    pub handle: String,
    pub harness: Option<String>,
}

/// A message of a task, as far as the run's numbers go.
#[derive(Debug)]
pub(super) struct Message {
    pub kind: String,
    pub body: String,
    pub created_at: String,
    pub delivered_at: Option<String>,
}

/// A task with the messages that belong to it.
#[derive(Debug)]
pub(super) struct TaskRow {
    pub task: Task,
    pub requester: String,
    pub updated_at: String,
    pub messages: Vec<Message>,
}

impl TaskRow {
    /// Whether the chief asked for it.
    pub fn is_the_chiefs(&self, chief: &str) -> bool {
        self.requester == chief
    }

    /// Whether it went to a pool of members of `role` (a worker's task may name
    /// none: one given to a window by name).
    pub fn pool_is(&self, pool: &str) -> bool {
        self.task.pool.as_deref() == Some(pool)
    }

    /// The first message of `kind` it has.
    fn first(&self, kind: &str) -> Option<&Message> {
        self.messages.iter().find(|message| message.kind == kind)
    }
}

/// The first project of the ledger.
pub(super) fn project(db: &Connection) -> Result<ProjectRef> {
    db.query_row("SELECT id, name FROM project ORDER BY id", [], |row| {
        Ok(ProjectRef {
            id: row.get(0)?,
            name: row.get(1)?,
        })
    })
    .optional()?
    .ok_or_else(|| Error::Refused("the eval ledger holds no project".to_owned()))
}

/// The human and the chief of the project: those who have not left.
pub(super) fn human_and_chief(db: &Connection, project: i64) -> Result<(Participant, Participant)> {
    let mut statement = db.prepare(
        "SELECT id, handle, role, harness FROM participant
         WHERE project_id = ?1 AND left_at IS NULL ORDER BY id",
    )?;
    let found = statement
        .query_map([project], |row| {
            Ok((
                row.get::<_, String>(2)?,
                Participant {
                    id: row.get(0)?,
                    handle: row.get(1)?,
                    harness: row.get(3)?,
                },
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let find = |role: &str| {
        found
            .iter()
            .find(|(has, _)| has == role)
            .map(|(_, participant)| participant.clone())
            .ok_or_else(|| Error::Refused(format!("the eval ledger holds no {role}")))
    };
    Ok((find("human")?, find("chief")?))
}

/// Every task of the project, by number, with its messages.
pub(super) fn tasks(db: &Connection, project: i64) -> Result<Vec<TaskRow>> {
    let mut statement = db.prepare(
        "SELECT t.id, t.number, t.title, t.pool, t.tier, t.state, t.updated_at,
                q.handle AS requester, a.handle AS assignee
         FROM task t
         JOIN participant q ON q.id = t.requester_id
         LEFT JOIN participant a ON a.id = t.assignee_id
         WHERE t.project_id = ?1 ORDER BY t.number",
    )?;
    let rows = statement
        .query_map([project], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                TaskRow {
                    task: Task {
                        number: row.get(1)?,
                        title: row.get(2)?,
                        pool: row.get(3)?,
                        tier: row.get(4)?,
                        state: row.get(5)?,
                        assignee: row.get(8)?,
                    },
                    updated_at: row.get(6)?,
                    requester: row.get(7)?,
                    messages: Vec::new(),
                },
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let mut messages = db.prepare(
        "SELECT kind, body, created_at, delivered_at FROM message WHERE task_id = ?1 ORDER BY id",
    )?;
    rows.into_iter()
        .map(|(id, mut row)| {
            row.messages = messages
                .query_map([id], |message| {
                    Ok(Message {
                        kind: message.get(0)?,
                        body: message.get(1)?,
                        created_at: message.get(2)?,
                        delivered_at: message.get(3)?,
                    })
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            Ok(row)
        })
        .collect()
}

/// A message the chief sent the human: its id, its kind and its words.
#[derive(Debug)]
pub(super) struct ToHuman {
    pub id: i64,
    pub kind: String,
    pub body: String,
}

/// What the chief sent the human, in order. The human's inbox is its newest 500
/// messages that are not held at the gate; a report reads those the chief sent.
pub(super) fn to_human(db: &Connection, human: i64, chief: &str) -> Result<Vec<ToHuman>> {
    let mut statement = db.prepare(
        "SELECT m.id, m.kind, m.body, s.handle AS sender
         FROM (SELECT * FROM message WHERE recipient_id = ?1 AND state != 'gated'
               ORDER BY id DESC LIMIT 500) m
         LEFT JOIN participant s ON s.id = m.sender_id ORDER BY m.id",
    )?;
    let sent = statement
        .query_map([human], |row| {
            Ok((
                ToHuman {
                    id: row.get(0)?,
                    kind: row.get(1)?,
                    body: row.get(2)?,
                },
                row.get::<_, Option<String>>(3)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(sent
        .into_iter()
        .filter(|(_, sender)| sender.as_deref() == Some(chief))
        .map(|(message, _)| message)
        .collect())
}

/// The data of each event of `kind` the project logged, in order.
pub(super) fn events_of(db: &Connection, project: i64, kind: &str) -> Result<Vec<Value>> {
    let mut statement =
        db.prepare("SELECT data FROM event WHERE project_id = ?1 AND kind = ?2 ORDER BY id")?;
    let data = statement
        .query_map(rusqlite::params![project, kind], |row| {
            row.get::<_, String>(0)
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    data.iter()
        .map(|text| {
            serde_json::from_str(text).map_err(|cause| {
                Error::Refused(format!(
                    "an event of the ledger holds no JSON ({cause}): {text}"
                ))
            })
        })
        .collect()
}

/// The harnesses the chief ran on, in order: each switch ends a conversation and
/// starts one, and the chief's own is the last.
pub(super) fn chiefs(db: &Connection, chief: &Participant) -> Result<Vec<Option<String>>> {
    let mut statement = db.prepare(
        "SELECT harness FROM conversation WHERE participant_id = ?1 AND ended_at IS NOT NULL ORDER BY id",
    )?;
    let mut ran = statement
        .query_map([chief.id], |row| row.get::<_, String>(0))?
        .map(|harness| harness.map(Some))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    ran.push(chief.harness.clone());
    Ok(ran)
}

/// States a task ends in without a result.
fn ended(state: &str) -> bool {
    matches!(state, "cancelled" | "failed")
}

/// The most tasks whose windows had the brief and no result yet at one moment.
/// A window is at work from its brief's delivery to its result, or to the task's
/// end when it ended without one; a task whose brief never reached a window
/// (open, queued, withdrawn) never ran.
pub(super) fn most_at_once(tasks: &[TaskRow], chief: &str) -> i64 {
    // `(when, change)`: a window begins (+1) or ends (-1).
    let mut points: Vec<(&str, i64)> = Vec::new();
    for row in tasks.iter().filter(|row| row.is_the_chiefs(chief)) {
        let Some(from) = row
            .first("task")
            .and_then(|brief| brief.delivered_at.as_deref())
        else {
            continue;
        };
        let to = row
            .first("result")
            .map(|result| result.created_at.as_str())
            .or_else(|| ended(&row.task.state).then_some(row.updated_at.as_str()))
            // Still at work when the ledger was read.
            .unwrap_or("9999");
        points.push((from, 1));
        points.push((to, -1));
    }
    // At one moment a window that ends is counted before one that begins.
    points.sort_by(|a, b| a.0.cmp(b.0).then(a.1.cmp(&b.1)));
    let (mut open, mut most) = (0, 0);
    for (_, change) in points {
        open += change;
        most = most.max(open);
    }
    most
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(state: &str, brief: Option<&str>, result: Option<&str>, ends: &str) -> TaskRow {
        let mut messages = Vec::new();
        if let Some(delivered) = brief {
            messages.push(Message {
                kind: "task".to_owned(),
                body: String::new(),
                created_at: "2026-10-08T10:00:00.000Z".to_owned(),
                delivered_at: Some(delivered.to_owned()),
            });
        }
        if let Some(created) = result {
            messages.push(Message {
                kind: "result".to_owned(),
                body: String::new(),
                created_at: created.to_owned(),
                delivered_at: None,
            });
        }
        TaskRow {
            task: Task {
                state: state.to_owned(),
                ..Task::default()
            },
            requester: "chief".to_owned(),
            updated_at: ends.to_owned(),
            messages,
        }
    }

    #[test]
    fn two_windows_at_work_at_once_are_two_and_a_task_that_never_had_its_brief_delivered_is_none() {
        let tasks = [
            row(
                "done",
                Some("2026-10-08T10:10:00Z"),
                Some("2026-10-08T10:20:00Z"),
                "x",
            ),
            row(
                "done",
                Some("2026-10-08T10:02:00Z"),
                Some("2026-10-08T10:15:00Z"),
                "x",
            ),
            // Queued, brief never delivered: it never ran.
            row("queued", None, None, "x"),
        ];
        assert_eq!(most_at_once(&tasks, "chief"), 2);
        assert_eq!(most_at_once(&tasks[2..], "chief"), 0);
        assert_eq!(most_at_once(&[], "chief"), 0);
    }

    #[test]
    fn a_window_that_ends_at_the_moment_another_begins_does_not_overlap_it() {
        let tasks = [
            row(
                "done",
                Some("2026-10-08T10:00:00Z"),
                Some("2026-10-08T10:05:00Z"),
                "x",
            ),
            row(
                "done",
                Some("2026-10-08T10:05:00Z"),
                Some("2026-10-08T10:09:00Z"),
                "x",
            ),
        ];
        assert_eq!(most_at_once(&tasks, "chief"), 1);
    }

    #[test]
    fn a_task_that_ended_without_a_result_ends_where_it_ended_and_one_still_going_never_does() {
        let cancelled = row(
            "cancelled",
            Some("2026-10-08T10:00:00Z"),
            None,
            "2026-10-08T10:03:00Z",
        );
        let later = row(
            "done",
            Some("2026-10-08T10:04:00Z"),
            Some("2026-10-08T10:06:00Z"),
            "x",
        );
        assert_eq!(most_at_once(&[cancelled, later], "chief"), 1);
        let working = row("working", Some("2026-10-08T10:00:00Z"), None, "x");
        let after = row(
            "done",
            Some("2026-10-08T10:04:00Z"),
            Some("2026-10-08T10:06:00Z"),
            "x",
        );
        assert_eq!(most_at_once(&[working, after], "chief"), 2);
    }

    #[test]
    fn only_the_tasks_the_chief_asked_for_are_counted() {
        let mut other = row(
            "done",
            Some("2026-10-08T10:00:00Z"),
            Some("2026-10-08T10:09:00Z"),
            "x",
        );
        other.requester = "zeus".to_owned();
        let mine = row(
            "done",
            Some("2026-10-08T10:01:00Z"),
            Some("2026-10-08T10:08:00Z"),
            "x",
        );
        assert_eq!(most_at_once(&[other, mine], "chief"), 1);
    }
}
