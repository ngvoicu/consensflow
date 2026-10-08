//! The daemon's seams of the engine: what a window starts with, the ids and
//! files of a launch, an adapter for each harness, and where the engine's work
//! runs: on the executor, drained and driven, a panic in it written down.

use std::cell::{Cell, RefCell};
use std::future::poll_fn;
use std::task::{Context, Poll, Waker};

use cf_engine::runtime::next_turn;
use cf_harness::contract::Work;
use cf_harness::testing::Fakes;
use cf_proto::agents::Harness;
use tokio::sync::Notify;
use tokio::task::LocalSet;

use super::*;
use crate::files::{Log, Trace};
use crate::testing::{scene, worked, Worked};

fn participant<'a>(project: &'a ProjectView, handle: &str) -> &'a ParticipantView {
    project
        .participants
        .iter()
        .find(|participant| participant.handle == handle)
        .unwrap()
}

#[test]
fn a_window_starts_with_its_url_project_participant_and_the_bundle_first_on_its_path() {
    let scene = scene();
    // The app named its Node to the daemon before the deletion release, and
    // the daemon passed it on to every window; nothing is bundled to name now,
    // and a variable left in the environment reaches no window.
    let env = Env::from_vars([
        ("CONSENSFLOW_NODE", "/opt/consensflow/node"),
        ("PATH", "/usr/bin:/bin"),
        ("HOME", "/home/me"),
    ]);
    let window = WindowEnv::new(&env, "http://127.0.0.1:4242", "/opt/consensflow/bin");
    let started = window.env(participant(&scene.project, "zeus"), &scene.project);
    let delimiter = if cfg!(windows) { ';' } else { ':' };
    assert_eq!(
        started,
        [
            (
                "CONSENSFLOW_URL".to_owned(),
                "http://127.0.0.1:4242".to_owned()
            ),
            (
                "CONSENSFLOW_PROJECT".to_owned(),
                scene.project.id.to_string()
            ),
            ("CONSENSFLOW_PARTICIPANT".to_owned(), "zeus".to_owned()),
            (
                "PATH".to_owned(),
                format!("/opt/consensflow/bin{delimiter}/usr/bin:/bin")
            ),
        ]
    );
}

#[test]
fn a_daemon_with_no_path_gives_its_windows_the_bundle_alone() {
    let scene = scene();
    for env in [Env::default(), Env::from_vars([("PATH", "")])] {
        let window = WindowEnv::new(&env, "http://127.0.0.1:1", "/bundle/bin");
        let started = window.env(participant(&scene.project, "chief"), &scene.project);
        assert_eq!(
            started.last(),
            Some(&("PATH".to_owned(), "/bundle/bin".to_owned()))
        );
    }
}

#[test]
fn the_chief_is_given_its_staff_and_every_role_the_cf_of_its_window() {
    let scene = scene();
    let texts = RoleTexts::new(Env::default(), "/bundle/bin/cf".to_owned());
    let chief = texts
        .instructions(participant(&scene.project, "chief"), &scene.project)
        .unwrap();
    assert!(
        chief.contains("| zeus | worker | Standard work |"),
        "{chief}"
    );
    assert!(chief.contains("Here `cf` is /bundle/bin/cf."), "{chief}");
    let worker = texts
        .instructions(participant(&scene.project, "zeus"), &scene.project)
        .unwrap();
    assert!(worker.contains("Here `cf` is /bundle/bin/cf."), "{worker}");
}

#[test]
fn a_role_that_has_no_text_fails_its_launch_in_the_words_of_why() {
    let scene = scene();
    let texts = RoleTexts::new(Env::default(), "/bundle/bin/cf".to_owned());
    let mut human = participant(&scene.project, "zeus").clone();
    human.role = "human".to_owned();
    let failed = texts.instructions(&human, &scene.project).unwrap_err();
    assert_eq!(failed, "no role instructions for human");
}

#[test]
fn every_launch_has_an_id_of_its_own_in_the_shape_of_a_uuid() {
    let ids = RandomLaunchIds;
    let drawn: Vec<LaunchId> = (0..20).map(|_| ids.draw()).collect();
    for id in &drawn {
        assert_eq!(id.as_str().len(), 36);
        assert_eq!(LaunchId::new(id.as_str()).as_ref(), Some(id));
    }
    for (at, id) in drawn.iter().enumerate() {
        assert!(!drawn[..at].contains(id), "{}", id.as_str());
    }
}

#[test]
fn a_launch_s_files_are_forgotten_from_every_harness_s_folder() {
    let home = tempfile::tempdir().unwrap();
    let launch = RandomLaunchIds.draw();
    for harness in ["claude", "pi", "devin", "opencode"] {
        let folder = home
            .path()
            .join("integrations")
            .join(harness)
            .join(launch.as_str());
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("settings.json"), "{}").unwrap();
    }
    let kept = home
        .path()
        .join("integrations")
        .join("claude")
        .join("not-a-launch");
    std::fs::create_dir_all(&kept).unwrap();
    let errors = Rc::new(Errors::new(
        Rc::new(Log::new(home.path())),
        Rc::new(Trace::new(home.path())),
    ));
    LaunchFolders::new(home.path().to_path_buf(), errors).forget(&launch);
    for harness in ["claude", "pi", "devin", "opencode"] {
        let folder = home
            .path()
            .join("integrations")
            .join(harness)
            .join(launch.as_str());
        assert!(!folder.exists(), "{harness}");
    }
    assert!(kept.exists(), "what is no launch's stays");
}

