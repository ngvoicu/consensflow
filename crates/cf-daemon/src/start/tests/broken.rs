//! A ledger that cannot save any more ends the daemon, and the app is told why
//! once, in words that name the ledger by its path (CLN-65).

use cf_ledger::{refuse_next_rollback, NewTask};

use super::*;

#[tokio::test]
async fn a_ledger_that_cannot_save_ends_the_daemon_and_tells_the_app_why_once() {
    LocalSet::new()
        .run_until(async {
            let root = Rc::new(tempfile::tempdir().unwrap());
            let rig = started(&root).await;
            let heard: Rc<RefCell<Vec<Value>>> = Rc::default();
            let told = Rc::clone(&heard);
            rig.app.on_event(crate::broken::EVENT, move |body| {
                told.borrow_mut().push(body.clone())
            });

            {
                let mut ledger = rig.daemon.parts.ledger.borrow_mut();
                // A rollback that SQLite refuses, after an operation that failed.
                refuse_next_rollback();
                let failed = ledger
                    .create_task(
                        99,
                        &NewTask {
                            from: "chief".into(),
                            pool: Some("worker".into()),
                            tier: Some("standard".into()),
                            body: "nowhere".into(),
                            ..NewTask::default()
                        },
                    )
                    .unwrap_err();
                assert!(failed.to_string().contains("no project 99"), "{failed}");
                assert!(ledger.broken().is_some());
            }
            tokio::time::timeout(Duration::from_secs(10), rig.daemon.finished())
                .await
                .expect("the daemon stops");

            let file = rig.home.join("consensflow.db");
            let heard = heard.borrow();
            assert_eq!(heard.len(), 1, "told once");
            assert_eq!(heard[0]["words"], crate::broken::words(&file).as_str());
            let said = heard[0]["words"].as_str().unwrap();
            assert!(
                said.contains(&file.display().to_string()),
                "the real path: {said}"
            );
            for part in [
                "Work has stopped because ConsensFlow can't save to its ledger at",
                "full, failing or disconnected",
                "lost write permission",
                "Quit ConsensFlow",
                "reopen ConsensFlow",
                "Check the board before retrying an unconfirmed change",
                "Choose Resume for each project",
            ] {
                assert!(said.contains(part), "{part}: {said}");
            }
            assert_eq!(
                *rig.exited.borrow(),
                [crate::broken::EXIT],
                "the daemon ends with the status that says why, once"
            );
            assert_eq!(heard[0]["pid"], std::process::id(), "and says who says it");
            let log = log_lines(&rig.home)
                .iter()
                .map(|line| said_of(line).to_owned())
                .collect::<Vec<_>>();
            assert!(
                log.iter()
                    .any(|line| line.contains("stop: the ledger cannot save")),
                "{log:?}"
            );
        })
        .await;
}

/// Makes the ledger break, as an operation that failed and a rollback SQLite refused do.
fn break_the_ledger(rig: &Rig) {
    let mut ledger = rig.daemon.parts.ledger.borrow_mut();
    refuse_next_rollback();
    let _ = ledger.create_task(
        99,
        &NewTask {
            from: "chief".into(),
            pool: Some("worker".into()),
            tier: Some("standard".into()),
            body: "nowhere".into(),
            ..NewTask::default()
        },
    );
    assert!(ledger.broken().is_some());
}

#[tokio::test]
async fn no_page_request_is_served_once_the_ledger_cannot_save() {
    LocalSet::new()
        .run_until(async {
            let root = Rc::new(tempfile::tempdir().unwrap());
            let rig = started(&root).await;
            let settings = crate::app_settings::settings_path(&environment(root.path())).unwrap();
            // A page request that fails in the ledger, whose rollback is refused, and the
            // one behind it, read in the same breath: it would write a file the person's
            // next start reads.
            refuse_next_rollback();
            let (broke, behind) = tokio::join!(
                ask(
                    &rig.app,
                    "project.rename",
                    json!({ "project": 99, "name": "x" })
                ),
                ask(&rig.app, "specs.set", json!({ "on": true })),
            );
            assert_eq!(broke["ok"], false, "{broke}");
            assert!(rig.daemon.parts.ledger.borrow().broken().is_some());
            assert_eq!(behind["ok"], false, "{behind}");
            assert_eq!(
                behind["error"], "the daemon is stopping",
                "refused by the fence, before any handler"
            );
            assert!(
                !settings.exists(),
                "agents.json was written after the break"
            );
            tokio::time::timeout(Duration::from_secs(10), rig.daemon.finished())
                .await
                .expect("the daemon stops");
        })
        .await;
}

