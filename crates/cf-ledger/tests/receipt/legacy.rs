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
fn a_gated_question_node_withdrew_whose_answer_node_then_cancelled_obliges_until_it_is_answered_again(
) {
    let mut w = world();
    let (task, question) = working_and_asking(&mut w);
    let answer = w.choose(question, "blue");
    // Node's pause or hold: the question answered at the gate was withdrawn,
    // and the answer, passed on, was cancelled with what else waited.
    w.edit(|db| {
        db.execute(
            "UPDATE message SET state = 'cancelled', reason = 'answered by @chief' WHERE id = ?",
            [question],
        )
        .unwrap();
        db.execute(
            "UPDATE message SET state = 'cancelled' WHERE id = ?",
            [answer.id],
        )
        .unwrap();
    });
    let note = w.note("chief", "zeus", task, "Mind the tests");
    w.deliver(note.id);
    assert_eq!(
        w.state(task),
        "waiting",
        "nobody received an answer to it: it is not left to work on its own"
    );
    let again = w.choose(question, "red");
    w.deliver(again.id);
    assert_eq!(w.state(task), "working");
}

/// What a row Rust folded into a carrier, that came after the carrier, is
/// when the Node daemon had the carrier last and left it in `state`: the
/// ledger is opened again (this daemon starts) with the carrier so, and the
/// task going on, as Node's confirm moves it, unless the carrier still waits.
/// Each of the states a carrier that cannot carry any more can be left in
/// frees what it carried, and the one that waits does not.
fn after_node_left_the_carrier(state: &str) -> (World, i64, i64) {
    after_node_left(state, "queued")
}

/// The same, with the row that rode in the carrier left in `row` as well:
/// `queued`, or `delivering` where Node was pasting it on its own.
fn after_node_left(state: &str, row: &str) -> (World, i64, i64) {
    let mut w = world();
    let created = w.give("zeus", "Parser");
    let (task, brief) = (created.task.number, created.message.expect("a brief").id);
    w.deliver(brief);
    let question = w.ask("zeus", task, "Which format?");
    w.hold(task);
    let carrier = w.daemon_resumes(task).message.expect("its words").id;
    let answer = w.answer(question.id, "JSON").id;
    assert_eq!(
        w.next("zeus"),
        Some(carrier),
        "the answer rides in the carrier"
    );
    w.edit(|db| {
        db.execute(
            "UPDATE message SET state = ? WHERE id = ?",
            (state, carrier),
        )
        .unwrap();
        if state != "queued" {
            db.execute("UPDATE task SET state = 'working' WHERE id = 1", [])
                .unwrap();
        }
        db.execute("UPDATE message SET state = ? WHERE id = ?", (row, answer))
            .unwrap();
    });
    (w, carrier, answer)
}

#[test]
fn a_row_left_under_a_carrier_node_delivered_cancelled_or_failed_is_a_queued_message_of_its_own() {
    for state in ["delivered", "cancelled", "failed"] {
        let (mut w, carrier, answer) = after_node_left_the_carrier(state);
        assert_eq!(
            let_go(&w),
            [serde_json::json!({ "carrier": carrier, "released": [answer] })],
            "the start says what it let go of, from a carrier {state}"
        );
        assert_eq!(
            w.next("zeus"),
            Some(answer),
            "a carrier {state} carries nothing: the answer is the next the window is given ({carrier} is not)"
        );
        assert_eq!(
            w.state(1),
            "waiting",
            "and the question it answers has no answer received yet"
        );
        let begun = w.deliver(answer);
        assert!(begun.carried.is_empty(), "it rides in nothing");
        assert_eq!(w.states(&[answer]), ["delivered"]);
        assert_eq!(w.state(1), "working");
        assert_eq!(w.next("zeus"), None, "and nothing is left queued");
    }
}

/// A home at schema `written_at`, its file open as a build left it.
fn a_home_at(written_at: usize) -> (tempfile::TempDir, Connection) {
    let dir = tempfile::tempdir().expect("a home");
    let db = Connection::open(dir.path().join("consensflow.db")).unwrap();
    db.execute_batch("PRAGMA foreign_keys = ON").unwrap();
    migrate(&db, &MIGRATIONS[..written_at]).unwrap();
    (dir, db)
}

