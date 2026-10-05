//! The thread's own cases, through readers that do what a test says (see
//! `support`). That the real readers read through the thread as they read
//! beside it is `fixtures`'.

use std::cell::RefCell;
use std::fs;
use std::rc::Rc;

use tokio::task::spawn_local;

use super::support::{asked, eventually, on_engine, polled, said, scripted, scripted_with, Script};
use super::*;
use crate::records::PiSettlement;
use crate::testing::ManualTime;

/// Looks at `session` of `harness` at `at`, one after another, and what each said.
async fn looked(thread: &Thread, at: &[(i64, Harness, &str)], time: &ManualTime) -> Vec<String> {
    let options = Options::default();
    let mut told = Vec::new();
    for (now, harness, session) in at {
        time.settle_at(*now);
        let reading = thread.look(*harness, session, &options).await;
        told.push(said(&reading).to_owned());
    }
    told
}

#[test]
fn a_look_carries_its_options_and_the_time_of_the_engines_clock() {
    let script = Script::ungated();
    let time = Rc::new(ManualTime::new(1_000));
    let thread = scripted(&script, &time);
    let launch = |id: &str| Options {
        pi_settlement: Some(PiSettlement {
            directory: None,
            launch_id: Some(id.to_owned()),
        }),
    };
    let told = on_engine(async {
        let first = thread.look(Harness::Pi, "a", &launch("one")).await;
        time.settle_at(5_000);
        let second = thread.look(Harness::Pi, "a", &Options::default()).await;
        let other = thread.look(Harness::Codex, "a", &Options::default()).await;
        [first, second, other].map(|reading| said(&reading).to_owned())
    });
    assert_eq!(
        told,
        [
            "1.1 pi a at 1000 for one",
            "1.2 pi a at 5000",
            "2.1 codex a at 5000"
        ]
    );
}

#[test]
fn a_look_carries_the_time_it_was_asked_at_and_not_the_time_the_worker_reaches_it() {
    let (script, gate) = Script::gated();
    let time = Rc::new(ManualTime::new(100));
    let thread = scripted(&script, &time);
    let options = Options::default();
    let told = on_engine(async {
        let first = asked(thread.look(Harness::Codex, "a", &options));
        script.wait_for("look 1.1 ");
        time.settle_at(200);
        let second = asked(thread.look(Harness::Codex, "b", &options));
        time.settle_at(900);
        gate.release(2);
        [first.await, second.await].map(|reading| said(&reading).to_owned())
    });
    assert_eq!(told, ["1.1 codex a at 100", "2.1 codex b at 200"]);
}

#[test]
fn looks_of_one_conversation_are_read_in_the_order_they_were_asked() {
    // The first look is held, so the rest wait in the queue together.
    let (script, gate) = Script::gated();
    let time = Rc::new(ManualTime::new(0));
    let thread = scripted(&script, &time);
    let options = Options::default();
    let told = on_engine(async {
        let mut asks = Vec::new();
        for at in 0..40 {
            time.settle_at(at);
            asks.push(asked(thread.look(Harness::Codex, "a", &options)));
        }
        gate.release(40);
        let mut told = Vec::new();
        for look in asks {
            let reading = look.await;
            told.push(said(&reading).to_owned());
        }
        told
    });
    let wanted: Vec<String> = (1..=40)
        .map(|look| format!("1.{look} codex a at {}", look - 1))
        .collect();
    assert_eq!(told, wanted);
}

#[test]
fn a_look_dropped_before_its_answer_is_read_all_the_same_and_the_next_goes_on_from_it() {
    let (script, gate) = Script::gated();
    let time = Rc::new(ManualTime::new(0));
    let thread = scripted(&script, &time);
    let options = Options::default();
    let next = on_engine(async {
        // Dropped while the worker is reading it.
        drop(asked(thread.look(Harness::Codex, "a", &options)));
        script.wait_for("look 1.1 ");
        // Dropped while it waits its turn behind that one.
        drop(asked(thread.look(Harness::Codex, "a", &options)));
        let next = asked(thread.look(Harness::Codex, "a", &options));
        gate.release(3);
        next.await
    });
    assert_eq!(said(&next), "1.3 codex a at 0");
    assert_eq!(
        script.log(),
        [
            "open 1 codex a",
            "look 1.1 codex a at 0",
            "look 1.2 codex a at 0",
            "look 1.3 codex a at 0"
        ]
    );
}

