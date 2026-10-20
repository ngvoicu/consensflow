//! A project written by hand into a ledger file made from the ledger's own
//! migrations: the rows a look of a long run reads, at the times a test says, and
//! the look itself.

use cf_evals::ledger_file::{self, add_project, Chief, Member, NewProject};
use cf_evals::long_run::ledger::Ledger;
use cf_evals::long_run::look::{Look, Reading};
use cf_evals::long_run::record::Record;
use rusqlite::{params, Connection};
use serde_json::Value;

use crate::support::Folder;

/// The time `minute`:`second` past 10:00 of 2026-10-15, as the ledger writes one.
pub fn t(minute: u32, second: u32) -> String {
    format!(
        "2026-10-15T{:02}:{:02}:{:02}.000Z",
        10 + minute / 60,
        minute % 60,
        second
    )
}

/// The same time in milliseconds.
pub fn ms(minute: u32, second: u32) -> i64 {
    cf_evals::clock::millis(&t(minute, second)).unwrap()
}

/// The participants of a scene: the human is 1, the chief 2, and the members from 3.
pub const HUMAN: i64 = 1;
pub const CHIEF: i64 = 2;

/// A ledger of a project with a Claude Code chief and the members `staff`, each an
/// agent on a harness, a worker of standard tier.
pub struct Scene {
    _folder: Folder,
    pub db: Connection,
    pub project: i64,
}

impl Scene {
    pub fn new(staff: &[(&str, &str)]) -> Self {
        let folder = Folder::new();
        let db = ledger_file::open(&folder.ledger(), false).unwrap();
        let members: Vec<Member> = staff
            .iter()
            .map(|(agent, harness)| Member {
                agent,
                harness,
                roles: &["worker"],
                tier: "standard",
            })
            .collect();
        let project = add_project(
            &db,
            &NewProject {
                directory: "/work/ledgerline",
                name: "ledgerline",
                chief: Chief {
                    harness: "claude-code",
                    agent: Some("long-chief"),
                },
                staff: &members,
            },
        )
        .unwrap();
        Self {
            _folder: folder,
            db,
            project,
        }
    }

    /// A conversation of `participant`'s window: its row.
    pub fn conversation(&self, participant: i64, harness: &str, session: &str) -> i64 {
        self.db
            .execute(
                "INSERT INTO conversation (participant_id, harness, native_session, started_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![participant, harness, session, t(0, 0)],
            )
            .unwrap();
        self.db.last_insert_rowid()
    }

    /// The conversation `id` ended at `at`: a participant has one at a time.
    pub fn end_conversation(&self, id: i64, at: &str) {
        self.db
            .execute(
                "UPDATE conversation SET ended_at = ?2 WHERE id = ?1",
                params![id, at],
            )
            .unwrap();
    }

    /// T-`number`, the chief's, held by `assignee` in `state` since `since`: its row.
    pub fn task(&self, number: i64, assignee: i64, state: &str, since: &str) -> i64 {
        self.db
            .execute(
                "INSERT INTO task (project_id, number, title, body, requester_id, assignee_id, state, pool, tier, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?3, ?4, ?5, ?6, 'worker', 'standard', ?7, ?8)",
                params![self.project, number, format!("Task {number}"), CHIEF, assignee, state, t(0, 0), since],
            )
            .unwrap();
        self.db.last_insert_rowid()
    }

    /// The message `id` (ids are given, as the field's are named), delivered.
    #[allow(clippy::too_many_arguments)]
    pub fn message(
        &self,
        id: i64,
        (recipient, sender): (i64, Option<i64>),
        kind: &str,
        task: Option<i64>,
        body: &str,
        at: &str,
    ) {
        self.db
            .execute(
                "INSERT INTO message (id, project_id, recipient_id, sender_id, kind, task_id, body, state, created_at, delivered_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'delivered', ?8, ?8)",
                params![id, self.project, recipient, sender, kind, task, body, at],
            )
            .unwrap();
    }

    /// The chief's task message `id` of the task whose row is `task` to `recipient`, its
    /// delivery begun at `at`, as the log tells it.
    pub fn briefed(&self, id: i64, recipient: i64, task: i64, at: &str) {
        self.message(
            id,
            (recipient, Some(CHIEF)),
            "task",
            Some(task),
            "the brief",
            at,
        );
        self.event(
            at,
            "delivery.begun",
            &serde_json::json!({ "message": id, "attempt": 1 }),
        );
    }

    /// An item of a conversation's copy, at its place `seq`.
    pub fn item(
        &self,
        conversation: i64,
        seq: i64,
        role: &str,
        text: &str,
        complete: bool,
        at: &str,
    ) {
        self.db
            .execute(
                "INSERT INTO transcript (conversation_id, item_id, seq, role, text, complete, at, copied_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
                params![conversation, format!("i-{conversation}-{seq}"), seq, role, text, i64::from(complete), at],
            )
            .unwrap();
    }

    /// An entry of the log.
    pub fn event(&self, at: &str, kind: &str, data: &Value) {
        self.db
            .execute(
                "INSERT INTO event (project_id, at, kind, data) VALUES (?1, ?2, ?3, ?4)",
                params![self.project, at, kind, data.to_string()],
            )
            .unwrap();
    }

    /// The look at `at`, with no window and no record.
    pub fn look(&self, at: i64) -> Look {
        Look {
            at,
            ledger: Ledger::read(&self.db).unwrap(),
            ..Look::default()
        }
    }

    /// The look at `at`, with the record `record` of the conversation `conversation`.
    pub fn look_with(&self, at: i64, conversation: i64, record: Record) -> Look {
        let mut look = self.look(at);
        look.records.insert(conversation, record);
        look
    }

    /// The look at `at`, with the record `record` of the conversation `conversation` as
    /// far as its last reading that held read it (`through`), its latest reading failing
    /// `failed`.
    pub fn look_read(
        &self,
        at: i64,
        conversation: i64,
        record: Record,
        (through, failed): (i64, &str),
    ) -> Look {
        let mut look = self.look_with(at, conversation, record);
        look.readings.insert(
            conversation,
            Reading {
                through: Some(through),
                failed: Some(failed.to_owned()),
            },
        );
        look
    }
}

/// The breaks of one invariant that a look finds.
pub fn found(look: &Look, invariant: u8) -> Vec<cf_evals::long_run::invariants::Break> {
    cf_evals::long_run::invariants::check(look, &Default::default())
        .into_iter()
        .filter(|found| found.invariant == invariant)
        .collect()
}
