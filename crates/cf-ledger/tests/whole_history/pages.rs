//! What a read of the history goes through of the ledger's file, counted in
//! SQLite's pages: the same where the fields no read of the history uses run
//! to 32 MiB as where they are short. A row holds its values one after
//! another, a long one on a chain of pages, and reading a value past it goes
//! through the chain (some 8,200 pages for 32 MiB), which no value limit and no
//! count of steps shows. So the history reads what it uses from an index that
//! holds it, an item's head, or a row's fields before anything long; and these
//! count the pages each read takes to show it: an item's id, the participant's
//! roles, the project's name and description, a conversation's role, a
//! message's body and (in a ledger an older build wrote) the time of an event a
//! heading reads, each 32 MiB, cost what short ones do.

use std::ffi::c_int;

use cf_ledger::{open_ledger, HistoryFrom, Ledger, NewNote, Options};
use rusqlite::{ffi, params, Connection};
use serde_json::json;

use super::old_logs::{of, version_18, AT};
use super::reads::{ask, ids, LOADED_AT_MOST, ROOM};
use super::{id_of, item, ledger_in, project};

/// The reviewers' 32 MiB.
const LONG: usize = 32 << 20;

/// How many pages more a read may take where the fields are long: a tree laid
/// out a little otherwise. One long value gone through is some 8,200.
pub(super) const SLACK: i32 = 2;

/// The pages SQLite's cache gave the ledger's connection since this was last
/// asked: those it held, and those it read from the file. A long value's pages
/// that lie in the database file itself, not in the write-ahead log, can be
/// read straight from it past the cache, and then count for nothing: a read of
/// the whole 24 MiB text counted 9 pages so, 6,159 with its pages in the log
/// (`reads.rs` keeps what it writes there). The fixtures here end checkpointed,
/// so their counts see a read that goes through a long value's chain to a
/// field past it, and the value limit [`costs`] sets is what refuses a read
/// that loads one whole.
pub(super) fn fetched(ledger: &Ledger) -> i32 {
    let counted = |what: c_int| {
        let (mut now, mut most) = (0, 0);
        // SAFETY: the handle is the ledger's open connection's, which outlives
        // this call; SQLite writes the two counts and nothing else.
        let code = unsafe {
            ffi::sqlite3_db_status(ledger.sqlite().handle(), what, &mut now, &mut most, 1)
        };
        assert_eq!(code, ffi::SQLITE_OK);
        now
    };
    counted(ffi::SQLITE_DBSTATUS_CACHE_HIT) + counted(ffi::SQLITE_DBSTATUS_CACHE_MISS)
}

/// The pages each read takes: the newest page with the tools' outputs left out
/// and shown, a search, the newest item's part, and a message's opening.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Costs {
    page: i32,
    tools: i32,
    search: i32,
    part: i32,
    opening: i32,
}

/// What each read of the chief's history in `conversation` of `project` takes,
/// and message `message`'s opening: each read once first, so that what SQLite
/// reads of its schema to make a statement is not counted.
fn costs(ledger: &Ledger, project: i64, conversation: i64, message: i64) -> Costs {
    // A read that loaded a field of 32 MiB whole would fail, the value too long.
    ledger.limit_value_length(LOADED_AT_MOST).expect("held");
    let counted = |read: &dyn Fn()| {
        read();
        fetched(ledger);
        read();
        fetched(ledger)
    };
    let read = |find: Option<&'static str>, tools: bool| {
        move || {
            let read = ledger
                .read_history(
                    project,
                    "chief",
                    &ask(HistoryFrom::Newest, find, tools, ROOM),
                )
                .expect("a read");
            assert_eq!(ids(&read).len(), 2, "both items: {:?}", read.scanned);
        }
    };
    let newest = ledger
        .read_history(
            project,
            "chief",
            &ask(HistoryFrom::Newest, None, false, ROOM),
        )
        .expect("a page")
        .entries[0]
        .key()
        .row;
    Costs {
        page: counted(&read(None, false)),
        tools: counted(&read(None, true)),
        search: counted(&read(Some("said"), false)),
        part: counted(&|| {
            let part = ledger
                .read_history_part(project, "chief", conversation, newest, 0)
                .expect("a part");
            assert_eq!((part.text.as_str(), part.next), ("said two", None));
        }),
        opening: counted(&|| {
            let opening = ledger.message_opening(message).expect("read");
            assert!(opening.expect("there").opening.starts_with("bbbb"));
        }),
    }
}

/// Writes `size` bytes in each field of `db`'s chief, project and conversation
/// `conversation` that no read of the history uses but the rows hold before
/// or among those it does: the chief's roles, the project's name and
/// description, the conversation's role.
fn fields_of(db: &Connection, conversation: i64, size: usize) {
    db.execute(
        "UPDATE participant SET roles = ?1 WHERE handle = 'chief'",
        [format!("[\"{}\"]", "r".repeat(size))],
    )
    .expect("the chief's roles");
    db.execute(
        "UPDATE project SET name = ?1, description = ?1",
        ["n".repeat(size)],
    )
    .expect("the project's name and description");
    db.execute(
        "UPDATE conversation SET role_text = ?1 WHERE id = ?2",
        params!["c".repeat(size), conversation],
    )
    .expect("the conversation's role");
}