#[tokio::test]
async fn a_log_another_process_holds_does_not_keep_the_daemon_from_stopping() {
    LocalSet::new()
        .run_until(async {
            let root = Rc::new(tempfile::tempdir().unwrap());
            let rig = started(&root).await;
            let heard: Rc<RefCell<Vec<Value>>> = Rc::default();
            let told = Rc::clone(&heard);
            rig.app.on_event(crate::broken::EVENT, move |body| {
                told.borrow_mut().push(body.clone())
            });
            // Another writer has the log's lock, and keeps it.
            let held = std::fs::OpenOptions::new()
                .create(true)
                .truncate(false)
                .write(true)
                .open(cf_base::file::lock_beside(&rig.home.join("daemon.log")))
                .unwrap();
            held.lock().unwrap();
            // It is let go after a while, so that a stop that waits for it ends late, not never.
            let letting_go = std::thread::spawn(move || {
                std::thread::sleep(Duration::from_secs(5));
                drop(held);
            });
            let began = Instant::now();
            break_the_ledger(&rig);
            tokio::time::timeout(Duration::from_secs(20), rig.daemon.finished())
                .await
                .expect("the daemon stops with its log held");
            assert!(
                began.elapsed() < Duration::from_secs(3),
                "{:?}",
                began.elapsed()
            );
            assert_eq!(*rig.exited.borrow(), [crate::broken::EXIT]);
            for _ in 0..50 {
                if !heard.borrow().is_empty() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            assert_eq!(heard.borrow().len(), 1, "the app was told");
            letting_go.join().unwrap();
        })
        .await;
}

#[tokio::test]
async fn an_ordinary_stop_ends_with_its_log_held_too() {
    LocalSet::new()
        .run_until(async {
            let root = Rc::new(tempfile::tempdir().unwrap());
            let rig = started(&root).await;
            let held = std::fs::OpenOptions::new()
                .create(true)
                .truncate(false)
                .write(true)
                .open(cf_base::file::lock_beside(&rig.home.join("daemon.log")))
                .unwrap();
            held.lock().unwrap();
            let letting_go = std::thread::spawn(move || {
                std::thread::sleep(Duration::from_secs(5));
                drop(held);
            });
            let began = Instant::now();
            rig.daemon.stop("SIGTERM");
            tokio::time::timeout(Duration::from_secs(20), rig.daemon.finished())
                .await
                .expect("the stop ends with its log held");
            assert!(
                began.elapsed() < Duration::from_secs(3),
                "{:?}",
                began.elapsed()
            );
            assert_eq!(*rig.exited.borrow(), [0]);
            letting_go.join().unwrap();
        })
        .await;
}

#[tokio::test]
async fn a_home_that_was_used_and_has_no_ledger_is_refused_and_never_started_as_an_empty_board() {
    LocalSet::new()
        .run_until(async {
            let root = tempfile::tempdir().unwrap();
            let home = a_marked_home(root.path());
            let (said, exited, handles) = (Said::default(), Rc::default(), Rc::default());
            let (options, _app) = options(root.path(), &said, &exited, &handles);
            let refused = start(environment(root.path()), options).await.err();
            let Some(StartError::CannotSave(words)) = refused else {
                panic!("it was started: {refused:?}");
            };
            assert_eq!(
                words,
                crate::broken::lost_words(&home.join("consensflow.db"))
            );
            assert_eq!(
                StartError::CannotSave(words).exit_code(),
                crate::broken::EXIT
            );
            assert!(
                !home.join("consensflow.db").exists(),
                "no ledger was made: the board that was is not replaced by an empty one"
            );
        })
        .await;
}

/// A home with nothing of ours in it, or only what is no witness (the app's own files, the
/// page's, a first start's staging file), is a first use: it starts, and is marked.
#[tokio::test]
async fn a_home_with_no_witness_is_a_first_use() {
    LocalSet::new()
        .run_until(async {
            type Left = fn(&Path);
            let table: Vec<(&str, Left)> = vec![
                ("nothing of ours", |home| {
                    std::fs::write(home.join("notes.txt"), "the person's own").unwrap();
                }),
                ("the app's own files", |home| {
                    std::fs::create_dir_all(home.join("app")).unwrap();
                    std::fs::write(home.join("app/app.log"), "x\n").unwrap();
                    std::fs::write(home.join("app/updates.json"), "{}").unwrap();
                }),
                ("the page's agents file", |home| {
                    std::fs::write(home.join("agents.json"), "[]").unwrap();
                }),
                ("a staging file of a cut first start", |home| {
                    std::fs::write(home.join("consensflow.db.new-0123456789abcdef"), "x").unwrap();
                }),
            ];
            for (row, leave) in table {
                let root = tempfile::tempdir().unwrap();
                let home = root.path().join("consensflow");
                std::fs::create_dir_all(&home).unwrap();
                leave(&home);
                started_over(root.path())
                    .await
                    .unwrap_or_else(|failed| panic!("{row}: it was refused: {failed}"));
                assert!(home.join(crate::broken::MARKER).exists(), "{row}: marked");
            }
        })
        .await;
}

/// An existing empty file in a used home is refused, not initialized: the file stays empty
/// and the start is the ledger's lost words; the same through the handle the open holds
/// where the file was swapped for an empty one after the look.
#[tokio::test]
async fn an_empty_file_in_a_used_home_is_refused_and_stays_empty() {
    LocalSet::new()
        .run_until(async {
            let root = tempfile::tempdir().unwrap();
            let home = a_marked_home(root.path());
            let ledger = home.join("consensflow.db");
            std::fs::write(&ledger, b"").unwrap();
            let refused = started_over(root.path()).await.err();
            assert!(
                matches!(refused, Some(StartError::CannotSave(_))),
                "{refused:?}"
            );
            assert_eq!(std::fs::metadata(&ledger).unwrap().len(), 0, "initialized");
            // Swapped after the look: opened with no leave to make it, the handle is empty.
            let failed = cf_ledger::open_existing_ledger(&ledger, LedgerOptions::default())
                .err()
                .expect("an empty file is no ledger");
            assert!(matches!(
                at_start_of_used(&ledger, failed),
                StartError::CannotSave(words) if words == crate::broken::lost_words(&ledger)
            ));
            assert_eq!(std::fs::metadata(&ledger).unwrap().len(), 0);
        })
        .await;
}

/// A non-empty file that is no ledger, in a used home, ends the start with 74 and the
/// new-board words, never with 1: the app never retries it.
#[tokio::test]
async fn an_unreadable_ledger_in_a_used_home_exits_74_with_the_new_board_words() {
    LocalSet::new()
        .run_until(async {
            let root = tempfile::tempdir().unwrap();
            let home = a_marked_home(root.path());
            let ledger = home.join("consensflow.db");
            std::fs::write(
                &ledger,
                b"this is not a database, but it is not empty either, no",
            )
            .unwrap();
            let refused = started_over(root.path()).await.expect_err("refused");
            assert_eq!(refused.exit_code(), crate::broken::EXIT);
            let StartError::CannotSave(words) = refused else {
                panic!("{refused:?}");
            };
            assert_eq!(words, crate::broken::lost_words(&ledger));
            assert!(words.contains("move"), "{words}");
        })
        .await;
}

/// A marker that cannot be written stops the start with 74 and words that name the file, before
/// the first write to a ledger that was there: its bytes are the same after, and the words say
/// so. The log says the status the process ends with.
#[tokio::test]
async fn a_marker_that_cannot_be_written_stops_the_start_and_names_the_file() {
    LocalSet::new()
        .run_until(async {
            let root = tempfile::tempdir().unwrap();
            let home = root.path().join("consensflow");
            std::fs::create_dir_all(&home).unwrap();
            // An earlier build's ledger: its upgrade commits before the marker is written.
            let file = home.join("consensflow.db");
            let older = cf_ledger::open_ledger(&file, LedgerOptions::default()).unwrap();
            cf_ledger::testing::undo_migration_0044(older.sqlite()).unwrap();
            older
                .sqlite()
                .execute_batch("PRAGMA user_version = 43")
                .unwrap();
            drop(older);
            let bytes = std::fs::read(&file).unwrap();
            // Where the marker's temporary file would go.
            std::fs::create_dir(home.join(format!(
                "{}.{}.tmp",
                crate::broken::MARKER,
                std::process::id()
            )))
            .unwrap();
            let refused = started_over(root.path()).await.expect_err("refused");
            assert_eq!(refused.exit_code(), crate::broken::EXIT);
            let StartError::CannotSave(words) = refused else {
                panic!("{refused:?}");
            };
            assert!(words.contains(crate::broken::MARKER), "{words}");
            assert!(!home.join(crate::broken::MARKER).exists());
            assert_eq!(std::fs::read(&file).unwrap(), bytes, "the ledger was not written");
            let now = rusqlite::Connection::open(&file).unwrap();
            let version: i64 = now
                .query_row("PRAGMA user_version", [], |row| row.get(0))
                .unwrap();
            assert_eq!(version, 43, "the upgrade did not run");
            assert!(
                words.contains("No saved work was changed."),
                "the words say what is true: {words}"
            );
            let log = log_lines(&home);
            let said: Vec<&str> = log.iter().map(|line| said_of(line)).collect();
            assert_eq!(
                said.last(),
                Some(&"info exit 74"),
                "the status the process ends with: {said:?}"
            );
        })
        .await;
}

#[test]
fn a_ledger_that_cannot_be_saved_to_at_the_start_is_told_as_one_and_a_held_one_is_not() {
    let file = std::path::Path::new("/h/consensflow.db");
    let broken = cf_ledger::LedgerError::Unrolled {
        first: Box::new(cf_ledger::LedgerError::refused("no", "no")),
        rollback: rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_IOERR),
            None,
        ),
    };
    assert!(matches!(at_start(file, broken), StartError::CannotSave(_)));
    let full = cf_ledger::LedgerError::Sqlite(rusqlite::Error::SqliteFailure(
        rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_FULL),
        None,
    ));
    assert!(matches!(at_start(file, full), StartError::CannotSave(_)));
    let held = cf_ledger::LedgerError::Sqlite(rusqlite::Error::SqliteFailure(
        rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY),
        None,
    ));
    assert!(matches!(at_start(file, held), StartError::Ledger(_)));
}

