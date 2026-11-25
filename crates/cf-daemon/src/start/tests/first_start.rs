//! A used home and its first start (P23-A2; T-2241 findings 9 to 12, 16): the home is judged
//! before anything is written, a database is ConsensFlow's only with its marks, a first start
//! is all or nothing, and the marker comes before the first write to a ledger that was there.

use std::cell::{Cell, RefCell};
use std::io;
use std::path::Path;

use super::broken::{a_marked_home, started_over};
use super::*;
use crate::broken::{Home, Steps, STEPS};

fn names(dir: &Path) -> Vec<String> {
    let mut found: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    found.sort();
    found
}

fn lost(home: &Path) -> String {
    crate::broken::lost_words(&home.join("consensflow.db"))
}

/// A database that is no ledger of ConsensFlow's: its own tables and `version`.
fn unrelated(file: &Path, version: i64, tables: &[&str]) {
    let db = rusqlite::Connection::open(file).unwrap();
    for table in tables {
        db.execute_batch(&format!("CREATE TABLE {table} (id INTEGER)"))
            .unwrap();
    }
    db.execute_batch(&format!("PRAGMA user_version = {version}"))
        .unwrap();
}

/// What a start that is refused for the home's own sake says: the status, the words.
fn refused_with(refused: Result<(), StartError>) -> (i32, String) {
    match refused {
        Err(failed @ StartError::CannotSave(_)) => {
            let code = failed.exit_code();
            let StartError::CannotSave(words) = failed else {
                unreachable!()
            };
            (code, words)
        }
        other => panic!("not refused as 74: {other:?}"),
    }
}

/// An unrelated SQLite database at the ledger's name, in a marked home and in an unmarked one,
/// is no ledger: the start is refused as 74 with the lost-ledger words, never migrated or put
/// in WAL, and its bytes and its folder are as they were.
#[tokio::test]
async fn an_unrelated_database_at_the_ledgers_name_is_refused_and_left_as_it_was() {
    LocalSet::new()
        .run_until(async {
            let all = [
                "project",
                "participant",
                "conversation",
                "task",
                "task_need",
                "message",
                "transcript",
                "event",
            ];
            let cases: Vec<(&str, i64, Vec<&str>)> = vec![
                ("own tables, no version", 0, vec!["notes"]),
                ("own tables, version 43", 43, vec!["notes", "people"]),
                ("version 44, seven tables", 44, all[..7].to_vec()),
                ("the eight tables, no version", 0, all.to_vec()),
            ];
            for marked in [true, false] {
                for (row, version, tables) in &cases {
                    let root = tempfile::tempdir().unwrap();
                    let home = if marked {
                        a_marked_home(root.path())
                    } else {
                        let home = root.path().join("consensflow");
                        std::fs::create_dir_all(&home).unwrap();
                        home
                    };
                    let file = home.join("consensflow.db");
                    unrelated(&file, *version, tables);
                    let (bytes, beside) = (std::fs::read(&file).unwrap(), names(&home));
                    let (code, words) = refused_with(started_over(root.path()).await);
                    let row = format!("{row}, marked {marked}");
                    assert_eq!(code, crate::broken::EXIT, "{row}");
                    assert_eq!(words, lost(&home), "{row}");
                    assert_eq!(std::fs::read(&file).unwrap(), bytes, "{row}: its bytes");
                    let made: Vec<String> = names(&home)
                        .into_iter()
                        .filter(|name| !beside.contains(name) && !name.starts_with("daemon.log"))
                        .collect();
                    assert!(made.is_empty(), "{row}: made {made:?}");
                }
            }
        })
        .await;
}

fn listed(names: &'static [&'static str]) -> Box<crate::broken::Lister> {
    Box::new(move |_| Ok(names.iter().map(|name| Ok((*name).to_owned())).collect()))
}