/// What a build left as it stopped while it pasted a carrier, m-3: the native `cf`
/// had folded the chief's answer, m-4, into the words that resume T-1 while they
/// waited, and the daemon had begun the words' paste. In 3.0.0-alpha.82 (schema 11)
/// that daemon may have been Node's, where the home held the `use-node` file: Node
/// knows no carriers, and pasted the words alone. T-1 is `working`, as Node's own
/// confirm left a task it moved on, its question answered by nothing received.
fn a_carrier_on_its_way_left_in(db: &Connection) {
    const AT: &str = "2026-10-08T12:00:00.000Z";
    db.execute_batch(&format!(
        "INSERT INTO project (id, directory, name, state, gate, created_at, updated_at)
           VALUES (1, '/work/app', 'app', 'open', 0, '{AT}', '{AT}');
         INSERT INTO participant (id, project_id, handle, role, roles, agent, harness, tier, created_at) VALUES
           (1, 1, 'human', 'human', '[]', NULL, NULL, NULL, '{AT}'),
           (2, 1, 'chief', 'chief', '[]', 'apollo', 'claude-code', NULL, '{AT}'),
           (3, 1, 'zeus', 'worker', '[\"worker\"]', 'zeus', 'claude-code', 'standard', '{AT}');
         INSERT INTO task (id, project_id, number, title, body, requester_id, assignee_id, state, created_at, updated_at)
           VALUES (1, 1, 1, 'Parser', 'Parser', 2, 3, 'working', '{AT}', '{AT}');
         INSERT INTO message (id, project_id, recipient_id, sender_id, kind, task_id, reply_to, body, state, attempts, created_at, carried_by) VALUES
           (1, 1, 3, 2, 'task', 1, NULL, 'Parser', 'delivered', 1, '{AT}', NULL),
           (2, 1, 2, 3, 'question', 1, NULL, 'Which format?', 'delivered', 1, '{AT}', NULL),
           (3, 1, 3, NULL, 'task', 1, NULL, 'Go on with T-1.', 'delivering', 1, '{AT}', NULL),
           (4, 1, 3, 2, 'answer', 1, 2, 'JSON', 'queued', 0, '{AT}', 3);
         INSERT INTO event (project_id, at, kind, data)
           VALUES (1, '{AT}', 'delivery.begun', '{{\"message\":3,\"attempt\":1}}');"
    ))
    .unwrap();
}

/// The home in `dir` opened by this build, as its start opens it, once the build
/// that held `db` let it go: the carrier and its answer.
fn opened(dir: tempfile::TempDir, db: Connection) -> (World, i64, i64) {
    db.close().expect("the file is closed");
    (World::left_in(dir, 1), 3, 4)
}

/// A carrier on its way at schema `written_at`, opened by this build.
fn a_carrier_on_its_way_as_a_build_left_it(written_at: usize) -> (World, i64, i64) {
    let (dir, db) = a_home_at(written_at);
    a_carrier_on_its_way_left_in(&db);
    opened(dir, db)
}

/// The answer m-4 rode in no paste the record holds: the start lets go of it, and
/// reconciles T-1, whose question it answers; the start's settling confirms the
/// carrier by the marker the record shows, and the answer is the next message of
/// its own.
fn lets_go_of_the_answer(w: &mut World, carrier: i64, answer: i64) {
    assert_eq!(
        let_go(w),
        [serde_json::json!({ "carrier": carrier, "released": [answer] })],
        "what the carrier carried is let go of"
    );
    assert_eq!(
        w.state(1),
        "waiting",
        "and the question it answers has no answer received yet"
    );
    let in_flight = w.ledger.in_flight().unwrap();
    assert_eq!(ids(&in_flight), [carrier]);
    w.confirm(carrier);
    assert_eq!(w.states(&[carrier, answer]), ["delivered", "queued"]);
    assert_eq!(w.next("zeus"), Some(answer));
    let begun = w.deliver(answer);
    assert!(begun.carried.is_empty(), "it rides in nothing");
    assert_eq!(w.states(&[answer]), ["delivered"]);
    assert_eq!(w.state(1), "working");
}

