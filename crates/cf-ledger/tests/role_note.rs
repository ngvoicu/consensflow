//! What a replacement of a role is to the ledger: migration 0018 gives the
//! message table a column that no row needs a value in, a note written to say a
//! conversation its role holds the text and no other message does, and a ledger
//! opened again still knows which messages they are.

#![allow(clippy::expect_used)]

use cf_ledger::{
    migrate, open_ledger, Ledger, NewChief, NewNote, NewProject, Options, ProjectView, MIGRATIONS,
};
use cf_proto::cf_word::CfWord;
use rusqlite::Connection;

const AT: &str = "2026-10-14T12:00:00.000Z";

fn ledger_in(dir: &tempfile::TempDir) -> Ledger {
    open_ledger(&dir.path().join("consensflow.db"), Options::default()).expect("a ledger")
}

fn project(ledger: &mut Ledger) -> ProjectView {
    ledger
        .create_project(&NewProject {
            directory: "/work/app".into(),
            name: "app".into(),
            chief: NewChief {
                harness: "codex".into(),
                model: Some("diana".into()),
                effort: None,
                model_name: None,
            },
            staff: Vec::new(),
            gate: false,
            wake_up: None,
            specs: None,
            staff_rules: None,
            compaction: None,
            advisor_tool: false,
            prompt_suggestion: false,
        })
        .expect("a project")
}

/// The cells of the one row `sql` reads, as SQLite holds them.
fn cells(db: &Connection, sql: &str) -> Vec<String> {
    let mut read = db.prepare(sql).expect("a query");
    let columns = read.column_count();
    read.query_row([], move |row| {
        (0..columns)
            .map(|at| {
                row.get::<_, rusqlite::types::Value>(at)
                    .map(|value| format!("{value:?}"))
            })
            .collect::<rusqlite::Result<Vec<_>>>()
    })
    .expect("a row")
}

