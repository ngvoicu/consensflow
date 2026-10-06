//! What Node's ledger wrote means to this one: each line of the table of rows
//! written before the migration or by Node's daemon after it. The rows are
//! made as Node made them (what it never wrote is left as the migration made
//! it), by changing the file where this ledger's own would differ.

use cf_ledger::{migrate, Claim, MIGRATIONS};
use rusqlite::Connection;

use crate::fixture::{ids, world, World};

/// A task given to zeus whose brief was received, with one question asked
/// through a door: the task, its question's id.
fn working_and_asking(w: &mut World) -> (i64, i64) {
    let created = w.give("zeus", "Parser");
    let (task, brief) = (created.task.number, created.message.expect("a brief").id);
    w.deliver(brief);
    let question = w.ask_with_options("zeus", task);
    (task, question.id)
}

#[test]
fn a_task_node_paused_has_no_stop_to_pay_and_its_doors_read_closed_by_its_state() {
    let mut w = world();
    let (task, question) = working_and_asking(&mut w);
    let zeus = w.id("zeus");
    // Node's pause: the state, and no stop counted, no door shut.
    w.edit(|db| {
        db.execute_batch(
            "UPDATE task SET state = 'paused', paused_at = updated_at;
             UPDATE message SET door_closed_at = NULL",
        )
        .unwrap();
    });
    let stop = w.ledger.stop_of(zeus).unwrap().expect("its task");
    assert_eq!(
        (stop.number, stop.seq),
        (task, 0),
        "harmless: nothing is owed"
    );
    assert_eq!(
        w.ledger.claim_answer(question, zeus).unwrap(),
        Claim::Closed,
        "paused is closed, whoever paused it"
    );
}

#[test]
fn a_question_from_nodes_time_has_its_door_open() {
    let mut w = world();
    let (_, question) = working_and_asking(&mut w);
    let zeus = w.id("zeus");
    w.edit(|db| {
        db.execute_batch("UPDATE message SET door_closed_at = NULL")
            .unwrap();
    });
    assert_eq!(
        w.ledger.claim_answer(question, zeus).unwrap(),
        Claim::Waiting
    );
}

#[test]
fn a_choice_answer_node_marked_read_with_no_receipt_is_taken_at_nodes_word_and_no_receipt_is_made()
{
    let mut w = world();
    let (task, question) = working_and_asking(&mut w);
    let zeus = w.id("zeus");
    let answer = w.choose(question, "blue");
    w.edit(|db| {
        db.execute(
            "UPDATE message SET state = 'read', receipt = NULL, delivered_at = NULL WHERE id = ?",
            [answer.id],
        )
        .unwrap();
    });
    // Anything that reconciles the task finds its question answered.
    let note = w.note("chief", "zeus", task, "Mind the tests");
    w.deliver(note.id);
    assert_eq!(w.state(task), "working");
    let read = w.message(answer.id);
    assert_eq!(
        (read.state.as_str(), read.receipt.is_null()),
        ("read", true)
    );
    assert!(matches!(
        w.ledger.claim_answer(question, zeus).unwrap(),
        Claim::Answered(found) if found.id == answer.id
    ));
}

#[test]
fn such_an_answer_written_after_the_pause_was_read_by_a_door_that_was_dead_and_is_kept_again() {
    let mut w = world();
    let (task, question) = working_and_asking(&mut w);
    w.pause(task);
    let answer = w.choose(question, "blue");
    w.edit(|db| {
        db.execute(
            "UPDATE message SET state = 'read', receipt = NULL, delivered_at = NULL WHERE id = ?",
            [answer.id],
        )
        .unwrap();
    });
    let words = w.resume(task, "Go on").message.expect("its words");
    let begun = w.deliver(words.id);
    assert_eq!(
        ids(&begun.carried),
        [answer.id],
        "set back to queued at the fold, and carried"
    );
    assert_eq!(w.state(task), "working");
}

#[test]
fn an_answer_nodes_pause_cancelled_stays_cancelled_and_its_question_still_obliges() {
    let mut w = world();
    let (task, question) = working_and_asking(&mut w);
    let answer = w.choose(question, "blue");
    w.edit(|db| {
        db.execute(
            "UPDATE message SET state = 'cancelled' WHERE id = ?",
            [answer.id],
        )
        .unwrap();
    });
    let note = w.note("chief", "zeus", task, "Mind the tests");
    w.deliver(note.id);
    assert_eq!(w.state(task), "waiting", "nobody received an answer to it");
    assert_eq!(
        w.states(&[answer.id]),
        ["cancelled"],
        "and the one it had stays so"
    );
    // It may be answered again.
    let again = w.choose(question, "red");
    w.deliver(again.id);
    assert_eq!(w.state(task), "working");
}

#[test]
fn a_question_nodes_pause_cancelled_before_the_chief_saw_it_obliges_no_more() {
    let mut w = world();
    let (task, question) = working_and_asking(&mut w);
    w.edit(|db| {
        db.execute(
            "UPDATE message SET state = 'cancelled' WHERE id = ?",
            [question],
        )
        .unwrap();
    });
    let note = w.note("chief", "zeus", task, "Mind the tests");
    w.deliver(note.id);
    assert_eq!(
        w.state(task),
        "working",
        "cancelled with no answer on its way: nothing waits for it"
    );
}