/// What a home is: a witness in a listing the stand-in gives, or one a named look finds.
#[test]
fn a_home_is_judged_from_its_witnesses_and_a_look_that_fails_is_unknown() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    // A folder that is not there is unused.
    assert!(matches!(crate::broken::judge(&home), Home::Unused));
    std::fs::create_dir_all(&home).unwrap();
    assert!(matches!(crate::broken::judge(&home), Home::Unused));
    let judged = |list: &crate::broken::Lister| crate::broken::judge_with(&home, list);
    // A listing that fails for any reason but "not there" is unknown, with no witness named.
    let denied = |_: &Path| -> io::Result<Vec<io::Result<String>>> {
        Err(io::ErrorKind::PermissionDenied.into())
    };
    assert!(matches!(judged(&denied), Home::Unknown(..)));
    // An entry of the listing that can't be read, too.
    let half = |_: &Path| -> io::Result<Vec<io::Result<String>>> {
        Ok(vec![Ok("a".to_owned()), Err(io::Error::other("an entry"))])
    };
    assert!(matches!(judged(&half), Home::Unknown(..)));
    // A listing that says the folder is not there is not a failure.
    let gone =
        |_: &Path| -> io::Result<Vec<io::Result<String>>> { Err(io::ErrorKind::NotFound.into()) };
    assert!(matches!(judged(&gone), Home::Unused));
    // A witness found settles it even where the listing failed, or an entry did not read.
    std::fs::write(home.join("daemon.log"), "x").unwrap();
    assert!(matches!(judged(&denied), Home::Used));
    std::fs::remove_file(home.join("daemon.log")).unwrap();
    assert!(matches!(
        judged(&*listed(&["events.jsonl.old.jsonl"])),
        Home::Used
    ));
    assert!(matches!(judged(&*listed(&["notes.txt"])), Home::Unused));
    let ordered = |_: &Path| -> io::Result<Vec<io::Result<String>>> {
        Ok(vec![
            Err(io::Error::other("an entry")),
            Ok("p.jsonl".to_owned()),
        ])
    };
    assert!(
        matches!(judged(&ordered), Home::Used),
        "a witness after a failed entry"
    );
}

/// A home that can't be listed, and one that can't be searched: the start is refused as 74,
/// the folder named, and nothing in it is made.
#[cfg(unix)]
#[tokio::test]
async fn a_home_that_cannot_be_listed_or_searched_is_refused_with_its_folder_named() {
    use std::os::unix::fs::PermissionsExt;
    LocalSet::new()
        .run_until(async {
            for (row, mode) in [("cannot be listed", 0o300), ("cannot be searched", 0o600)] {
                let root = tempfile::tempdir().unwrap();
                let home = root.path().join("consensflow");
                std::fs::create_dir_all(&home).unwrap();
                std::fs::set_permissions(&home, std::fs::Permissions::from_mode(mode)).unwrap();
                if std::fs::read_dir(&home).is_ok() && mode == 0o300 {
                    std::fs::set_permissions(&home, std::fs::Permissions::from_mode(0o700))
                        .unwrap();
                    continue; // a session that lists past its mode (root): nothing to prove
                }
                let refused = started_over(root.path()).await;
                std::fs::set_permissions(&home, std::fs::Permissions::from_mode(0o700)).unwrap();
                let (code, words) = refused_with(refused);
                assert_eq!(code, crate::broken::EXIT, "{row}");
                assert_eq!(words, cf_base::home::unread_home_words(&home), "{row}");
                assert!(
                    words.contains(&home.display().to_string()),
                    "{row}: {words}"
                );
                assert!(words.contains("Nothing in it was changed."), "{row}");
                assert_eq!(
                    names(&home),
                    Vec::<String>::new(),
                    "{row}: nothing was made"
                );
            }
        })
        .await;
}

/// The same refusal on every platform, through a stand-in for the listing.
#[tokio::test]
async fn a_listing_that_fails_refuses_the_start_with_the_unread_words() {
    let dir = tempfile::tempdir().unwrap();
    let denied = |_: &Path| -> io::Result<Vec<io::Result<String>>> {
        Err(io::ErrorKind::PermissionDenied.into())
    };
    let Home::Unknown(home, failed) = crate::broken::judge_with(dir.path(), &denied) else {
        panic!("a failed listing is unknown");
    };
    assert_eq!(home, dir.path());
    assert_eq!(failed.kind(), io::ErrorKind::PermissionDenied);
    let words = cf_base::home::unread_home_words(&home);
    assert!(words.contains(&dir.path().display().to_string()), "{words}");
    assert!(words.contains("Nothing in it was changed."), "{words}");
}

