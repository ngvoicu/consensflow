//! The connection every operation runs on, and what every concern shares: one
//! transaction around an operation, the clock, the event log, the rows looked
//! up by id or handle, and the one way a task changes state. The ledger holds
//! it privately; nothing outside the crate reaches the connection.

use std::panic::{self, AssertUnwindSafe};

use cf_base::time::{iso, Clock};
use cf_proto::ledger::MessageView;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};

use crate::messages::withdraw_continuations;
use crate::model::{quoted, require_active, LedgerError, ACTIVE_TASK_STATES};
use crate::projects::ProjectRow;
use crate::tasks::{no_longer_failed, no_longer_waiting};
use crate::views::{message_view, ParticipantRow, TaskRow, MESSAGE_SELECT, PARTICIPANT_SELECT};
use crate::Event;

/// What a holding's end says between its closing and the reading of the clock that
/// dates it, within one write.
pub(crate) const ENDING: i64 = -1;

pub(crate) struct Store {
    pub(crate) db: Connection,
    pub(crate) question_run: String,
    clock: Box<dyn Clock>,
    /// What the clock last said, in ms: the time a holding's bounds are kept at
    /// without a reading of their own.
    last_ms: Option<i64>,
    /// A holding ended in this operation and waits for the operation's next
    /// reading of the clock to say when ([`ENDING`]).
    ending: bool,
    /// The write that dated a holding's end failed: the operation's commit fails with it.
    end_failed: Option<rusqlite::Error>,
    names: Box<dyn FnMut() -> String>,
    /// Told each event once it is committed, in the order it was logged.
    trace: Box<dyn FnMut(&Event)>,
    /// Events of the open transaction, discarded if it does not commit.
    logged: Vec<Event>,
    /// An operation's write is open: what it logs waits for its commit, even where SQLite
    /// has ended the transaction under it (a full disk rolls it back).
    open: bool,
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
            question_run: crate::holding::initial_run(),
            clock,
            last_ms: None,
            ending: false,
            end_failed: None,
            names,
            trace,
            logged: Vec::new(),
            open: false,
        }
    }

    /// A fresh session name: two words.
    pub(crate) fn name(&mut self) -> String {
        (self.names)()
    }

    /// One transaction around `work`; an operation called inside another
    /// joins it. A panic in `work` rolls it back too: a transaction left open
    /// would take every later operation in, and none would commit. The trace
    /// is told the transaction's events only after its commit returns.
    pub(crate) fn write<T>(
        &mut self,
        work: impl FnOnce(&mut Self) -> Result<T, LedgerError>,
    ) -> Result<T, LedgerError> {
        if self.open {
            self.lost()?;
            return work(self);
        }
        if !self.db.is_autocommit() {
            return work(self);
        }
        self.db.execute_batch("BEGIN IMMEDIATE")?;
        self.open = true;
        match panic::catch_unwind(AssertUnwindSafe(|| work(self))) {
            Ok(Ok(value)) => match self
                .settle_ends_unread()
                .and_then(|()| self.end_failed.take().map_or(Ok(()), Err))
                .and_then(|()| self.db.execute_batch("COMMIT"))
            {
                Ok(()) => {
                    self.open = false;
                    for event in std::mem::take(&mut self.logged) {
                        (self.trace)(&event);
                    }
                    Ok(value)
                }
                Err(cause) => {
                    self.roll_back();
                    Err(cause.into())
                }
            },
            Ok(Err(cause)) => {
                self.roll_back();
                Err(cause)
            }
            Err(unwound) => {
                self.roll_back();
                panic::resume_unwind(unwound)
            }
        }
    }

    /// An operation that never read the clock after a holding ended dates its end by
    /// the reading before it.
    fn settle_ends_unread(&mut self) -> rusqlite::Result<()> {
        if self.ending {
            let at = self.last_ms.unwrap_or_default();
            self.settle_ends(at);
        }
        Ok(())
    }

    /// Rolls back the transaction and drops everything it logged.
    fn roll_back(&mut self) {
        self.open = false;
        self.ending = false;
        self.end_failed = None;
        let _ = self.db.execute_batch("ROLLBACK");
        self.logged.clear();
    }

    /// The operation stops where SQLite ended its transaction (a settlement that failed
    /// and rolled it back): the saved error is the operation's.
    fn lost(&mut self) -> Result<(), LedgerError> {
        match self.end_failed.take() {
            Some(cause) => Err(cause.into()),
            None => Ok(()),
        }
    }

    /// A batch is one visible operation and needs a transaction of its own.
    pub(crate) fn write_batch<T>(
        &mut self,
        work: impl FnOnce(&mut Self) -> Result<T, LedgerError>,
    ) -> Result<T, LedgerError> {
        if self.open || !self.db.is_autocommit() {
            return Err(LedgerError::refused(
                "nested-batch",
                "a batch needs its own transaction",
            ));
        }
        self.write(work)
    }

    /// The time now, as the ledger writes it: one reading of the clock.
    pub(crate) fn at(&mut self) -> String {
        iso(self.now_ms())
    }

    /// The time now in milliseconds: one reading of the clock.
    pub(crate) fn now_ms(&mut self) -> i64 {
        let now = self.clock.now_ms();
        self.last_ms = Some(now);
        self.settle_ends(now);
        now
    }

    /// A holding that ended in this operation has the operation's next reading
    /// of the clock for its end, which an event of the same operation makes: the
    /// ledger reads the clock no more often for it.
    pub(crate) fn end_with_next_reading(&mut self) {
        self.ending = true;
    }

    fn settle_ends(&mut self, at: i64) {
        if std::mem::take(&mut self.ending) {
            // A failed write fails the operation: its commit reads the error.
            if let Err(cause) = self.db.execute(
                "UPDATE question_holding SET ended_at = ?1 WHERE ended_at = ?2",
                rusqlite::params![at, ENDING],
            ) {
                self.end_failed.get_or_insert(cause);
            }
        }
    }

    /// The time the clock last said, in milliseconds, with no reading of its own:
    /// the operation that sets a holding's bounds has read the clock for its events
    /// already, and a ledger's replay counts its readings. A clock not yet read is read.
    pub(crate) fn recent_ms(&mut self) -> i64 {
        match self.last_ms {
            Some(at) => at,
            None => self.now_ms(),
        }
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
    /// A move ends a hold, and the mark that a project's suspension paused the
    /// task: the pause that sets either sets it after the move. A move out of the
    /// work (to any state but working and waiting) withdraws what told the task's
    /// window to go on with it and was not handed to it (`continuations`), and a move out of
    /// `failed`, or of the wait for a failed task, what told the requester of the failure and
    /// was not given it (`outage`).
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
            "UPDATE task SET state = ?, updated_at = ?, held_until = NULL, paused_with_project = 0,
             retry_hold = NULL WHERE id = ?",
            params![to, at, task.id],
        )?;
        // A result starts the task's failed turns' tries again (D3's N35); a reopen does so
        // by the message it queues (`queue`).
        if to == "done" {
            crate::tasks::start_tries_again(self, task.id, &at)?;
        }
        if matches!(to, "accepted" | "cancelled" | "failed") {
            crate::holding::close_task(self, task, None)?;
        }
        // What its task's line said of it, a note shown and not typed, held until now.
        crate::tasks::end_asides(self, task.id, &at)?;
        let mut data = json!({ "task": task.number, "from": task.state, "to": to });
        if let (Value::Object(data), Value::Object(detail)) = (&mut data, detail) {
            data.extend(detail);
        }
        self.log(task.project_id, kind, data)?;
        // Every terminal transition closes the previous window's question doors,
        // including failures made by a delivery rather than by a task verb.
        if matches!(to, "accepted" | "cancelled" | "failed") {
            if let Some(window) = task.assignee_id {
                crate::tasks::shut_tracked_doors(self, task, window)?;
            }
        }
        if !ACTIVE_TASK_STATES.contains(&to) {
            withdraw_continuations(self, task, to)?;
        }
        // A task no longer failed, or no longer waiting, leaves what told it so untrue.
        if task.state == "failed" && to != "failed" {
            no_longer_failed(self, task, to)?;
        }
        if matches!(task.state.as_str(), "open" | "paused") && !matches!(to, "open" | "paused") {
            no_longer_waiting(self, task, to)?;
        }
        Ok(())
    }

    /// Logs an event of the project, telling the trace after it commits.
    pub(crate) fn log(
        &mut self,
        project_id: i64,
        kind: &str,
        data: Value,
    ) -> Result<(), LedgerError> {
        let at = self.at();
        self.lost()?;
        let data = cf_base::json::js_order(data);
        self.db.execute(
            "INSERT INTO event (project_id, at, kind, data) VALUES (?, ?, ?, ?)",
            rusqlite::params![project_id, at, kind, data.to_string()],
        )?;
        let event = Event {
            at,
            project: project_id,
            kind: kind.to_string(),
            data,
        };
        if self.open || !self.db.is_autocommit() {
            self.logged.push(event);
        } else {
            (self.trace)(&event);
        }
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

    fn kinds(events: &Rc<RefCell<Vec<Event>>>) -> Vec<String> {
        events
            .borrow()
            .iter()
            .map(|event| event.kind.clone())
            .collect()
    }

    #[test]
    fn a_refused_operation_tells_the_trace_nothing() {
        let (mut store, events) = store();
        let refused = store.write(|store| {
            store.log(1, "refused", json!({}))?;
            Err::<(), _>(LedgerError::refused("no", "no"))
        });
        assert!(refused.is_err());
        assert_eq!(logged(&store), 0);
        assert!(kinds(&events).is_empty());
        store
            .write(|store| store.log(1, "after", json!({})))
            .unwrap();
        assert_eq!(kinds(&events), ["after"]);
    }

    #[test]
    fn an_inner_operation_goes_untold_with_the_outer_that_failed() {
        let (mut store, events) = store();
        let refused = store.write(|store| {
            store.log(1, "outer", json!({}))?;
            store.write(|store| store.log(1, "inner", json!({})))?;
            Err::<(), _>(LedgerError::refused("no", "no"))
        });
        assert!(refused.is_err());
        assert_eq!(logged(&store), 0);
        assert!(kinds(&events).is_empty());
        store
            .write(|store| store.log(1, "after", json!({})))
            .unwrap();
        assert_eq!(kinds(&events), ["after"]);
    }

    #[test]
    fn a_commit_that_fails_tells_nothing() {
        let (mut store, events) = store();
        // The insert succeeds; the deferred key is checked by COMMIT itself.
        store.db.execute_batch(
            "PRAGMA foreign_keys = ON;
             CREATE TABLE parent (id INTEGER PRIMARY KEY);
             CREATE TABLE child (parent INTEGER REFERENCES parent(id) DEFERRABLE INITIALLY DEFERRED);",
        ).unwrap();
        let failed = store.write(|store| {
            store.log(1, "uncommitted", json!({}))?;
            store.db.execute("INSERT INTO child VALUES (99)", [])?;
            Ok(())
        });
        assert!(failed.is_err(), "COMMIT must fail on the deferred key");
        assert!(store.db.is_autocommit());
        assert_eq!(logged(&store), 0);
        assert!(kinds(&events).is_empty());
        store
            .write(|store| store.log(1, "after", json!({})))
            .unwrap();
        assert_eq!(kinds(&events), ["after"]);
    }

    #[test]
    fn an_operation_that_panics_tells_nothing() {
        let (mut store, events) = store();
        let unwound = panic::catch_unwind(AssertUnwindSafe(|| {
            let _ = store.write(|store| -> Result<(), LedgerError> {
                store.log(1, "lost", json!({}))?;
                panic!("in the middle of an operation");
            });
        }));
        assert!(unwound.is_err());
        assert!(store.db.is_autocommit());
        assert_eq!(logged(&store), 0);
        assert!(kinds(&events).is_empty());
        store
            .write(|store| store.log(1, "after", json!({})))
            .unwrap();
        assert_eq!(kinds(&events), ["after"]);
    }

    #[test]
    fn what_committed_is_told_in_the_order_it_was_logged() {
        let (mut store, events) = store();
        store
            .write(|store| {
                store.log(1, "first", json!({}))?;
                store.write(|store| store.log(1, "inner", json!({})))?;
                store.log(1, "last", json!({}))?;
                assert!(kinds(&events).is_empty(), "nothing is told before COMMIT");
                Ok(())
            })
            .unwrap();
        assert_eq!(kinds(&events), ["first", "inner", "last"]);
        store
            .write_batch(|store| {
                store.log(1, "batch-one", json!({}))?;
                store.log(1, "batch-two", json!({}))?;
                assert_eq!(kinds(&events), ["first", "inner", "last"]);
                Ok(())
            })
            .unwrap();
        assert_eq!(
            kinds(&events),
            ["first", "inner", "last", "batch-one", "batch-two"]
        );
    }

    #[test]
    fn an_event_logged_outside_an_operation_is_told_at_once() {
        let (mut store, events) = store();
        store.log(1, "alone", json!({})).unwrap();
        assert_eq!(logged(&store), 1);
        assert_eq!(kinds(&events), ["alone"]);
    }

    #[test]
    fn a_batch_inside_an_operation_is_still_refused() {
        let (mut store, events) = store();
        store
            .write(|store| {
                let refused = store.write_batch(|store| store.log(1, "batch", json!({})));
                assert_eq!(refused.unwrap_err().code(), Some("nested-batch"));
                store.log(1, "outer", json!({}))
            })
            .unwrap();
        assert_eq!(kinds(&events), ["outer"]);
    }
}