#[test]
fn the_queue_is_bounded_and_a_look_that_waits_for_room_is_not_asked() {
    let (script, gate) = Script::gated();
    let time = Rc::new(ManualTime::new(0));
    let thread = scripted_with(&script, &time, 2);
    let options = Options::default();
    let next = on_engine(async {
        // The worker holds the first look, and two more fill the queue.
        let held = asked(thread.look(Harness::Codex, "a", &options));
        script.wait_for("look 1.1 ");
        let queued = [
            asked(thread.look(Harness::Codex, "a", &options)),
            asked(thread.look(Harness::Codex, "a", &options)),
        ];
        // Two more find no room, and are given up while they wait for it.
        drop(asked(thread.look(Harness::Codex, "a", &options)));
        drop(asked(thread.look(Harness::Codex, "a", &options)));
        gate.release(10);
        held.await;
        for look in queued {
            look.await;
        }
        thread.look(Harness::Codex, "a", &options).await
    });
    assert_eq!(
        said(&next),
        "1.4 codex a at 0",
        "the two given up were never asked"
    );
}

#[test]
fn the_engines_thread_runs_other_work_while_a_slow_look_is_read() {
    let (script, gate) = Script::gated();
    let time = Rc::new(ManualTime::new(0));
    let thread = Rc::new(scripted(&script, &time));
    let order = Rc::new(RefCell::new(Vec::new()));
    let reading = on_engine(async {
        let looking = spawn_local({
            let (thread, order) = (Rc::clone(&thread), Rc::clone(&order));
            async move {
                let options = Options::default();
                let reading = thread.look(Harness::Codex, "a", &options).await;
                order.borrow_mut().push("the look is answered");
                reading
            }
        });
        let meanwhile = spawn_local({
            let order = Rc::clone(&order);
            async move {
                tokio::task::yield_now().await;
                order.borrow_mut().push("other work");
                // The look reads only once this lets it.
                gate.release(1);
            }
        });
        meanwhile.await.unwrap();
        looking.await.unwrap()
    });
    assert_eq!(*order.borrow(), ["other work", "the look is answered"]);
    assert_eq!(said(&reading), "1.1 codex a at 0");
}

#[test]
fn a_conversation_unread_for_the_idle_time_is_read_anew() {
    let script = Script::ungated();
    let time = Rc::new(ManualTime::new(0));
    let thread = scripted(&script, &time);
    let at = [
        (0, Harness::Codex, "a"),
        (IDLE_MS - 1, Harness::Codex, "a"),
        // A sweep runs here, and a was read a millisecond ago.
        (IDLE_MS, Harness::Codex, "b"),
        (IDLE_MS + 1, Harness::Codex, "a"),
        // b was unread for the idle time, a was not.
        (2 * IDLE_MS, Harness::Codex, "b"),
        (2 * IDLE_MS, Harness::Codex, "a"),
    ];
    let told = on_engine(looked(&thread, &at, &time));
    assert_eq!(
        told,
        [
            "1.1 codex a at 0",
            "1.2 codex a at 599999",
            "2.1 codex b at 600000",
            "1.3 codex a at 600001",
            "3.1 codex b at 1200000",
            "1.4 codex a at 1200000"
        ]
    );
}

#[test]
fn the_caches_begin_when_the_thread_is_made_as_the_engines_clock_reads_then() {
    let script = Script::ungated();
    let made = 1_000_000;
    let time = Rc::new(ManualTime::new(made));
    let thread = scripted(&script, &time);
    let at = [
        (made + 1, Harness::Codex, "z"),
        // The first sweep is due, and z was read an idle time less a
        // millisecond ago.
        (made + IDLE_MS, Harness::Codex, "y"),
        // The next is not due for another idle time.
        (made + IDLE_MS + 1, Harness::Codex, "z"),
    ];
    let told = on_engine(looked(&thread, &at, &time));
    assert_eq!(
        told,
        [
            "1.1 codex z at 1000001",
            "2.1 codex y at 1600000",
            "1.2 codex z at 1600001"
        ]
    );
}

