//! The connection every operation runs on, and what every concern shares:
//! one transaction around an operation, the clock, the event log, the rows
//! looked up by id or handle, and the one way a task changes state
//! (`src/ledger/store.js`). The ledger holds it privately; nothing outside
//! the crate reaches the connection.

use std::panic::{self, AssertUnwindSafe};

use cf_base::time::{iso, Clock};
use cf_proto::ledger::MessageView;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};

use crate::model::{quoted, require_active, LedgerError};
use crate::projects::ProjectRow;
use crate::views::{message_view, ParticipantRow, TaskRow, MESSAGE_SELECT, PARTICIPANT_SELECT};
use crate::Event;

pub(crate) struct Store {
    pub(crate) db: Connection,
    clock: Box<dyn Clock>,
    names: Box<dyn FnMut() -> String>,
    /// Told every event as it is logged.
    trace: Box<dyn FnMut(&Event)>,
}

impl Store {
    pub(crate) fn new(
        db: Connection,
        clock: Box<dyn Clock>,
        names: Box<dyn FnMut() -> String>,
        trace: Box<dyn FnMut(&Event)>,
    ) -> Self {
        Self {
            db,
            clock,
            names,
            trace,
        }
    }

    /// A fresh session name: two words.
    pub(crate) fn name(&mut self) -> String {
        (self.names)()
    }

    /// One transaction around `work`; an operation called inside another
    /// joins it. A panic in `work` rolls it back too: a transaction left open
    /// would take every later operation in, and none would commit.
    pub(crate) fn write<T>(
        &mut self,
        work: impl FnOnce(&mut Self) -> Result<T, LedgerError>,
    ) -> Result<T, LedgerError> {
        if !self.db.is_autocommit() {
            return work(self);
        }
        self.db.execute_batch("BEGIN IMMEDIATE")?;
        match panic::catch_unwind(AssertUnwindSafe(|| work(self))) {
            Ok(Ok(value)) => match self.db.execute_batch("COMMIT") {
                Ok(()) => Ok(value),
                Err(cause) => {
                    let _ = self.db.execute_batch("ROLLBACK");
                    Err(cause.into())
                }
            },
            Ok(Err(cause)) => {
                let _ = self.db.execute_batch("ROLLBACK");
                Err(cause)
            }
            Err(unwound) => {
                let _ = self.db.execute_batch("ROLLBACK");
                panic::resume_unwind(unwound)
            }
        }
    }

    /// The time now, as the ledger writes it: one reading of the clock.
    pub(crate) fn at(&mut self) -> String {
        iso(self.clock.now_ms())
    }

    /// A project's row, or a refusal naming the id.
    pub(crate) fn project_row(&self, id: i64) -> Result<ProjectRow, LedgerError> {
        self.db
            .query_row("SELECT * FROM project WHERE id = ?", [id], ProjectRow::read)
            .optional()?
            .ok_or_else(|| {
                LedgerError::refused_with("unknown-project", format!("no project {id}"), 404)
            })
    }

    /// A participant's row, its member's handle with it, or a refusal naming the id.
    pub(crate) fn participant_row(&self, id: i64) -> Result<ParticipantRow, LedgerError> {
        self.db
            .query_row(
                &format!("{PARTICIPANT_SELECT} WHERE p.id = ?"),
                [id],
                ParticipantRow::read,
            )
            .optional()?
            .ok_or_else(|| {
                LedgerError::refused_with(
                    "unknown-participant",
                    format!("no participant {id}"),
                    404,
                )
            })
    }

    /// The participant a row names, which may be none: `no participant null`, as JavaScript said it.
    pub(crate) fn participant_of(&self, id: Option<i64>) -> Result<ParticipantRow, LedgerError> {
        match id {
            Some(id) => self.participant_row(id),
            None => Err(LedgerError::refused_with(
                "unknown-participant",
                "no participant null",
                404,
            )),
        }
    }

    /// A participant still in the project, by its handle; one that left is refused.
    pub(crate) fn participant_by_handle(
        &self,
        project_id: i64,
        handle: &str,
    ) -> Result<ParticipantRow, LedgerError> {
        self.project_row(project_id)?;
        let row = self
            .db
            .query_row(
                &format!("{PARTICIPANT_SELECT} WHERE p.project_id = ? AND p.handle = ?"),
                params![project_id, handle],
                ParticipantRow::read,
            )
            .optional()?
            .ok_or_else(|| {
                LedgerError::refused_with(
                    "unknown-participant",
                    format!("{} is not in project {project_id}", quoted(handle)),
                    404,
                )
            })?;
        require_active(&row)?;
        Ok(row)
    }

    /// A task of the project by its number, or a refusal naming it.
    pub(crate) fn task_row(&self, project_id: i64, number: i64) -> Result<TaskRow, LedgerError> {
        self.db
            .query_row(
                "SELECT * FROM task WHERE project_id = ? AND number = ?",
                params![project_id, number],
                TaskRow::read,
            )
            .optional()?
            .ok_or_else(|| {
                LedgerError::refused_with(
                    "unknown-task",
                    format!("no task T-{number} in project {project_id}"),
                    404,
                )
            })
    }