#[test]
fn a_populated_version_17_ledger_gains_the_role_text_column_of_message_in_migration_18_and_every_row_stays(
) {
    let db = Connection::open_in_memory().expect("a database");
    db.execute_batch("PRAGMA foreign_keys = ON").expect("keys");
    migrate(&db, &MIGRATIONS[..17]).expect("version 17");
    db.execute_batch(&format!(
        "INSERT INTO project (id, directory, name, state, gate, created_at, updated_at)
           VALUES (1, '/work/app', 'app', 'open', 0, '{AT}', '{AT}');
         INSERT INTO participant (id, project_id, handle, role, roles, agent, harness, tier, member_id, created_at, designer) VALUES
           (1, 1, 'human', 'human', '[]', NULL, NULL, NULL, NULL, '{AT}', 0),
           (2, 1, 'chief', 'chief', '[]', 'diana', 'codex', NULL, NULL, '{AT}', 0);
         INSERT INTO message (id, project_id, recipient_id, sender_id, kind, body, state, attempts, created_at)
           VALUES (1, 1, 2, NULL, 'note', 'Your ConsensFlow role instructions have changed', 'queued', 0, '{AT}');"
    ))
    .expect("a populated ledger");
    let message = "SELECT id, project_id, recipient_id, sender_id, kind, task_id, reply_to, body, questions, choices, state, attempts, reason, receipt, created_at, delivered_at, carried_by, claimed_at, door_closed_at FROM message";
    let before = cells(&db, message);

    // Through 0018 and no further: what a later migration does is its own test's.
    migrate(&db, &MIGRATIONS[..18]).expect("migrated");
    migrate(&db, &MIGRATIONS[..18]).expect("and again");

    let version: i64 = db
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("the version");
    assert_eq!(version, 18);
    assert_eq!(
        cells(&db, message),
        before,
        "the message's own columns are as they were"
    );
    assert_eq!(
        cells(&db, "SELECT role_text FROM message"),
        ["Null"],
        "no message says a role: its words are none of the ledger's knowledge of it"
    );
    let last: String = db
        .query_row(
            "SELECT name FROM pragma_table_info('message') ORDER BY cid DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .expect("the last column");
    assert_eq!(last, "role_text", "the new one comes last");
    let integrity: String = db
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .expect("the check");
    assert_eq!(integrity, "ok");
    assert!(
        !db.prepare("PRAGMA foreign_key_check")
            .expect("the check")
            .exists([])
            .expect("the check"),
        "every reference holds"
    );
}

#[test]
fn a_ledger_opened_again_still_knows_which_messages_say_a_role_and_which_only_say_its_words() {
    let dir = tempfile::tempdir().expect("a home");
    let mut ledger = ledger_in(&dir);
    let app = project(&mut ledger);
    let chief = app
        .participants
        .iter()
        .find(|participant| participant.role == "chief")
        .expect("a chief")
        .id;
    let words = "Your ConsensFlow role instructions have changed since this conversation began.";
    let said = ledger
        .note_role(app.id, "chief", words, "the role")
        .expect("a note that says a role");
    let alike = ledger
        .note(
            app.id,
            &NewNote {
                from: None,
                to: "chief".into(),
                body: words.into(),
                task: None,
            },
        )
        .expect("a note with the same words");
    assert_eq!(ledger.role_notes_to(chief).expect("read"), [said.id]);
    assert_eq!(said.body, alike.body);
    assert_eq!(
        (said.kind.as_str(), said.sender.clone(), said.task_number),
        ("note", None, None),
        "from ConsensFlow, about no task, as a note of ConsensFlow's is"
    );

    drop(ledger);
    let ledger = ledger_in(&dir);
    assert_eq!(
        ledger.role_notes_to(chief).expect("read"),
        [said.id],
        "it is the file's"
    );
}

/// T-1513: a part of a role read (`receive_read_part`) is the recipient's own, of a note
/// that says a role, and a part there is: anything else is nothing, logs nothing and
/// leaves the note as it was. A role of one part is read as one printed whole is.
/// T-1513 (review T-1516): a part read is of its recipient's own role, of a part there is,
/// and of the number of parts its window's `cf` prints the note in, which the ledger counts
/// itself: a read that says another number is refused and says why, a role in two parts said
/// read as one whole is no read of it, and one in one part said read in two is none either.
#[test]
fn a_part_read_is_of_its_recipients_own_role_of_a_part_there_is_and_of_the_parts_the_ledger_counts()
{
    let dir = tempfile::tempdir().expect("a home");
    let mut ledger = ledger_in(&dir);
    let app = project(&mut ledger);
    let chief = app
        .participants
        .iter()
        .find(|participant| participant.role == "chief")
        .expect("a chief")
        .id;
    let delivered = |ledger: &mut Ledger, id: i64| {
        ledger.begin_delivery(id).expect("begun");
        ledger.confirm_delivery(id, None).expect("confirmed");
    };
    let cf = CfWord::bare();
    let long: String = (0..350)
        .map(|at| format!("Line {at} of the role."))
        .collect::<Vec<_>>()
        .join("\n\n");
    assert_eq!(cf_base::role_read::role_parts(&long).len(), 2);
    let role = ledger
        .note_role(app.id, "chief", &long, "the role")
        .expect("a role")
        .id;
    delivered(&mut ledger, role);
    let words = ledger
        .note(
            app.id,
            &NewNote {
                from: None,
                to: "chief".into(),
                body: long.clone(),
                task: None,
            },
        )
        .expect("a note")
        .id;
    delivered(&mut ledger, words);
    let read = cf_ledger::Read::Inbox;
    for (recipient, id, part, of, via, case) in [
        (chief + 1_000, role, 2, 2, read, "another's"),
        (chief, words, 2, 2, read, "no role"),
        (chief, role, 0, 2, read, "part 0"),
        (chief, role, 3, 2, read, "past the last"),
        (chief, role, 2, 2, cf_ledger::Read::Task, "by cf task get"),
    ] {
        for part in [1, part] {
            assert_eq!(
                ledger
                    .receive_read_part(recipient, id, part, of, via, &cf)
                    .expect("read"),
                0,
                "{case}"
            );
        }
        assert_eq!(
            ledger.message(role).expect("read").expect("there").state,
            "delivered",
            "{case}"
        );
    }
    let refused = ledger
        .receive_read_part(chief, role, 1, 1, read, &cf)
        .expect_err("a role in 2 parts is not read whole");
    assert_eq!(refused.code(), Some("invalid-read"));
    assert_eq!(
        refused.to_string(),
        format!("m-{role} is printed in 2 parts in this window, not 1: a read says the part `cf inbox read` printed and how many it prints the note in")
    );
    assert_eq!(
        ledger.message(role).expect("read").expect("there").state,
        "delivered"
    );
    let one = ledger
        .note_role(app.id, "chief", "A short role.", "a short role")
        .expect("a role")
        .id;
    delivered(&mut ledger, one);
    let refused = ledger
        .receive_read_part(chief, one, 2, 2, read, &cf)
        .expect_err("a role in 1 part is not read in 2");
    assert_eq!(
        refused.to_string(),
        format!("m-{one} is printed in 1 part in this window, not 2: a read says the part `cf inbox read` printed and how many it prints the note in")
    );
    assert_eq!(
        ledger.message(one).expect("read").expect("there").state,
        "delivered"
    );
    assert_eq!(
        ledger
            .receive_read_part(chief, one, 1, 1, read, &cf)
            .expect("read"),
        1
    );
    assert_eq!(
        ledger.message(one).expect("read").expect("there").state,
        "read"
    );
}

/// What happens to a role note too long to be printed whole, between its paste's first
/// try and its confirmation.
#[derive(Debug, Clone, Copy)]
enum Step {
    /// The paste begins (`begin_delivery`): the note is `delivering`.
    Begin,
    /// The window prints part N, in whatever state the note is.
    Prints(usize),
    /// The paste is given back to the queue (`retry_delivery`): the note is `queued`.
    Retry,
    /// The paste is confirmed (`confirm_delivery`): the note is `delivered`, or `read`.
    Confirm,
    /// The window's conversation is cleared: a new one begins.
    Cleared,
}

/// T-1513 (review T-1516): whether a role read in parts was read does not hang on the state
/// its note was in when each part was printed. The review's case is the first row: part 1
/// while the paste is on its way, the paste given back, part 2 while queued, then the paste
/// again and confirmed. Then the other orders a retry makes, a part printed twice across
/// one, parts out of order, and the reads before and after a confirmed paste. Every part
/// printed in the conversation the role goes to is the role read, and told: the note is
/// `read` and the conversation holds the role, so the next Resume says nothing of it. A
/// part not printed, or printed in a conversation since cleared, is not.
#[test]
fn a_role_read_in_parts_across_a_retried_paste_is_read_whatever_state_each_part_was_printed_in() {
    use Step::{Begin, Cleared, Confirm, Prints, Retry};
    let body: String = (0..350)
        .map(|at| format!("Line {at} of the role."))
        .collect::<Vec<_>>()
        .join("\n\n");
    let of = cf_base::role_read::role_parts(&body).len();
    assert_eq!(of, 2, "the role is printed in 2 parts");
    let mut wrong = Vec::new();
    for (steps, told) in [
        (
            &[Begin, Prints(1), Retry, Prints(2), Begin, Confirm][..],
            true,
        ),
        (&[Begin, Retry, Prints(1), Prints(2), Begin, Confirm], true),
        (&[Begin, Retry, Prints(1), Begin, Prints(2), Confirm], true),
        (
            &[
                Begin,
                Prints(1),
                Prints(2),
                Retry,
                Prints(2),
                Begin,
                Confirm,
            ],
            true,
        ),
        (&[Begin, Prints(2), Retry, Prints(1), Begin, Confirm], true),
        (&[Begin, Prints(1), Prints(2), Confirm], true),
        (&[Begin, Confirm, Prints(1), Prints(2)], true),
        (&[Begin, Confirm, Prints(2), Prints(1)], true),
        (&[Begin, Prints(1), Retry, Begin, Confirm], false),
        (&[Begin, Confirm, Prints(1)], false),
        (&[Begin, Prints(1), Prints(2), Cleared, Confirm], false),
        (&[Begin, Prints(1), Cleared, Prints(2), Confirm], false),
        (&[Begin, Confirm, Prints(1), Cleared, Prints(2)], false),
    ] {
        let dir = tempfile::tempdir().expect("a home");
        let mut ledger = ledger_in(&dir);
        let app = project(&mut ledger);
        let chief = app
            .participants
            .iter()
            .find(|participant| participant.role == "chief")
            .expect("a chief")
            .id;
        let mut conversation = ledger
            .start_conversation(chief, "codex")
            .expect("a conversation")
            .id;
        let id = ledger
            .note_role(app.id, "chief", &body, "the long role")
            .expect("a role")
            .id;
        let mut states = Vec::new();
        for step in steps {
            match *step {
                Begin => {
                    ledger.begin_delivery(id).expect("begun");
                }
                Prints(part) => {
                    printed(&mut ledger, chief, id, part, of);
                }
                Retry => {
                    ledger
                        .retry_delivery(id, "the record never showed it", true)
                        .expect("given back");
                }
                Confirm => {
                    ledger
                        .confirm_delivery(id, Some(&serde_json::json!({ "item": "i-1" })))
                        .expect("confirmed");
                }
                Cleared => {
                    conversation = ledger
                        .start_conversation(chief, "codex")
                        .expect("a conversation")
                        .id;
                }
            }
            states.push(ledger.message(id).expect("read").expect("there").state);
        }
        let state = ledger.message(id).expect("read").expect("there").state;
        let held = ledger.conversation_role_text(conversation).expect("read");
        if state != if told { "read" } else { "delivered" }
            || held.as_deref() != told.then_some("the long role")
        {
            wrong.push(format!("{steps:?}: {states:?}, told {:?}", held.is_some()));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// The window of `recipient` printed part `part` of `of` of the note `id` (`cf inbox read
/// m-N --part K`), and said so to the board.
fn printed(ledger: &mut Ledger, recipient: i64, id: i64, part: usize, of: usize) -> usize {
    ledger
        .receive_read_part(
            recipient,
            id,
            part,
            of,
            cf_ledger::Read::Inbox,
            &CfWord::bare(),
        )
        .expect("read")
}