/// How long the short id is: a little longer than one an item's text is read
/// past in its row (1,024 bytes), so that its text is kept in a part as the
/// long one's is, and each read takes the same steps for both.
const SHORT_ID: usize = 1_025;

/// A chief's conversation of two items, the newest of an id of `id` bytes,
/// and a note to the chief of a body of `size`, with each field of
/// [`fields_of`] as long: the ledger, the project, the conversation and the note.
fn copied(dir: &tempfile::TempDir, id: usize, size: usize) -> (Ledger, i64, i64, i64) {
    let mut ledger = ledger_in(dir);
    let view = project(&mut ledger);
    let conversation = ledger
        .start_conversation(id_of(&view, "chief"), "claude-code")
        .expect("a conversation")
        .id;
    // With its time, which its row holds past its id and its text: a time
    // that is none is told by the row's header alone.
    let long = json!({ "id": "i".repeat(id), "role": "user", "text": "said two", "at": AT });
    ledger
        .copy_transcript(conversation, &[item("u-1", "user", "said one"), long], 0)
        .expect("copied");
    let note = ledger
        .note(
            view.id,
            &NewNote {
                from: Some("human".into()),
                to: "chief".into(),
                body: "b".repeat(8),
                task: None,
            },
        )
        .expect("a note")
        .id;
    // Longer than a note may be now, as an older build may have written it.
    ledger
        .sqlite()
        .execute(
            "UPDATE message SET body = ?1 WHERE id = ?2",
            params!["b".repeat(size.max(8)), note],
        )
        .expect("a long body");
    fields_of(ledger.sqlite(), conversation, size);
    (ledger, view.id, conversation, note)
}

/// [`copied`], as a version-18 ledger an older build wrote, migrated; the
/// time of the event that began the conversation, which its row holds before
/// its kind and its data, a time of a fraction of `size` digits.
fn migrated(dir: &tempfile::TempDir, id: usize, size: usize) -> (Ledger, i64, i64, i64) {
    let started = of("conversation.started", 1);
    let file = version_18(dir, 1, &[(started.0, started.1.as_str())]);
    let db = Connection::open(&file).expect("the file");
    db.execute(
        "UPDATE event SET at = ?",
        [format!("2026-10-16T12:00:00.5{}Z", "0".repeat(size))],
    )
    .expect("a long time");
    db.execute(
        "INSERT INTO transcript (conversation_id, item_id, seq, role, text, complete, at, copied_at)
           VALUES (1, ?1, 1, 'user', 'said two', 1, ?2, ?2)",
        params!["i".repeat(id), AT],
    )
    .expect("an item");
    db.execute(
        "UPDATE transcript SET text = 'said one' WHERE item_id = 'i-1'",
        [],
    )
    .expect("the first");
    db.execute(
        "INSERT INTO message (project_id, recipient_id, sender_id, kind, body, state, created_at)
           VALUES (1, 2, NULL, 'note', ?, 'queued', ?)",
        params!["b".repeat(size.max(8)), AT],
    )
    .expect("a note");
    let note = db.last_insert_rowid();
    fields_of(&db, 1, size);
    drop(db);
    let ledger = open_ledger(&file, Options::default()).expect("migrated and open");
    let at: i64 = ledger
        .sqlite()
        .query_row("SELECT octet_length(at) FROM event", [], |row| row.get(0))
        .expect("its time");
    assert!(at <= 64, "a time of {at} bytes");
    (ledger, 1, 1, note)
}

/// Asserts the reads of the long ledger take what those of the short do.
fn alike(short: Costs, long: Costs) {
    let pairs = [
        ("a page", short.page, long.page),
        ("a page with the tools", short.tools, long.tools),
        ("a search", short.search, long.search),
        ("a part", short.part, long.part),
        ("an opening", short.opening, long.opening),
    ];
    for (what, short, long) in pairs {
        assert!(
            long <= short + SLACK,
            "{what} took {long} pages beside 32 MiB fields, {short} beside short ones"
        );
    }
}

#[test]
fn a_read_takes_as_many_pages_beside_32_mib_ids_roles_names_and_bodies_as_beside_short_ones() {
    let dir = tempfile::tempdir().expect("a folder");
    let (ledger, project, conversation, note) = copied(&dir, SHORT_ID, 8);
    let short = costs(&ledger, project, conversation, note);
    drop(ledger);
    let dir = tempfile::tempdir().expect("a folder");
    let (ledger, project, conversation, note) = copied(&dir, LONG, LONG);
    let long = costs(&ledger, project, conversation, note);
    println!("short fields: {short:?}\n32 MiB fields: {long:?}");
    alike(short, long);
}

#[test]
fn a_migrated_ledger_s_reads_take_as_many_pages_beside_32_mib_ids_roles_names_and_bodies() {
    let dir = tempfile::tempdir().expect("a folder");
    let (ledger, project, conversation, note) = migrated(&dir, SHORT_ID, 8);
    let short = costs(&ledger, project, conversation, note);
    drop(ledger);
    let dir = tempfile::tempdir().expect("a folder");
    let (ledger, project, conversation, note) = migrated(&dir, LONG, LONG);
    let long = costs(&ledger, project, conversation, note);
    println!("short fields: {short:?}\n32 MiB fields: {long:?}");
    alike(short, long);
}