/// Each name the daemon writes in a home once it has a ledger shows that the home was used, in a
/// home that has no other: the start is refused as lost, whatever else the home holds, and
/// writes nothing. What is no witness is a first use.
#[tokio::test]
async fn every_witness_shows_a_used_home() {
    LocalSet::new()
        .run_until(async {
            let witnesses = [
                "consensflow.board",
                "consensflow.db-wal",
                "consensflow.db-shm",
                "daemon.log",
                "daemon.log.1",
                "daemon.log.lock",
                "events.jsonl",
                "events.jsonl.1",
                "project-7.jsonl",
            ];
            for name in witnesses {
                let root = tempfile::tempdir().unwrap();
                let home = root.path().join("consensflow");
                std::fs::create_dir_all(&home).unwrap();
                std::fs::write(home.join(name), "x").unwrap();
                let beside = names(&home);
                let (code, words) = refused_with(started_over(root.path()).await);
                assert_eq!(code, crate::broken::EXIT, "{name}");
                assert_eq!(words, lost(&home), "{name}");
                assert_eq!(names(&home), beside, "{name}: nothing was made");
            }
        })
        .await;
}

/// The ledger's own name is a witness whatever is at it: an empty file, a folder, a link.
#[tokio::test]
async fn the_ledgers_name_is_a_witness_as_anything() {
    LocalSet::new()
        .run_until(async {
            type Spoil = fn(&Path);
            let table: Vec<(&str, Spoil)> = vec![
                ("an empty file", |ledger| {
                    std::fs::write(ledger, b"").unwrap()
                }),
                ("a folder", |ledger| std::fs::create_dir(ledger).unwrap()),
                #[cfg(unix)]
                ("a dangling link", |ledger| {
                    std::os::unix::fs::symlink(ledger.with_file_name("nowhere"), ledger).unwrap();
                }),
            ];
            for (row, spoil) in table {
                let root = tempfile::tempdir().unwrap();
                let home = root.path().join("consensflow");
                std::fs::create_dir_all(&home).unwrap();
                spoil(&home.join("consensflow.db"));
                let beside = names(&home);
                let (code, words) = refused_with(started_over(root.path()).await);
                assert_eq!(code, crate::broken::EXIT, "{row}");
                assert_eq!(words, lost(&home), "{row}");
                assert_eq!(names(&home), beside, "{row}: nothing was made");
            }
        })
        .await;
}

thread_local! {
    static COUNT: Cell<usize> = const { Cell::new(0) };
    static CUT_BEFORE: Cell<usize> = const { Cell::new(usize::MAX) };
    static CUT: Cell<bool> = const { Cell::new(false) };
    static FAIL_AT: Cell<usize> = const { Cell::new(usize::MAX) };
    static HELD: RefCell<Vec<cf_ledger::Ledger>> = const { RefCell::new(Vec::new()) };
    static TAKE_THE_NAME: Cell<bool> = const { Cell::new(false) };
}

fn gone() -> io::Error {
    io::Error::new(io::ErrorKind::Interrupted, "cut")
}

/// One file operation of the first start: it is cut before it (the process is gone, and every
/// later operation, the removal of a staging file included, with it), refused (its effect
/// not made), or made.
fn operation(real: impl FnOnce() -> io::Result<()>) -> io::Result<()> {
    if CUT.get() {
        return Err(gone());
    }
    let index = COUNT.get();
    COUNT.set(index + 1);
    if index == CUT_BEFORE.get() {
        CUT.set(true);
        return Err(gone());
    }
    if index == FAIL_AT.get() {
        return Err(io::Error::other("refused"));
    }
    real()
}

fn counted_steps() -> Steps {
    Steps {
        make: |staging| operation(|| (Steps::REAL.make)(staging)),
        flush: |file| operation(|| (Steps::REAL.flush)(file)),
        put: |from, to| {
            operation(|| {
                if TAKE_THE_NAME.get() {
                    // Another ConsensFlow, at the same moment: the name is taken after the look.
                    let other = cf_ledger::open_ledger(to, LedgerOptions::default()).unwrap();
                    HELD.with(|held| held.borrow_mut().push(other));
                }
                (Steps::REAL.put)(from, to)
            })
        },
        remove: |file| operation(|| (Steps::REAL.remove)(file)),
    }
}

fn reset() {
    COUNT.set(0);
    CUT_BEFORE.set(usize::MAX);
    CUT.set(false);
    FAIL_AT.set(usize::MAX);
    TAKE_THE_NAME.set(false);
    HELD.with(|held| held.borrow_mut().clear());
    STEPS.set(Steps::REAL);
}