#[test]
fn each_harness_forgets_on_a_clock_of_its_own() {
    let script = Script::ungated();
    let time = Rc::new(ManualTime::new(0));
    let thread = scripted(&script, &time);
    let at = [
        (1, Harness::Pi, "x"),
        // Codex's cache sweeps here, and Pi's does not.
        (IDLE_MS, Harness::Codex, "z"),
        // Pi's own first sweep is due, and x was unread for the idle time.
        (IDLE_MS + 1, Harness::Pi, "x"),
    ];
    let told = on_engine(looked(&thread, &at, &time));
    assert_eq!(
        told,
        [
            "1.1 pi x at 1",
            "2.1 codex z at 600000",
            "3.1 pi x at 600001"
        ]
    );
}

#[test]
fn a_look_sweeps_the_cache_it_goes_through_and_no_other() {
    let script = Script::ungated();
    let time = Rc::new(ManualTime::new(0));
    let thread = scripted(&script, &time);
    let at = [(0, Harness::Pi, "x"), (IDLE_MS, Harness::Codex, "z")];
    on_engine(looked(&thread, &at, &time));
    assert!(
        !script.log().contains(&"drop 1".to_owned()),
        "x has been unread for the idle time, but Pi's cache was not looked through: {:?}",
        script.log()
    );
    on_engine(looked(&thread, &[(IDLE_MS + 1, Harness::Pi, "x")], &time));
    assert!(script.log().contains(&"drop 1".to_owned()));
}

#[test]
fn a_reader_that_panics_fails_its_look_and_the_next_look_of_another_works() {
    let script = Script::ungated();
    let time = Rc::new(ManualTime::new(0));
    let thread = scripted(&script, &time);
    let at = [
        (0, Harness::Claude, "boom"),
        (0, Harness::Claude, "boom"),
        (0, Harness::Claude, "fine"),
        (0, Harness::Claude, "boom"),
    ];
    let (told, transcript) = on_engine(async {
        let told = looked(&thread, &at, &time).await;
        (told, thread.has_transcript(Harness::Claude, "boom").await)
    });
    assert_eq!(
        told,
        [
            "1.1 claude boom at 0",
            "unreadable: the reader panicked: a bug in reader 1",
            "2.1 claude fine at 0",
            // What the reader had read went with it: the record is read from the start.
            "3.1 claude boom at 0"
        ]
    );
    assert_eq!(
        script.log(),
        [
            "open 1 claude boom",
            "look 1.1 claude boom at 0",
            "look 1.2 claude boom at 0",
            "drop 1",
            "open 2 claude fine",
            "look 2.1 claude fine at 0",
            "open 3 claude boom",
            "look 3.1 claude boom at 0"
        ]
    );
    assert_eq!(transcript, Err("missing home in env".to_owned()));
}

#[test]
fn a_panic_is_said_in_its_own_words_or_in_none() {
    assert_eq!(panics_to_text(|| 3), Ok(3));
    assert_eq!(
        panics_to_text(|| -> () { panic!("a literal") }),
        Err("a literal".to_owned())
    );
    assert_eq!(
        panics_to_text(|| -> () { panic!("{}", "a made one") }),
        Err("a made one".to_owned())
    );
    assert_eq!(
        panics_to_text(|| -> () { std::panic::panic_any(7) }),
        Err("no message".to_owned())
    );
}

