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

/// What a home that was used holds: a trace with a line of the board in it.
fn a_used_home(root: &Path) -> PathBuf {
    let home = root.join("consensflow");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(
        home.join("events.jsonl"),
        concat!(
            "{\"at\":\"2026-11-22T10:00:00.000Z\",\"kind\":\"daemon.start\"}\n",
            "{\"at\":\"2026-11-22T10:00:01.000Z\",\"project\":1,\"kind\":\"project.created\"}\n"
        ),
    )
    .unwrap();
    home
}

#[tokio::test]
async fn a_home_that_was_used_and_has_no_ledger_is_refused_and_never_started_as_an_empty_board() {
    LocalSet::new()
        .run_until(async {
            let root = tempfile::tempdir().unwrap();
            let home = a_used_home(root.path());
            let (said, exited, handles) = (Said::default(), Rc::default(), Rc::default());
            let (options, _app) = options(root.path(), &said, &exited, &handles);
            let refused = start(environment(root.path()), options).await.err();
            let Some(StartError::CannotSave(words)) = refused else {
                panic!("it was started: {refused:?}");
            };
            assert_eq!(words, crate::broken::words(&home.join("consensflow.db")));
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

#[tokio::test]
async fn a_home_that_only_its_own_start_wrote_in_is_a_first_use() {
    LocalSet::new()
        .run_until(async {
            let root = tempfile::tempdir().unwrap();
            let home = root.path().join("consensflow");
            std::fs::create_dir_all(&home).unwrap();
            std::fs::write(
                home.join("events.jsonl"),
                "{\"at\":\"2026-11-22T10:00:00.000Z\",\"kind\":\"daemon.start\"}\n",
            )
            .unwrap();
            std::fs::write(home.join("daemon.log"), "an earlier start that ended\n").unwrap();
            assert!(!crate::broken::home_was_used(&home));
            let (said, exited, handles) = (Said::default(), Rc::default(), Rc::default());
            let (options, _app) = options(root.path(), &said, &exited, &handles);
            let daemon = start(environment(root.path()), options)
                .await
                .expect("a first use starts");
            daemon.stop("SIGTERM");
            daemon.finished().await;
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