/// A first creation refused at each step (the staging file's making, in its build, at the put)
/// is an ordinary refusal: it leaves nothing of ours, writes no line of `daemon.log`, ends with
/// 74 and the first-ledger words, and two unmodified retries start.
#[tokio::test]
async fn a_first_creation_refused_at_each_step_is_retried_unmodified() {
    LocalSet::new()
        .run_until(async {
            // Operations of a first start: 0 make, 1 flush, 2 put.
            for (row, at) in [
                ("the staging file's making", 0),
                ("its flush", 1),
                ("the put", 2),
            ] {
                reset();
                let root = tempfile::tempdir().unwrap();
                let home = root.path().join("consensflow");
                FAIL_AT.set(at);
                STEPS.set(counted_steps());
                let (code, words) = refused_with(started_over(root.path()).await);
                reset();
                assert_eq!(code, crate::broken::EXIT, "{row}");
                assert_eq!(
                    words,
                    cf_base::home::first_ledger_words(&home.join("consensflow.db")),
                    "{row}"
                );
                assert!(names(&home).is_empty(), "{row}: left {:?}", names(&home));
                for retry in 1..=2 {
                    started_over(root.path())
                        .await
                        .unwrap_or_else(|failed| panic!("{row}, retry {retry}: {failed}"));
                }
            }
        })
        .await;
}

/// A first start that is cut before each file operation of it, and in the middle of its build,
/// leaves nothing that counts before the put and a whole ledger after it; each is followed by
/// two unmodified starts that come up.
#[tokio::test]
async fn a_first_start_cut_at_every_file_operation_is_followed_by_two_starts_that_come_up() {
    LocalSet::new()
        .run_until(async {
            // Cut before make, flush, put, and then past the put (the marker's, the log's).
            for before in 0..4 {
                reset();
                let root = tempfile::tempdir().unwrap();
                let home = root.path().join("consensflow");
                CUT_BEFORE.set(before);
                STEPS.set(counted_steps());
                let first = started_over(root.path()).await;
                reset();
                let row = format!("cut before operation {before}");
                if before < 3 {
                    assert!(first.is_err(), "{row}");
                    let left = names(&home);
                    assert!(
                        left.iter()
                            .all(|name| name.starts_with("consensflow.db.new-")),
                        "{row}: only a staging file: {left:?}"
                    );
                    assert!(
                        matches!(crate::broken::judge(&home), Home::Unused),
                        "{row}: nothing that counts"
                    );
                } else {
                    first.unwrap_or_else(|failed| panic!("{row}: {failed}"));
                }
                for retry in 1..=2 {
                    started_over(root.path())
                        .await
                        .unwrap_or_else(|failed| panic!("{row}, retry {retry}: {failed}"));
                }
                let left = names(&home);
                assert!(
                    left.iter()
                        .all(|name| !name.starts_with("consensflow.db.new-")),
                    "{row}: a leftover was removed by the start that holds the ledger: {left:?}"
                );
            }
        })
        .await;
}

const STAGING: &str = "consensflow.db.new-00112233445566ff";

/// What a first start leaves where it is cut inside a step, made by hand: an empty staging file
/// (made new), one with a first page and no table, one that is whole and not flushed; and the
/// two states after the put: both names for one file (the link made, the removal not), and
/// the ledger with no marker, then with its marker only. Two unmodified starts come up after
/// each.
#[tokio::test]
async fn the_states_a_cut_leaves_are_each_followed_by_two_starts_that_come_up() {
    LocalSet::new()
        .run_until(async {
            type Leave = fn(&Path);
            let table: Vec<(&str, Leave)> = vec![
                ("an empty staging file", |home| {
                    std::fs::write(home.join(STAGING), b"").unwrap();
                }),
                ("a staging file with a page and no table", |home| {
                    let db = rusqlite::Connection::open(home.join(STAGING)).unwrap();
                    db.execute_batch("BEGIN EXCLUSIVE; COMMIT").unwrap();
                }),
                ("a whole staging file", |home| {
                    cf_ledger::make_ledger(&home.join(STAGING)).unwrap();
                }),
                #[cfg(unix)]
                ("both names for one whole ledger", |home| {
                    cf_ledger::make_ledger(&home.join(STAGING)).unwrap();
                    std::fs::hard_link(home.join(STAGING), home.join("consensflow.db")).unwrap();
                }),
                ("a whole ledger and nothing else", |home| {
                    cf_ledger::make_ledger(&home.join("consensflow.db")).unwrap();
                }),
                ("a whole ledger and its marker", |home| {
                    cf_ledger::make_ledger(&home.join("consensflow.db")).unwrap();
                    crate::broken::mark(home).unwrap();
                }),
                ("a whole ledger, its marker and a log", |home| {
                    cf_ledger::make_ledger(&home.join("consensflow.db")).unwrap();
                    crate::broken::mark(home).unwrap();
                    std::fs::write(home.join("daemon.log"), "start\n").unwrap();
                }),
            ];
            for (row, leave) in table {
                let root = tempfile::tempdir().unwrap();
                let home = root.path().join("consensflow");
                std::fs::create_dir_all(&home).unwrap();
                leave(&home);
                for start in 1..=2 {
                    started_over(root.path())
                        .await
                        .unwrap_or_else(|failed| panic!("{row}, start {start}: {failed}"));
                }
                assert!(
                    !names(&home)
                        .iter()
                        .any(|name| name.starts_with("consensflow.db.new-")),
                    "{row}: the leftover is removed: {:?}",
                    names(&home)
                );
            }
        })
        .await;
}