/// A home with a marker and no trace at all: what a home holds that this build has opened.
pub(super) fn a_marked_home(root: &Path) -> PathBuf {
    let home = root.join("consensflow");
    std::fs::create_dir_all(&home).unwrap();
    crate::broken::mark(&home).unwrap();
    home
}

/// Starts a daemon over `root` and says what came of it: the refusal, or the daemon (stopped).
pub(super) async fn started_over(root: &Path) -> Result<(), StartError> {
    let (said, exited, handles) = (Said::default(), Rc::default(), Rc::default());
    let (options, _app) = options(root, &said, &exited, &handles);
    let daemon = start(environment(root), options).await?;
    daemon.stop("SIGTERM");
    daemon.finished().await;
    Ok(())
}

/// A home that has had a board never opens as an empty one: whatever became of its ledger
/// (gone, empty, a link to nothing, a folder in its place), the start is refused, ends with
/// the status the app does not retry, and makes no ledger. The words say how to start anew.
#[tokio::test]
async fn a_used_home_whose_ledger_is_gone_empty_dangling_or_not_a_file_is_never_opened_empty() {
    LocalSet::new()
        .run_until(async {
            type Spoil = fn(&Path);
            let table: Vec<(&str, Spoil)> = vec![
                ("missing", |_| {}),
                ("zero bytes", |ledger| std::fs::write(ledger, b"").unwrap()),
                ("a folder", |ledger| std::fs::create_dir(ledger).unwrap()),
                #[cfg(unix)]
                ("a dangling link", |ledger| {
                    let nowhere = ledger
                        .parent()
                        .unwrap()
                        .join("moved-away")
                        .join("consensflow.db");
                    std::os::unix::fs::symlink(nowhere, ledger).unwrap();
                }),
            ];
            for (row, spoil) in table {
                let root = tempfile::tempdir().unwrap();
                let home = a_marked_home(root.path());
                let ledger = home.join("consensflow.db");
                spoil(&ledger);
                let before = std::fs::read_dir(&home).unwrap().count();
                let refused = started_over(root.path()).await.err();
                let Some(StartError::CannotSave(words)) = refused else {
                    panic!("{row}: it was started: {refused:?}");
                };
                assert_eq!(words, crate::broken::lost_words(&ledger), "{row}");
                assert!(
                    words.contains("move") && words.contains("a new folder"),
                    "{row}: {words}"
                );
                assert!(
                    words.contains(&ledger.display().to_string()),
                    "{row}: the true path: {words}"
                );
                assert_eq!(
                    StartError::CannotSave(words).exit_code(),
                    crate::broken::EXIT,
                    "{row}"
                );
                // Nothing was made: not at the link's target, not beside it.
                assert!(
                    !home.join("moved-away").exists(),
                    "{row}: a ledger was made at the target"
                );
                assert_eq!(
                    std::fs::read_dir(&home).unwrap().count(),
                    before,
                    "{row}: something was made"
                );
            }
        })
        .await;
}