#[test]
fn a_gated_question_node_withdrew_when_it_was_answered_obliges_while_its_answer_is_on_its_way() {
    let mut w = world();
    let (task, question) = working_and_asking(&mut w);
    let answer = w.choose(question, "blue");
    w.edit(|db| {
        db.execute(
            "UPDATE message SET state = 'cancelled', reason = 'answered by @chief' WHERE id = ?",
            [question],
        )
        .unwrap();
        db.execute(
            "UPDATE message SET state = 'gated' WHERE id = ?",
            [answer.id],
        )
        .unwrap();
    });
    let note = w.note("chief", "zeus", task, "Mind the tests");
    w.deliver(note.id);
    assert_eq!(w.state(task), "waiting");
    w.ledger
        .approve_message(answer.id, "human")
        .expect("passed on");
    w.deliver(answer.id);
    assert_eq!(w.state(task), "working");
}

#[test]
fn a_populated_version_10_ledger_is_migrated_to_11_and_again_with_every_row_as_it_was() {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch("PRAGMA foreign_keys = ON").unwrap();
    migrate(&db, &MIGRATIONS[..10]).unwrap();
    db.execute_batch(
        "INSERT INTO project (id, directory, name, state, gate, created_at, updated_at)
           VALUES (1, '/work/app', 'app', 'open', 0, '2026-10-01T00:00:00.000Z', '2026-10-01T00:00:00.000Z');
         INSERT INTO participant (id, project_id, handle, role, created_at) VALUES
           (1, 1, 'human', 'human', '2026-10-01T00:00:00.000Z'),
           (2, 1, 'chief', 'chief', '2026-10-01T00:00:00.000Z'),
           (3, 1, 'zeus', 'worker', '2026-10-01T00:00:00.000Z');
         INSERT INTO task (id, project_id, number, title, body, requester_id, assignee_id, state, created_at, updated_at, paused_at)
           VALUES (1, 1, 1, 'Parser', 'Parser', 2, 3, 'paused', '2026-10-01T00:00:00.000Z', '2026-10-01T00:00:00.000Z', '2026-10-01T00:00:00.000Z');
         INSERT INTO message (id, project_id, recipient_id, sender_id, kind, task_id, body, state, created_at) VALUES
           (1, 1, 3, 2, 'task', 1, 'Parser', 'delivered', '2026-10-01T00:00:00.000Z'),
           (2, 1, 2, 3, 'question', 1, 'Which?', 'delivered', '2026-10-01T00:00:00.000Z');
         INSERT INTO message (id, project_id, recipient_id, sender_id, kind, task_id, reply_to, body, state, created_at) VALUES
           (3, 1, 3, 2, 'answer', 1, 2, 'This one', 'read', '2026-10-01T00:00:00.000Z');",
    )
    .unwrap();
    let rows = |db: &Connection| -> Vec<String> {
        ["project", "participant", "event"]
            .iter()
            .flat_map(|table| {
                let mut read = db
                    .prepare(&format!("SELECT * FROM {table} ORDER BY id"))
                    .unwrap();
                let columns = read.column_count();
                read.query_map([], move |row| {
                    (0..columns)
                        .map(|at| {
                            row.get::<_, rusqlite::types::Value>(at)
                                .map(|value| format!("{value:?}"))
                        })
                        .collect::<rusqlite::Result<Vec<_>>>()
                        .map(|cells| format!("{table}: {}", cells.join("|")))
                })
                .unwrap()
                .map(Result::unwrap)
                .collect::<Vec<_>>()
            })
            .collect()
    };
    let before = rows(&db);

    migrate(&db, &MIGRATIONS).unwrap();
    migrate(&db, &MIGRATIONS).unwrap();

    let version: i64 = db
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, 11);
    let integrity: String = db
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .unwrap();
    assert_eq!(integrity, "ok");
    let dangling = db
        .prepare("PRAGMA foreign_key_check")
        .unwrap()
        .exists([])
        .unwrap();
    assert!(!dangling, "every reference holds");
    assert_eq!(
        rows(&db),
        before,
        "the tables the migration did not touch are as they were"
    );
    let task: (i64, String) = db
        .query_row("SELECT stop_seq, state FROM task WHERE id = 1", [], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .unwrap();
    assert_eq!(task, (0, "paused".into()), "never stopped, and as it was");
    let marked: i64 = db
        .query_row(
            "SELECT count(*) FROM message
             WHERE carried_by IS NOT NULL OR claimed_at IS NOT NULL OR door_closed_at IS NOT NULL",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(marked, 0, "never carried, claimed or shut");
    let states: Vec<String> = db
        .prepare("SELECT state FROM message ORDER BY id")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(
        states,
        ["delivered", "delivered", "read"],
        "and as they were"
    );
}
