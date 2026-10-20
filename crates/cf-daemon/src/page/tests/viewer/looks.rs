//! The looks at a specs folder, over the bridge: each off the daemon's
//! executor, so many at once and one more refused until one ends; whether the
//! folder holds a file looked at once however many ask meanwhile, remembered
//! for a moment, and an answer from memory saying when the folder is read again.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Condvar, Mutex};

use super::super::super::viewer::MAX_JOBS;
use super::*;

/// What is called as each look at a folder begins.
type Hook = Arc<dyn Fn() + Send + Sync>;
/// Whether the looks held may go on, and what tells them it may.
type Gate = Arc<(Mutex<bool>, Condvar)>;

/// A hook for each look at a folder that counts the looks begun and holds each
/// until the gate opens (never longer than a few seconds: a look run on the
/// executor would hold the test there), and the gate.
fn held_looks() -> (Hook, Arc<AtomicUsize>, Gate) {
    let begun = Arc::new(AtomicUsize::new(0));
    let gate: Gate = Arc::new((Mutex::new(false), Condvar::new()));
    let (counted, waited) = (Arc::clone(&begun), Arc::clone(&gate));
    let hook: Hook = Arc::new(move || {
        counted.fetch_add(1, Ordering::SeqCst);
        let (open, opened) = &*waited;
        let _ = opened
            .wait_timeout_while(open.lock().unwrap(), Duration::from_secs(5), |open| !*open)
            .unwrap();
    });
    (hook, begun, gate)
}

/// Opens the gate of [`held_looks`]: every look held goes on, and every one after.
fn open_gate(gate: &(Mutex<bool>, Condvar)) {
    *gate.0.lock().unwrap() = true;
    gate.1.notify_all();
}

/// Until `count` looks have begun.
async fn begun(looks: &AtomicUsize, count: usize) {
    while looks.load(Ordering::SeqCst) < count {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

/// The board asks at each of its reads: what it was told is remembered for a
/// moment, and read again once the moment is over. An answer from memory says
/// when that is (`readAgainIn`, in milliseconds), so the page asks once more
/// then and its button cannot keep what the folder no longer says.
#[tokio::test]
async fn whether_the_folder_holds_a_file_is_remembered_for_a_moment_which_is_said() {
    LocalSet::new()
        .run_until(async {
            let held = asked_with(Value::Null, &[(".specs/a/SPEC.md", "# A")], |specs| {
                specs.remembering_for(Duration::from_secs(60))
            });
            assert_eq!(held.present().await, json!({ "ok": true, "present": true }));
            fs::remove_dir_all(held.specs()).unwrap();
            let remembered = held.present().await;
            assert_eq!(remembered["present"], true, "remembered");
            let again = remembered["readAgainIn"].as_u64().unwrap();
            assert!((59_000..=60_000).contains(&again), "{remembered}");
            let forgets = asked_with(Value::Null, &[(".specs/a/SPEC.md", "# A")], |specs| {
                specs.remembering_for(Duration::ZERO)
            });
            assert_eq!(forgets.present().await["present"], true);
            fs::remove_dir_all(forgets.specs()).unwrap();
            assert_eq!(
                forgets.present().await,
                json!({ "ok": true, "present": false }),
                "read again"
            );
        })
        .await;
}

/// The board of every window asks at each of its reads: however many ask
/// while the folder is looked at, it is looked at once, and each is answered
/// what that look found.
#[tokio::test]
async fn whether_the_folder_holds_a_file_is_looked_at_once_however_many_ask_meanwhile() {
    LocalSet::new()
        .run_until(async {
            let (hook, looks, gate) = held_looks();
            let held = asked_with(Value::Null, &[(".specs/a/SPEC.md", "# A")], |specs| {
                specs.remembering_for(Duration::ZERO).before_each_look(hook)
            });
            let asking = |app: &Bridge| {
                app.request(
                    "specs.present",
                    json!({ "project": held.project }),
                    Some(Duration::from_secs(10)),
                )
            };
            let (first, second, third) = (asking(&held.app), asking(&held.app), asking(&held.app));
            begun(&looks, 1).await;
            tokio::time::sleep(Duration::from_millis(50)).await;
            open_gate(&gate);
            for answer in [first.await, second.await, third.await] {
                assert_eq!(answer.unwrap(), json!({ "ok": true, "present": true }));
            }
            assert_eq!(looks.load(Ordering::SeqCst), 1, "one look for the three");
            // Once it has answered, the next asks again.
            assert_eq!(held.present().await["present"], true);
            assert_eq!(looks.load(Ordering::SeqCst), 2);
        })
        .await;
}

/// A look a dead drive holds is never given back: so many may run at once, and
/// one more is refused at once, in words, until one of them ends.
#[tokio::test]
async fn so_many_looks_at_the_folder_run_at_once_and_one_more_is_refused_until_one_ends() {
    LocalSet::new()
        .run_until(async {
            let (hook, looks, gate) = held_looks();
            let held = asked_with(Value::Null, &[(".specs/a/SPEC.md", "# A")], |specs| {
                specs.before_each_look(hook)
            });
            let running: Vec<_> = (0..MAX_JOBS)
                .map(|_| {
                    held.app.request(
                        "specs.file",
                        json!({ "project": held.project, "path": "a/SPEC.md" }),
                        Some(Duration::from_secs(10)),
                    )
                })
                .collect();
            begun(&looks, MAX_JOBS).await;
            let refused = json!({
                "ok": false,
                "error": "the specs folders are slow to answer: 4 reads of them are still \
                          running. Try again once they end",
            });
            assert_eq!(held.tree().await, refused);
            assert_eq!(held.open("a/SPEC.html").await, refused);
            open_gate(&gate);
            for answer in running {
                assert_eq!(answer.await.unwrap()["file"]["text"], "# A");
            }
            assert_eq!(held.tree().await["specs"]["found"], true, "room again");
        })
        .await;
}

/// A look at the folder runs off the daemon's executor: one held on a slow
/// drive (here, held by the test until the daemon has answered something
/// else) holds up only the operation that asked.
#[tokio::test]
async fn a_slow_look_at_the_folder_holds_up_nothing_else_of_the_daemon() {
    LocalSet::new()
        .run_until(async {
            let (begun, began) = mpsc::channel::<()>();
            let (release, released) = mpsc::channel::<()>();
            let (begun, released) = (Mutex::new(begun), Mutex::new(released));
            let hold: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
                let _ = begun.lock().unwrap().send(());
                // Never for ever: a look run on the executor would hold the test here.
                let _ = released
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(2));
            });
            let held = asked_with(Value::Null, &[(".specs/a/SPEC.md", "# A")], |specs| {
                specs.before_each_look(hold)
            });
            let order: Rc<RefCell<Vec<&str>>> = Rc::default();
            let tree = held.app.request(
                "specs.tree",
                json!({ "project": held.project }),
                Some(Duration::from_secs(10)),
            );
            let noted = Rc::clone(&order);
            let looked = tokio::task::spawn_local(async move {
                let answer = tree.await.unwrap();
                noted.borrow_mut().push("specs.tree");
                answer
            });
            while began.try_recv().is_err() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            assert_eq!(
                ask(&held.app, "ping", json!({})).await,
                json!({ "ok": true })
            );
            order.borrow_mut().push("ping");
            release.send(()).unwrap();
            let answer = looked.await.unwrap();
            assert_eq!(answer["specs"]["found"], true, "{answer}");
            assert_eq!(*order.borrow(), ["ping", "specs.tree"]);
        })
        .await;
}
