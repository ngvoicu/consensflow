//! The ledger of a long run as one look reads it: the rows of its project, read
//! with SQLite from a copy of the file the daemon holds (`measure::on_copy`), and
//! none of the ledger's code. What the invariants are held to is what the rows
//! say: the tasks and their states, every message and when it was made, the
//! copy the daemon keeps of each window's conversation (`transcript`, item by
//! item as the harness's record had them) and the log of what happened (`event`).

use rusqlite::Connection;
use serde_json::Value;

use crate::{clock, Result};

/// A project of the ledger.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    pub id: i64,
    pub name: String,
    /// `open` or `suspended`.
    pub state: String,
}

/// Someone of the project: the human, the chief, a member, or a member's session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Participant {
    pub id: i64,
    pub handle: String,
    pub role: String,
    /// The harness's kind (`claude-code`, `devin`…): none for the human.
    pub harness: Option<String>,
    pub agent: Option<String>,
}

/// A conversation of a participant's window, as its harness names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conversation {
    pub id: i64,
    pub participant: i64,
    pub harness: String,
    /// The harness's own id of it: Claude Code's session, Devin's…
    pub session: Option<String>,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
}

/// A task of the board.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    pub id: i64,
    pub number: i64,
    pub title: String,
    pub state: String,
    pub assignee: Option<i64>,
    pub requester: i64,
    pub updated_at: String,
    /// Deleted from the board by the human: no work of the run's.
    pub deleted: bool,
}

/// A message, with the time the ledger made it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub id: i64,
    pub recipient: i64,
    /// None for ConsensFlow's own.
    pub sender: Option<i64>,
    pub kind: String,
    /// The task's row, not its number.
    pub task: Option<i64>,
    pub body: String,
    pub state: String,
    pub created_at: String,
}

/// One item of the copy of a conversation, as the harness's record had it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub conversation: i64,
    pub id: String,
    pub seq: i64,
    /// `user`, `assistant`, `tool` or `custom`.
    pub role: String,
    pub text: String,
    /// Whether the record says the item ended a turn.
    pub complete: bool,
    /// When the harness wrote it, and when the daemon copied it.
    pub at: Option<String>,
    pub copied_at: String,
}

impl Item {
    /// When it was said: the harness's own time of it, else when it was copied.
    pub fn millis(&self) -> Option<i64> {
        self.at
            .as_deref()
            .and_then(clock::millis)
            .or_else(|| clock::millis(&self.copied_at))
    }
}

/// An entry of the ledger's log.
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub id: i64,
    pub at: String,
    pub kind: String,
    pub data: Value,
}

/// The rows of the project, as one look read them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Ledger {
    pub project: Option<Project>,
    pub participants: Vec<Participant>,
    pub conversations: Vec<Conversation>,
    pub tasks: Vec<Task>,
    pub messages: Vec<Message>,
    /// In the order of their conversation and their place in it.
    pub items: Vec<Item>,
    pub events: Vec<Event>,
}