#[test]
fn a_transcript_is_had_as_the_switch_says_and_a_failure_travels_too() {
    let home = tempfile::tempdir().unwrap();
    let env = Env::from_vars([("HOME", home.path().to_str().unwrap())]);
    let keep = |under: &[&str], name: &str| {
        let folder = under
            .iter()
            .fold(home.path().to_path_buf(), |path, part| path.join(part));
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join(name), "").unwrap();
    };
    keep(&[".claude", "projects", "-work"], "s.jsonl");
    keep(
        &[".codex", "sessions", "2026", "10", "05"],
        "rollout-2026-10-05T01-00-00-s.jsonl",
    );
    keep(
        &[".pi", "agent", "sessions", "--work--"],
        "2026-10-05T01-00-00-000Z_s.jsonl",
    );
    keep(&[".local", "share", "opencode"], "opencode.db");
    let time: Rc<dyn Time> = Rc::new(ManualTime::new(0));
    let thread = Thread::new(env.clone(), TimeZone::UTC, Rc::clone(&time)).unwrap();
    let bare = Thread::new(Env::default(), TimeZone::UTC, time).unwrap();
    on_engine(async {
        for harness in Harness::ALL {
            for session in ["s", "q7"] {
                assert_eq!(
                    thread.has_transcript(harness, session).await,
                    has_transcript(harness, session, &env),
                    "{harness:?} {session}"
                );
            }
        }
        assert_eq!(thread.has_transcript(Harness::Claude, "s").await, Ok(true));
        assert_eq!(
            bare.has_transcript(Harness::Claude, "s").await,
            Err("missing home in env".to_owned())
        );
    });
}

#[test]
fn a_transcript_is_asked_of_the_worker_and_waits_behind_the_looks_before_it() {
    let (script, gate) = Script::gated();
    let time = Rc::new(ManualTime::new(0));
    let thread = scripted(&script, &time);
    let options = Options::default();
    on_engine(async {
        let held = asked(thread.look(Harness::Codex, "a", &options));
        script.wait_for("look 1.1 ");
        let mut asking = thread.has_transcript(Harness::Claude, "a");
        assert!(
            polled(&mut asking).is_pending(),
            "the worker is still reading the look before it"
        );
        gate.release(1);
        held.await;
        assert_eq!(asking.await, Err("missing home in env".to_owned()));
    });
}

#[test]
fn a_look_the_worker_never_answers_is_unreadable_and_the_engine_goes_on() {
    let time = || -> Rc<dyn Time> { Rc::new(ManualTime::new(0)) };
    // A worker that ended: nobody reads the queue.
    let (asks, waiting) = mpsc::channel(1);
    drop(waiting);
    let ended = Thread { time: time(), asks };
    // Workers that take an ask and let go of it unanswered.
    let (asks, mut waiting) = mpsc::channel(1);
    let letting_go = std::thread::spawn(move || waiting.blocking_recv().map(drop));
    let dropped = Thread { time: time(), asks };
    let (asks, mut waiting) = mpsc::channel(1);
    let letting_go_too = std::thread::spawn(move || waiting.blocking_recv().map(drop));
    let dropped_too = Thread { time: time(), asks };
    on_engine(async {
        for thread in [&ended, &dropped] {
            let reading = thread.look(Harness::Codex, "a", &Options::default()).await;
            assert_eq!(said(&reading), "unreadable: the records thread ended");
        }
        assert_eq!(
            ended.has_transcript(Harness::Codex, "a").await,
            Err("the records thread ended".to_owned())
        );
        assert_eq!(
            dropped_too.has_transcript(Harness::Codex, "a").await,
            Err("the records thread ended".to_owned())
        );
    });
    letting_go.join().unwrap();
    letting_go_too.join().unwrap();
}

#[test]
fn dropping_the_thread_ends_the_worker_once_it_has_read_what_was_asked() {
    let (script, gate) = Script::gated();
    let time = Rc::new(ManualTime::new(0));
    let thread = scripted(&script, &time);
    let options = Options::default();
    // One look being read and one waiting, both given up.
    drop(asked(thread.look(Harness::Codex, "a", &options)));
    script.wait_for("look 1.1 ");
    drop(asked(thread.look(Harness::Codex, "a", &options)));
    drop(thread);
    assert!(
        Arc::strong_count(&script) > 1,
        "the worker is still reading: it holds its readers"
    );
    gate.release(2);
    eventually("the worker to end", || Arc::strong_count(&script) == 1);
    assert_eq!(
        script.log(),
        [
            "open 1 codex a",
            "look 1.1 codex a at 0",
            "look 1.2 codex a at 0",
            "drop 1"
        ]
    );
}