    /// One message, or none.
    pub(crate) fn message(&self, id: i64) -> Result<Option<MessageView>, LedgerError> {
        Ok(self
            .db
            .query_row(
                &format!("{MESSAGE_SELECT} WHERE m.id = ?"),
                [id],
                message_view,
            )
            .optional()?)
    }

    /// A task moves to another state, and the log says so (`task.state`),
    /// `detail` after the move. A member leaving, a delivery and a decision
    /// all move tasks, so the move lives here, under every concern that makes one.
    pub(crate) fn move_task(
        &mut self,
        task: &TaskRow,
        to: &str,
        detail: Value,
    ) -> Result<(), LedgerError> {
        self.move_task_as(task, to, detail, "task.state")
    }

    /// A task's move, logged as `kind`.
    pub(crate) fn move_task_as(
        &mut self,
        task: &TaskRow,
        to: &str,
        detail: Value,
        kind: &str,
    ) -> Result<(), LedgerError> {
        let at = self.at();
        self.db.execute(
            "UPDATE task SET state = ?, updated_at = ?, held_until = NULL WHERE id = ?",
            params![to, at, task.id],
        )?;
        let mut data = json!({ "task": task.number, "from": task.state, "to": to });
        if let (Value::Object(data), Value::Object(detail)) = (&mut data, detail) {
            data.extend(detail);
        }
        self.log(task.project_id, kind, data)
    }

    /// Logs an event of the project, and tells the trace.
    pub(crate) fn log(
        &mut self,
        project_id: i64,
        kind: &str,
        data: Value,
    ) -> Result<(), LedgerError> {
        let at = self.at();
        let data = cf_base::json::js_order(data);
        self.db.execute(
            "INSERT INTO event (project_id, at, kind, data) VALUES (?, ?, ?, ?)",
            rusqlite::params![project_id, at, kind, data.to_string()],
        )?;
        (self.trace)(&Event {
            at,
            project: project_id,
            kind: kind.to_string(),
            data,
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    struct Ticks(i64);

    impl Clock for Ticks {
        fn now_ms(&mut self) -> i64 {
            self.0 += 1000;
            self.0
        }
    }

    fn store() -> (Store, Rc<RefCell<Vec<Event>>>) {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE TABLE event (id INTEGER PRIMARY KEY, project_id INTEGER, at TEXT, kind TEXT, data TEXT)")
            .unwrap();
        let events = Rc::new(RefCell::new(Vec::new()));
        let told = Rc::clone(&events);
        let trace = Box::new(move |event: &Event| told.borrow_mut().push(event.clone()));
        let names = Box::new(|| "amber-pine".to_string());
        (Store::new(db, Box::new(Ticks(0)), names, trace), events)
    }

    fn logged(store: &Store) -> i64 {
        store
            .db
            .query_row("SELECT COUNT(*) FROM event", [], |row| row.get(0))
            .unwrap()
    }

    #[test]
    fn commits_an_operation_whole_or_writes_nothing() {
        let (mut store, _) = store();
        store
            .write(|store| store.log(1, "one", serde_json::json!({})))
            .unwrap();
        let refused = store.write(|store| {
            store.log(1, "two", serde_json::json!({}))?;
            Err::<(), _>(LedgerError::refused("no", "no"))
        });
        assert!(refused.is_err());
        assert_eq!(
            logged(&store),
            1,
            "the refused operation's event is gone with it"
        );
        assert!(store.db.is_autocommit());
    }

    #[test]
    fn joins_the_transaction_an_outer_operation_holds() {
        let (mut store, _) = store();
        let refused = store.write(|store| {
            store.write(|store| store.log(1, "inner", serde_json::json!({})))?;
            Err::<(), _>(LedgerError::refused("no", "no"))
        });
        assert!(refused.is_err());
        assert_eq!(
            logged(&store),
            0,
            "the inner one joined the outer and went with it"
        );
    }

    #[test]
    fn rolls_back_when_an_operation_panics_so_later_ones_still_commit() {
        let (mut store, _) = store();
        let unwound = panic::catch_unwind(AssertUnwindSafe(|| {
            let _ = store.write(|store| -> Result<(), LedgerError> {
                store.log(1, "lost", serde_json::json!({}))?;
                panic!("in the middle of an operation");
            });
        }));
        assert!(unwound.is_err());
        assert!(store.db.is_autocommit(), "no transaction is left open");
        store
            .write(|store| store.log(1, "after", serde_json::json!({})))
            .unwrap();
        assert_eq!(logged(&store), 1);
    }

    #[test]
    fn logs_an_event_with_one_reading_of_the_clock_and_tells_the_trace() {
        let (mut store, events) = store();
        store
            .write(|store| {
                store.log(
                    7,
                    "task.state",
                    serde_json::json!({ "task": 3, "2": "two" }),
                )
            })
            .unwrap();
        let (at, data): (String, String) = store
            .db
            .query_row("SELECT at, data FROM event", [], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .unwrap();
        assert_eq!(at, "1970-01-01T00:00:01.000Z");
        assert_eq!(
            data, r#"{"2":"two","task":3}"#,
            "keys in the order JSON.stringify writes them"
        );
        let told = events.borrow();
        assert_eq!(
            (told[0].at.as_str(), told[0].project, told[0].kind.as_str()),
            (at.as_str(), 7, "task.state")
        );
        assert_eq!(store.at(), "1970-01-01T00:00:02.000Z", "the next reading");
    }
}