impl Ledger {
    /// The rows of the newest project of `db` (a long run's home has the one); a
    /// ledger with none is read as empty.
    pub fn read(db: &Connection) -> Result<Self> {
        let project = db
            .prepare("SELECT id, name, state FROM project ORDER BY id DESC LIMIT 1")?
            .query_map([], |row| {
                Ok(Project {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    state: row.get(2)?,
                })
            })?
            .next()
            .transpose()?;
        let Some(project) = project else {
            return Ok(Self::default());
        };
        let id = project.id;
        Ok(Self {
            participants: rows(
                db,
                "SELECT id, handle, role, harness, agent FROM participant
                 WHERE project_id = ?1 ORDER BY id",
                id,
                |row| {
                    Ok(Participant {
                        id: row.get(0)?,
                        handle: row.get(1)?,
                        role: row.get(2)?,
                        harness: row.get(3)?,
                        agent: row.get(4)?,
                    })
                },
            )?,
            conversations: rows(
                db,
                "SELECT c.id, c.participant_id, c.harness, c.native_session, c.started_at,
                        c.ended_at
                 FROM conversation c JOIN participant p ON p.id = c.participant_id
                 WHERE p.project_id = ?1 ORDER BY c.id",
                id,
                |row| {
                    Ok(Conversation {
                        id: row.get(0)?,
                        participant: row.get(1)?,
                        harness: row.get(2)?,
                        session: row.get(3)?,
                        started_at: row.get(4)?,
                        ended_at: row.get(5)?,
                    })
                },
            )?,
            tasks: rows(
                db,
                "SELECT id, number, title, state, assignee_id, requester_id, updated_at, deleted_at
                 FROM task WHERE project_id = ?1 ORDER BY number",
                id,
                |row| {
                    Ok(Task {
                        id: row.get(0)?,
                        number: row.get(1)?,
                        title: row.get(2)?,
                        state: row.get(3)?,
                        assignee: row.get(4)?,
                        requester: row.get(5)?,
                        updated_at: row.get(6)?,
                        deleted: row.get::<_, Option<String>>(7)?.is_some(),
                    })
                },
            )?,
            messages: rows(
                db,
                "SELECT id, recipient_id, sender_id, kind, task_id, body, state, created_at
                 FROM message WHERE project_id = ?1 ORDER BY id",
                id,
                |row| {
                    Ok(Message {
                        id: row.get(0)?,
                        recipient: row.get(1)?,
                        sender: row.get(2)?,
                        kind: row.get(3)?,
                        task: row.get(4)?,
                        body: row.get(5)?,
                        state: row.get(6)?,
                        created_at: row.get(7)?,
                    })
                },
            )?,
            items: rows(
                db,
                "SELECT t.conversation_id, t.item_id, t.seq, t.role, t.text, t.complete, t.at, t.copied_at
                 FROM transcript t JOIN conversation c ON c.id = t.conversation_id
                 JOIN participant p ON p.id = c.participant_id
                 WHERE p.project_id = ?1 ORDER BY t.conversation_id, t.seq",
                id,
                |row| {
                    Ok(Item {
                        conversation: row.get(0)?,
                        id: row.get(1)?,
                        seq: row.get(2)?,
                        role: row.get(3)?,
                        text: row.get(4)?,
                        complete: row.get::<_, i64>(5)? == 1,
                        at: row.get(6)?,
                        copied_at: row.get(7)?,
                    })
                },
            )?,
            events: rows(
                db,
                "SELECT id, at, kind, data FROM event WHERE project_id = ?1 ORDER BY id",
                id,
                |row| {
                    let data: String = row.get(3)?;
                    Ok(Event {
                        id: row.get(0)?,
                        at: row.get(1)?,
                        kind: row.get(2)?,
                        data: serde_json::from_str(&data).unwrap_or(Value::Null),
                    })
                },
            )?,
            project: Some(project),
        })
    }

    /// The participant `id`.
    pub fn participant(&self, id: i64) -> Option<&Participant> {
        self.participants.iter().find(|someone| someone.id == id)
    }

    /// The handle of the participant `id`, or `?` for one the look has not.
    pub fn handle(&self, id: i64) -> &str {
        self.participant(id).map_or("?", |someone| &someone.handle)
    }

    /// The participant whose handle is `handle`.
    pub fn by_handle(&self, handle: &str) -> Option<&Participant> {
        self.participants
            .iter()
            .find(|someone| someone.handle == handle)
    }

    /// The task whose row is `id`.
    pub fn task(&self, id: i64) -> Option<&Task> {
        self.tasks.iter().find(|task| task.id == id)
    }

    /// The conversations of the participant `id`, the oldest first.
    pub fn conversations_of(&self, participant: i64) -> Vec<&Conversation> {
        self.conversations
            .iter()
            .filter(|conversation| conversation.participant == participant)
            .collect()
    }

    /// The items of the conversations of the participant `id`, in the order they
    /// were said: a member's window may have had several conversations.
    pub fn items_of(&self, participant: i64) -> Vec<&Item> {
        let ids: Vec<i64> = self
            .conversations_of(participant)
            .iter()
            .map(|conversation| conversation.id)
            .collect();
        self.items
            .iter()
            .filter(|item| ids.contains(&item.conversation))
            .collect()
    }

    /// The messages of the task whose row is `task`, in order.
    pub fn messages_of(&self, task: i64) -> Vec<&Message> {
        self.messages
            .iter()
            .filter(|message| message.task == Some(task))
            .collect()
    }

    /// The events of the log of `kind`, in order.
    pub fn events_of(&self, kind: &str) -> Vec<&Event> {
        self.events
            .iter()
            .filter(|event| event.kind == kind)
            .collect()
    }
}

/// The rows `sql` selects for the project `project`, each made by `row`.
fn rows<T>(
    db: &Connection,
    sql: &str,
    project: i64,
    row: impl FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<T>,
) -> Result<Vec<T>> {
    Ok(db
        .prepare(sql)?
        .query_map([project], row)?
        .collect::<std::result::Result<Vec<_>, _>>()?)
}