#[test]
fn every_harness_the_ledger_names_has_its_adapter_and_no_other_word_has() {
    let home = tempfile::tempdir().unwrap();
    let services = Fakes::new(&Env::default()).services(&Env::default(), home.path());
    let adapters = HarnessAdapters::new(&services);
    for harness in Harness::ALL {
        assert!(
            adapters.adapter(harness.kind()).is_some(),
            "{}",
            harness.kind()
        );
    }
    // The CLI's own name is not the ledger's word for Claude.
    assert!(adapters.adapter("claude").is_none());
    for word in ["", "image", "kimi", "Codex", "mystery"] {
        assert!(adapters.adapter(word).is_none(), "{word:?}");
    }
}

#[tokio::test]
async fn work_spawned_waits_for_a_drain_and_a_drain_runs_it_in_the_order_it_was_woken() {
    LocalSet::new()
        .run_until(async {
            let worked = worked();
            let order = Rc::new(RefCell::new(Vec::new()));
            let chain = Rc::clone(&order);
            worked.spawn.spawn(Box::pin(async move {
                for turn in 0..4 {
                    chain.borrow_mut().push(turn);
                    next_turn().await;
                }
            }));
            let other = Rc::clone(&order);
            worked
                .spawn
                .spawn(Box::pin(async move { other.borrow_mut().push(10) }));
            assert!(order.borrow().is_empty(), "nothing runs until a drain");
            // The local set gets no turn: the whole chain is one drain's.
            worked.spawn.drain();
            assert_eq!(*order.borrow(), [0, 10, 1, 2, 3]);
        })
        .await;
}

#[tokio::test]
async fn a_panic_in_work_apart_is_written_down_and_the_work_after_it_runs() {
    let home = {
        let local = LocalSet::new();
        local
            .run_until(async {
                let worked = worked();
                let ran = Rc::new(Cell::new(0));
                let counted = Rc::clone(&ran);
                worked
                    .spawn
                    .spawn(Box::pin(async { panic!("a launch went wrong") }));
                worked.spawn.apart("a window's exit failed", async {
                    panic!("an exit went wrong");
                });
                worked
                    .spawn
                    .spawn(Box::pin(async move { counted.set(counted.get() + 1) }));
                worked.spawn.drain();
                assert_eq!(ran.get(), 1, "the work after it ran");
                worked.home
            })
            .await
    };
    let log = std::fs::read_to_string(home.path().join("daemon.log")).unwrap();
    assert!(log.contains("error a task failed"), "{log}");
    assert!(log.contains("panic: a launch went wrong"), "{log}");
    assert!(log.contains("error a window's exit failed"), "{log}");
    assert!(log.contains("panic: an exit went wrong"), "{log}");
    let trace = std::fs::read_to_string(home.path().join("events.jsonl")).unwrap();
    assert!(
        trace.contains(r#""reason":"a task failed: a launch went wrong""#),
        "{trace}"
    );
}

/// A wait the test ends by hand: it keeps the waker it was polled with, and is
/// ready once it has been ended.
#[derive(Default)]
struct Hand {
    ended: Cell<bool>,
    waker: RefCell<Option<Waker>>,
}

impl Hand {
    fn wait(self: &Rc<Self>) -> Work<'static, ()> {
        let this = Rc::clone(self);
        Box::pin(poll_fn(move |context| {
            if this.ended.get() {
                return Poll::Ready(());
            }
            *this.waker.borrow_mut() = Some(context.waker().clone());
            Poll::Pending
        }))
    }

    /// Wakes whoever waits, with nothing to say yet.
    fn wake(&self) {
        let waker = self.waker.borrow_mut().take();
        if let Some(waker) = waker {
            waker.wake();
        }
    }

    /// The wait is over, and whoever waits is woken.
    fn end(&self) {
        self.ended.set(true);
        self.wake();
    }
}

#[tokio::test]
async fn a_wait_its_relay_woke_with_nothing_to_say_has_its_answer_from_its_relay_again() {
    LocalSet::new()
        .run_until(async {
            let Worked { spawn, .. } = worked();
            let hand = Rc::new(Hand::default());
            let mut waiting = spawn.arrival(hand.wait());
            // The test is the work that awaits it, and polls it by hand.
            let poll = |waiting: &mut Work<'static, ()>| {
                waiting
                    .as_mut()
                    .poll(&mut Context::from_waker(Waker::noop()))
            };
            assert!(poll(&mut waiting).is_pending());
            // It wakes with nothing to say: its relay tells the work, which
            // polls it and finds it waits still.
            hand.wake();
            for _ in 0..5 {
                tokio::task::yield_now().await;
            }
            assert!(poll(&mut waiting).is_pending());
            // Now it ends. A poll that is not its relay's, made before the
            // relay ran, does not take the answer: the relay's wake does.
            hand.end();
            assert!(poll(&mut waiting).is_pending(), "not before its relay");
            for _ in 0..5 {
                tokio::task::yield_now().await;
            }
            assert!(poll(&mut waiting).is_ready());
        })
        .await;
}

#[tokio::test]
async fn work_woken_from_outside_a_drain_is_run_by_the_driver() {
    LocalSet::new()
        .run_until(async {
            let worked = worked();
            let (gate, ran) = (Rc::new(Notify::new()), Rc::new(Cell::new(false)));
            let (held, marked) = (Rc::clone(&gate), Rc::clone(&ran));
            worked.spawn.spawn(Box::pin(async move {
                held.notified().await;
                marked.set(true);
            }));
            worked.spawn.drain();
            assert!(!ran.get(), "it waits for the gate");
            // What a timer or a socket does: wakes it, and nobody drains.
            gate.notify_one();
            for _ in 0..5 {
                tokio::task::yield_now().await;
            }
            assert!(ran.get(), "the driver ran it");
        })
        .await;
}