/// The home as it is listed at every operation up to the ledger holds nothing but the staging
/// file: no log, no trace, no marker, no folder of the app's.
#[tokio::test]
async fn a_first_start_writes_nothing_but_its_staging_file_before_the_ledger_is_at_its_name() {
    LocalSet::new()
        .run_until(async {
            for before in 0..3 {
                reset();
                let root = tempfile::tempdir().unwrap();
                let home = root.path().join("consensflow");
                CUT_BEFORE.set(before);
                STEPS.set(counted_steps());
                let _ = started_over(root.path()).await;
                reset();
                for name in names(&home) {
                    assert!(name.starts_with("consensflow.db.new-"), "{name}");
                }
            }
        })
        .await;
}

/// A name taken between the look and the put: the ledger that took it is unchanged and in use,
/// the start's own staging file is removed, and the start says `ledger-locked`.
#[tokio::test]
async fn a_name_taken_between_the_look_and_the_put_is_refused_as_locked_and_not_replaced() {
    LocalSet::new()
        .run_until(async {
            reset();
            let root = tempfile::tempdir().unwrap();
            let home = root.path().join("consensflow");
            TAKE_THE_NAME.set(true);
            STEPS.set(counted_steps());
            let refused = started_over(root.path()).await;
            let held = HELD.with(|held| held.borrow().len());
            STEPS.set(Steps::REAL);
            let Err(StartError::Ledger(failed)) = refused else {
                panic!("not refused as held: {refused:?}");
            };
            assert_eq!(failed.code(), Some("ledger-locked"), "{failed}");
            assert_eq!(held, 1, "the other ConsensFlow took the name");
            assert!(
                !names(&home)
                    .iter()
                    .any(|name| name.starts_with("consensflow.db.new-")),
                "its staging file is removed: {:?}",
                names(&home)
            );
            HELD.with(|held| {
                let mut held = held.borrow_mut();
                // Still the one that was opened first, still usable.
                assert!(held[0].projects().is_ok());
                held.clear();
            });
            reset();
        })
        .await;
}

/// A start that refuses a home before its ledger is at its name writes no line of the log.
#[tokio::test]
async fn a_first_start_that_fails_writes_no_log_and_no_trace() {
    LocalSet::new()
        .run_until(async {
            reset();
            let root = tempfile::tempdir().unwrap();
            let home = root.path().join("consensflow");
            FAIL_AT.set(2);
            STEPS.set(counted_steps());
            let _ = started_over(root.path()).await;
            reset();
            for name in [
                "daemon.log",
                "daemon.log.lock",
                "events.jsonl",
                "consensflow.board",
            ] {
                assert!(!home.join(name).exists(), "{name}");
            }
        })
        .await;
}