/// The marker is written when the ledger is first made, and a ledger an earlier build made,
/// which has none, is marked by the first start that opens it: from then on the home that
/// loses it is refused.
#[tokio::test]
async fn the_marker_is_written_at_first_use_and_at_the_first_open_of_an_older_home() {
    LocalSet::new()
        .run_until(async {
            let root = tempfile::tempdir().unwrap();
            let home = root.path().join("consensflow");
            assert!(!home.join(crate::broken::MARKER).exists(), "nothing yet");
            started_over(root.path()).await.expect("a first use starts");
            assert!(
                home.join(crate::broken::MARKER).exists(),
                "marked at first use"
            );
            // An older home: a ledger and none of what this build writes beside it.
            for entry in std::fs::read_dir(&home).unwrap().flatten() {
                if entry.file_name() != "consensflow.db" {
                    let path = entry.path();
                    if path.is_dir() {
                        std::fs::remove_dir_all(path).unwrap();
                    } else {
                        std::fs::remove_file(path).unwrap();
                    }
                }
            }
            assert!(!home.join(crate::broken::MARKER).exists());
            started_over(root.path()).await.expect("it opens");
            assert!(
                home.join(crate::broken::MARKER).exists(),
                "marked by its first open"
            );
            std::fs::rename(
                home.join("consensflow.db"),
                root.path().join("moved-aside.db"),
            )
            .unwrap();
            let refused = started_over(root.path()).await.err();
            assert!(
                matches!(refused, Some(StartError::CannotSave(_))),
                "{refused:?}"
            );
            assert!(!home.join("consensflow.db").exists());
        })
        .await;
}

