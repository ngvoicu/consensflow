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
                "full disk",
                "failing disk",
                "a disk that went away",
                "lost permission",
                "start ConsensFlow again",
                "wasn't confirmed may still show after the restart",
                "check the board",
            ] {
                assert!(said.contains(part), "{part}: {said}");
            }
            assert_eq!(
                *rig.exited.borrow(),
                [0],
                "the daemon ends as at a quit, once"
            );
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