/// What the start let go of, as the log says it.
fn let_go(w: &World) -> Vec<serde_json::Value> {
    w.ledger
        .events(w.project, 0, 500)
        .expect("the log")
        .iter()
        .filter(|event| event.kind == "message.uncarried")
        .map(|event| event.data.clone())
        .collect()
}

/// The reviews of 2026-10-16: a home alpha.82 left, opened by this build, had the
/// answer confirmed from the carrier's marker, though Node never pasted it. What
/// Node pasted was the carrier's own words: the answer was never in the paste, and
/// is not marked received by it. A carrier alpha.83 or 3.0.0 began when it stopped
/// looks the same, and its rows go again: pasted twice, never lost.
#[test]
fn a_row_left_under_a_carrier_node_began_to_paste_is_not_taken_for_received_with_it() {
    let (mut w, carrier, answer) = a_carrier_on_its_way_as_a_build_left_it(11);
    lets_go_of_the_answer(&mut w, carrier, answer);
}

/// A review of 2026-10-16: this build's start took a home alpha.82 left past some of
/// its migrations, each a step of its own, and stopped before it let go of the
/// answer; the next start read a schema past 11 and confirmed the answer from the
/// carrier's marker. The migration that lets go of it is one of the steps.
#[test]
fn a_row_node_left_under_a_carrier_on_its_way_is_let_go_of_by_a_start_after_one_that_stopped_between_two_migrations(
) {
    for stopped_at in 12..MIGRATIONS.len() {
        let (dir, db) = a_home_at(11);
        a_carrier_on_its_way_left_in(&db);
        migrate(&db, &MIGRATIONS[..stopped_at]).unwrap();
        let (mut w, carrier, answer) = opened(dir, db);
        lets_go_of_the_answer(&mut w, carrier, answer);
    }
}

/// A review of 2026-10-16: on 3.0.4 (schema 18) the carrier's first paste was late,
/// so it waited again, and the chief's answer joined it then; its second paste
/// began, and the daemon stopped before it pasted. The window's record shows the
/// first paste, which never held the answer, and its marker is the carrier's.
#[test]
fn a_row_that_joined_a_carrier_an_earlier_build_pasted_again_is_not_taken_for_received_by_its_first_paste(
) {
    const AT: &str = "2026-10-15T12:00:00.000Z";
    let (dir, db) = a_home_at(18);
    a_carrier_on_its_way_left_in(&db);
    db.execute_batch(&format!(
        "UPDATE message SET attempts = 2 WHERE id = 3;
         INSERT INTO event (project_id, at, kind, data) VALUES
           (1, '{AT}', 'delivery.retried', '{{\"message\":3,\"reason\":\"the harness record never showed it\"}}'),
           (1, '{AT}', 'delivery.begun', '{{\"message\":3,\"attempt\":2}}');"
    ))
    .unwrap();
    let (mut w, carrier, answer) = opened(dir, db);
    lets_go_of_the_answer(&mut w, carrier, answer);
}

/// A carrier this build began froze its set at its first paste: its rows were in
/// every paste of it, and the start settles them with it (a review of 2026-10-16:
/// let go of, they were pasted again on their own).
#[test]
fn a_row_left_under_a_carrier_this_daemon_began_to_paste_is_settled_with_it() {
    let (mut w, carrier, answer) = a_carrier_on_its_way_as_a_build_left_it(MIGRATIONS.len());
    assert!(let_go(&w).is_empty(), "the start lets go of nothing");
    assert_eq!(ids(&w.ledger.in_flight().unwrap()), [carrier]);
    w.confirm(carrier);
    assert_eq!(w.states(&[carrier, answer]), ["delivered", "delivered"]);
    assert_eq!(w.next("zeus"), None);
}