/// A ledger that was there at the look and is not at the open is refused and never made again,
/// empty, beside the board it was.
#[tokio::test]
async fn a_ledger_that_goes_between_the_look_and_the_open_is_refused_not_made_again() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("consensflow.db");
    let failed = cf_ledger::open_existing_ledger(&file, LedgerOptions::default())
        .err()
        .expect("no ledger is made");
    assert!(failed.cannot_save(), "{failed}");
    assert!(!file.exists(), "SQLite was given no leave to make it");
    assert!(matches!(
        at_start_of_used(&file, failed),
        StartError::CannotSave(words) if words == crate::broken::lost_words(&file)
    ));
}

/// An ordinary stop under way when work breaks the ledger still ends with the status that
/// says so: the reason that came first does not decide the status.
#[tokio::test]
async fn a_stop_already_under_way_ends_with_the_ledgers_status_when_the_ledger_breaks() {
    LocalSet::new()
        .run_until(async {
            let root = Rc::new(tempfile::tempdir().unwrap());
            let rig = started(&root).await;
            rig.daemon.stop("SIGTERM");
            break_the_ledger(&rig);
            tokio::time::timeout(Duration::from_secs(10), rig.daemon.finished())
                .await
                .expect("the daemon stops");
            assert_eq!(*rig.exited.borrow(), [crate::broken::EXIT]);
        })
        .await;
}

/// An act under way that outlasts the bound is not waited for for good: the app is told at the
/// bound, the daemon ends with 74 within its stop's time, and the log's line of the cause says
/// an effect was still under way.
#[tokio::test]
async fn an_act_held_past_the_bound_does_not_hold_the_break() {
    LocalSet::new()
        .run_until(async {
            let root = Rc::new(tempfile::tempdir().unwrap());
            let rig = started(&root).await;
            let halt = rig.daemon.latch.halt();
            let (entered, inside) = std::sync::mpsc::channel();
            let (let_go, held) = std::sync::mpsc::channel::<()>();
            let acting = std::thread::spawn(move || {
                halt.act(move || {
                    let _ = entered.send(());
                    // Let go by the test; never for longer than this, so a broken one ends.
                    let _ = held.recv_timeout(Duration::from_secs(6));
                })
            });
            inside.recv_timeout(Duration::from_secs(5)).unwrap();
            let heard: Rc<RefCell<Vec<std::time::Instant>>> = Rc::default();
            let told = Rc::clone(&heard);
            rig.app.on_event(crate::broken::EVENT, move |_| {
                told.borrow_mut().push(std::time::Instant::now());
            });

            let began = std::time::Instant::now();
            break_the_ledger(&rig);
            let reacted = began.elapsed();
            assert!(
                reacted >= crate::files::SETTLE,
                "the break waits for the act under way: {reacted:?}"
            );
            assert!(
                reacted < Duration::from_secs(2),
                "and no longer than the bound: {reacted:?}"
            );
            tokio::time::timeout(Duration::from_secs(4), rig.daemon.finished())
                .await
                .expect("the daemon stops while the act is still under way");
            assert_eq!(*rig.exited.borrow(), [crate::broken::EXIT]);
            assert_eq!(heard.borrow().len(), 1, "the app was told");
            let log = log_lines(&rig.home);
            assert!(
                log.iter()
                    .any(|line| line.contains("the ledger cannot save any more")
                        && line.contains("an effect was still under way")),
                "{log:?}"
            );
            let _ = let_go.send(());
            acting
                .join()
                .unwrap()
                .expect("the act that had begun was made");
        })
        .await;
}