/// A leftover is removed by the daemon that holds the ledger and by no other, and only by its
/// name: a refused start leaves it, and a file that only looks like it is the person's own.
#[tokio::test]
async fn a_leftover_is_removed_only_by_the_start_that_holds_the_ledger() {
    LocalSet::new()
        .run_until(async {
            let root = tempfile::tempdir().unwrap();
            let home = root.path().join("consensflow");
            std::fs::create_dir_all(&home).unwrap();
            cf_ledger::make_ledger(&home.join("consensflow.db")).unwrap();
            let leftover = home.join("consensflow.db.new-00112233445566ff");
            let alike = [
                home.join("consensflow.db.new-0011"),
                home.join("consensflow.db.new-00112233445566fg"),
                home.join("consensflow.db.new-00112233445566ff.bak"),
            ];
            std::fs::write(&leftover, "x").unwrap();
            for name in &alike {
                std::fs::write(name, "x").unwrap();
            }
            // Another daemon holds the ledger: this start is refused and removes nothing.
            let other = cf_ledger::hold_ledger(&home.join("consensflow.db")).unwrap();
            let refused = started_over(root.path()).await;
            assert!(matches!(refused, Err(StartError::Ledger(_))), "{refused:?}");
            assert!(
                leftover.exists(),
                "a start that holds no ledger removes none"
            );
            drop(other);
            started_over(root.path()).await.unwrap();
            assert!(!leftover.exists(), "the holder removed it");
            for name in &alike {
                assert!(name.exists(), "{} is not a staging name", name.display());
            }
        })
        .await;
}

/// A marker that can't be written is refused before the first write to a ledger that was there:
/// the ledger's bytes and its schema version are the same after an upgrade that did not run.
#[tokio::test]
async fn a_marker_that_cannot_be_written_leaves_an_old_ledger_untouched() {
    LocalSet::new()
        .run_until(async {
            let root = tempfile::tempdir().unwrap();
            let home = root.path().join("consensflow");
            std::fs::create_dir_all(&home).unwrap();
            let file = home.join("consensflow.db");
            let older = cf_ledger::open_ledger(&file, LedgerOptions::default()).unwrap();
            cf_ledger::testing::undo_migration_0044(older.sqlite()).unwrap();
            older
                .sqlite()
                .execute_batch("PRAGMA user_version = 43")
                .unwrap();
            drop(older);
            let bytes = std::fs::read(&file).unwrap();
            let in_the_way = home.join(format!(
                "{}.{}.tmp",
                crate::broken::MARKER,
                std::process::id()
            ));
            std::fs::create_dir(&in_the_way).unwrap();
            let (code, words) = refused_with(started_over(root.path()).await);
            assert_eq!(code, crate::broken::EXIT);
            assert!(words.contains("No saved work was changed."), "{words}");
            assert_eq!(
                std::fs::read(&file).unwrap(),
                bytes,
                "the ledger was not written"
            );
            assert!(!names(&home).iter().any(|name| name.ends_with("-wal")));
        })
        .await;
}

/// A used home that is cut in a first start on a file system with no links, between the claim of
/// the name and its fill, leaves an empty ledger file: the next start refuses it, in the lost
/// words, and its way out is to move the home away (a known limit).
#[tokio::test]
async fn an_empty_claim_left_by_a_cut_is_refused_as_lost() {
    LocalSet::new()
        .run_until(async {
            let root = tempfile::tempdir().unwrap();
            let home = root.path().join("consensflow");
            std::fs::create_dir_all(&home).unwrap();
            std::fs::write(home.join("consensflow.db"), b"").unwrap();
            let (code, words) = refused_with(started_over(root.path()).await);
            assert_eq!(code, crate::broken::EXIT);
            assert_eq!(words, lost(&home));
            assert!(words.contains("move"), "{words}");
        })
        .await;
}

/// A marked home whose folder can't be written and whose ledger is whole is a ledger that
/// cannot be saved to, said so, and not one that is lost: its words don't offer to move the
/// home away. A ledger that is not there, or is no ledger, still gets the lost-ledger words.
#[cfg(unix)]
#[tokio::test]
async fn a_whole_ledger_in_a_folder_that_cannot_be_written_is_not_told_as_lost() {
    use std::os::unix::fs::PermissionsExt;
    LocalSet::new()
        .run_until(async {
            let root = tempfile::tempdir().unwrap();
            let home = a_marked_home(root.path());
            let file = home.join("consensflow.db");
            drop(cf_ledger::open_ledger(&file, LedgerOptions::default()).unwrap());
            std::fs::set_permissions(&home, std::fs::Permissions::from_mode(0o555)).unwrap();
            let refused = started_over(root.path()).await;
            std::fs::set_permissions(&home, std::fs::Permissions::from_mode(0o755)).unwrap();
            if refused.is_ok() {
                return; // a session that writes past its mode (root): nothing to prove
            }
            let (code, words) = refused_with(refused);
            assert_eq!(code, crate::broken::EXIT);
            assert_eq!(words, crate::broken::words(&file), "the folder's case");
            assert!(!words.contains("move"), "{words}");
        })
        .await;
}