#[test]
fn a_row_under_a_carrier_that_still_waits_stays_in_its_paste() {
    let (mut w, carrier, answer) = after_node_left_the_carrier("queued");
    assert_eq!(w.next("zeus"), Some(carrier));
    let begun = w.deliver(carrier);
    assert_eq!(ids(&begun.carried), [answer]);
    assert_eq!(w.states(&[carrier, answer]), ["delivered", "delivered"]);
}

/// What Node left when it was pasting an answer on its own, under a carrier it
/// had delivered: the answer is `delivering`, so the start of this daemon
/// leaves it as it is, and its delivery is settled after.
fn after_node_stopped_pasting_a_row_of_its_own() -> (World, i64, i64) {
    let (w, carrier, answer) = after_node_left("delivered", "delivering");
    assert_eq!(
        ids(&w.ledger.in_flight().unwrap()),
        [answer],
        "in flight, not queued: the start does not release it"
    );
    assert_eq!(
        w.ledger.next_delivery(w.id("zeus")).unwrap(),
        None,
        "and the carrier it rode in is over: the row is nobody's to deliver yet ({carrier})"
    );
    (w, carrier, answer)
}

#[test]
fn a_row_node_was_pasting_on_its_own_under_a_carrier_that_is_over_goes_again_as_a_row_of_its_own() {
    let (mut w, _, answer) = after_node_stopped_pasting_a_row_of_its_own();
    w.ledger
        .retry_delivery(answer, "the daemon stopped before it arrived", false)
        .unwrap();
    assert_eq!(
        w.next("zeus"),
        Some(answer),
        "it rides in nothing: carried, it would be skipped and never delivered"
    );
    let begun = w.deliver(answer);
    assert!(begun.carried.is_empty());
    assert_eq!(w.states(&[answer]), ["delivered"]);
    assert_eq!(w.state(1), "working");
}

#[test]
fn a_row_node_was_pasting_on_its_own_joins_the_carrier_that_waits_when_it_goes_again() {
    let (mut w, _, answer) = after_node_stopped_pasting_a_row_of_its_own();
    // The task is paused and resumed meanwhile: the carrier that waits is new.
    w.pause(1);
    let carrier = w.resume(1, "Go on").message.expect("its words").id;
    w.ledger
        .retry_delivery(answer, "the daemon stopped before it arrived", false)
        .unwrap();
    assert_eq!(w.next("zeus"), Some(carrier), "the words go first, with it");
    let begun = w.deliver(carrier);
    assert_eq!(
        ids(&begun.carried),
        [answer],
        "it rides in the paste of the words that wait for the window"
    );
    assert_eq!(w.states(&[carrier, answer]), ["delivered", "delivered"]);
}

/// The rows of `tables`, in the order they are kept, one line each: the table,
/// then every cell as SQLite holds it.
fn rows_of(db: &Connection, tables: &[&str]) -> Vec<String> {
    tables
        .iter()
        .flat_map(|table| {
            let mut read = db
                .prepare(&format!("SELECT * FROM {table} ORDER BY rowid"))
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
    let rows = |db: &Connection| rows_of(db, &["project", "participant", "event"]);
    let before = rows(&db);

    migrate(&db, &MIGRATIONS[..11]).unwrap();
    migrate(&db, &MIGRATIONS[..11]).unwrap();

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

#[test]
fn a_populated_version_11_ledger_is_migrated_to_12_and_again_as_it_was_and_has_room_for_a_tester() {
    const AT: &str = "2026-10-01T00:00:00.000Z";
    const TABLES: [&str; 8] = [
        "project",
        "participant",
        "conversation",
        "task",
        "task_need",
        "message",
        "event",
        "sqlite_sequence",
    ];
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch("PRAGMA foreign_keys = ON").unwrap();
    migrate(&db, &MIGRATIONS[..11]).unwrap();
    db.execute_batch(&format!(
        "INSERT INTO project (id, directory, name, state, gate, created_at, updated_at)
           VALUES (1, '/work/app', 'app', 'open', 0, '{AT}', '{AT}');
         INSERT INTO participant (id, project_id, handle, role, roles, agent, harness, tier, member_id, created_at, designer) VALUES
           (1, 1, 'human', 'human', '[]', NULL, NULL, NULL, NULL, '{AT}', 0),
           (2, 1, 'chief', 'chief', '[]', 'apollo', 'claude-code', NULL, NULL, '{AT}', 0),
           (3, 1, 'zeus', 'worker', '[\"worker\",\"reviewer\"]', 'zeus', 'claude-code', 'standard', NULL, '{AT}', 0),
           (4, 1, 'pygmalion', 'designer', '[\"designer\"]', 'pygmalion', 'codex', NULL, NULL, '{AT}', 1),
           (5, 1, 'hera', 'reviewer', '[\"reviewer\"]', 'hera', 'claude-code', 'light', NULL, '{AT}', 0),
           (6, 1, 'zeus-amber-pine', 'worker', '[\"worker\"]', 'zeus', 'claude-code', 'standard', 3, '{AT}', 0),
           (7, 1, 'gone', 'advisor', '[\"advisor\"]', 'gone', 'claude-code', 'light', NULL, '{AT}', 0);
         UPDATE participant SET switched_from_harness = 'codex', switched_from_agent = 'mars', switched_from_cut = 1 WHERE id = 2;
         UPDATE participant SET out_until = '2026-10-02T00:00:00.000Z', out_since = '{AT}' WHERE id = 5;
         INSERT INTO conversation (id, participant_id, harness, native_session, started_at)
           VALUES (1, 6, 'claude-code', 'session-1', '{AT}');
         INSERT INTO task (id, project_id, number, title, body, requester_id, assignee_id, state, pool, tier, purpose, created_at, updated_at, taken_from_id, held_until, deleted_at, paused_at, stop_seq) VALUES
           (1, 1, 1, 'Parser', 'Parser', 2, 6, 'paused', NULL, NULL, NULL, '{AT}', '{AT}', 3, '2026-10-02T00:00:00.000Z', NULL, '{AT}', 2),
           (2, 1, 2, 'Review T-1', 'Review T-1', 2, NULL, 'open', 'reviewer', 'standard', 'critical-review', '{AT}', '{AT}', NULL, NULL, '{AT}', NULL, 0),
           (3, 1, 3, 'Dropped', 'Dropped', 2, NULL, 'cancelled', 'worker', 'light', NULL, '{AT}', '{AT}', NULL, NULL, NULL, NULL, 0);
         INSERT INTO task_need (task_id, needs_id) VALUES (2, 1);
         INSERT INTO message (id, project_id, recipient_id, sender_id, kind, task_id, body, state, created_at)
           VALUES (1, 1, 6, 2, 'task', 1, 'Parser', 'delivered', '{AT}');
         INSERT INTO event (id, project_id, at, kind, data) VALUES (1, 1, '{AT}', 'project.created', '{{}}');
         DELETE FROM task WHERE id = 3;
         DELETE FROM participant WHERE id = 7;"
    ))
    .unwrap();
    // A member of a role, and a task for a pool, each a statement of its own.
    let member = |handle: &str, role: &str| {
        format!(
            "INSERT INTO participant (project_id, handle, role, roles, created_at)
               VALUES (1, '{handle}', '{role}', '[\"{role}\"]', '{AT}')"
        )
    };
    let task = |number: i64, pool: &str| {
        format!(
            "INSERT INTO task (project_id, number, title, body, requester_id, state, pool, tier, created_at, updated_at)
               VALUES (1, {number}, 'Test T-1', 'Test T-1', 2, 'open', '{pool}', 'light', '{AT}', '{AT}')"
        )
    };
    let refused = |sql: String| db.execute_batch(&sql).unwrap_err().to_string();
    // Neither check has room for a tester yet.
    assert!(refused(member("tess", "tester")).contains("participant_role_check"));
    assert!(refused(task(9, "tester")).contains("task_pool_check"));
    let before = rows_of(&db, &TABLES);

    migrate(&db, &MIGRATIONS[..12]).unwrap();
    migrate(&db, &MIGRATIONS[..12]).unwrap();

    let version: i64 = db
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, 12);
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
        rows_of(&db, &TABLES),
        before,
        "every row as it was, and each table's sequence, in its place"
    );

    // A tester is a member now, and a task may be for the testers; any other word is still none.
    db.execute_batch(&member("tess", "tester")).unwrap();
    db.execute_batch(&task(9, "tester")).unwrap();
    let ids: (i64, i64) = db
        .query_row(
            "SELECT (SELECT id FROM participant WHERE handle = 'tess'), (SELECT id FROM task WHERE number = 9)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(
        ids,
        (8, 4),
        "the member who left and the task that was deleted keep their numbers"
    );
    assert!(refused(member("chuck", "checker")).contains("participant_role_check"));
    assert!(refused(task(10, "checker")).contains("task_pool_check"));
}

/// Migration 0019 lets go of what rides in each carrier on its way, and of nothing
/// else: the rows of a carrier that waits ride in it still, and those of one that is
/// over are left to the start (`release_stranded`). The log says what it let go of,
/// one line per carrier, and a second run finds nothing to do.
#[test]
fn a_populated_version_18_ledger_is_migrated_to_19_and_again_with_only_what_rode_in_a_carrier_on_its_way_let_go_of(
) {
    const AT: &str = "2026-10-15T12:00:00.000Z";
    let (_dir, db) = a_home_at(18);
    a_carrier_on_its_way_left_in(&db);
    // A note rides in the carrier on its way too; m-6 waits with a note in it, and
    // m-8 was delivered with a note in it still queued.
    db.execute_batch(&format!(
        "INSERT INTO message (id, project_id, recipient_id, sender_id, kind, task_id, body, state, attempts, created_at, carried_by) VALUES
           (5, 1, 3, 2, 'note', 1, 'Mind the tests', 'queued', 0, '{AT}', 3),
           (6, 1, 3, NULL, 'task', 1, 'Go on with T-1.', 'queued', 0, '{AT}', NULL),
           (7, 1, 3, 2, 'note', 1, 'And the docs', 'queued', 0, '{AT}', 6),
           (8, 1, 3, NULL, 'task', 1, 'Go on with T-1.', 'delivered', 1, '{AT}', NULL),
           (9, 1, 3, 2, 'note', 1, 'Left by Node', 'queued', 0, '{AT}', 8);"
    ))
    .unwrap();
    let carriers = |db: &Connection| -> Vec<(i64, Option<i64>)> {
        db.prepare("SELECT id, carried_by FROM message ORDER BY id")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    };
    let uncarried = |db: &Connection| -> Vec<(i64, String, serde_json::Value)> {
        db.prepare("SELECT project_id, at, data FROM event WHERE kind = 'message.uncarried'")
            .unwrap()
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    serde_json::from_str(&row.get::<_, String>(2)?).unwrap(),
                ))
            })
            .unwrap()
            .map(Result::unwrap)
            .collect()
    };
    let before = rows_of(&db, &["project", "participant", "task"]);

    migrate(&db, &MIGRATIONS[..19]).unwrap();
    migrate(&db, &MIGRATIONS[..19]).unwrap();

    let version: i64 = db
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, 19);
    assert_eq!(
        carriers(&db),
        [
            (1, None),
            (2, None),
            (3, None),
            (4, None),
            (5, None),
            (6, None),
            (7, Some(6)),
            (8, None),
            (9, Some(8))
        ]
    );
    let logged = uncarried(&db);
    assert_eq!(logged.len(), 1, "{logged:?}");
    let (project, at, data) = &logged[0];
    assert_eq!(
        (*project, data),
        (1, &serde_json::json!({ "carrier": 3, "released": [4, 5] }))
    );
    assert!(
        at.len() == 24 && at.as_bytes()[10] == b'T' && at.ends_with('Z'),
        "an instant as the ledger writes one: {at}"
    );
    assert_eq!(
        rows_of(&db, &["project", "participant", "task"]),
        before,
        "and nothing else moved"
    );
}
