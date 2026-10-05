//! A panic caught where work runs: what is made of it, where it is written,
//! and that the hook notes where it was without writing anything itself.

use std::cell::Cell;
use std::sync::Mutex;

use super::*;

/// The hook is the process's: the tests that put one in take turns.
static HOOK: Mutex<()> = Mutex::new(());

fn homed() -> (tempfile::TempDir, Rc<Errors>) {
    let home = tempfile::tempdir().unwrap();
    let errors = Rc::new(Errors::new(
        Rc::new(Log::new(home.path())),
        Rc::new(Trace::new(home.path())),
    ));
    (home, errors)
}

#[tokio::test]
async fn work_that_ends_is_its_answer() {
    assert_eq!(contain(async { 7 }).await, Ok(7));
    assert_eq!(contain_now(|| "now"), Ok("now"));
}

#[tokio::test]
async fn a_panic_in_the_first_poll_is_caught_and_says_its_words() {
    let caught = contain(async {
        if true {
            panic!("a bug at the start");
        }
    })
    .await
    .unwrap_err();
    assert_eq!(caught.message, "a bug at the start");
    let formatted = contain(async {
        let bad = "formatted".len();
        assert_eq!(bad, 0, "words {bad}");
    })
    .await
    .unwrap_err();
    assert!(
        formatted.message.contains("words 9"),
        "{}",
        formatted.message
    );
}

#[tokio::test]
async fn a_panic_after_a_wait_is_caught_too_and_what_was_held_is_let_go() {
    struct Held<'a>(&'a Cell<bool>);
    impl Drop for Held<'_> {
        fn drop(&mut self) {
            self.0.set(true);
        }
    }
    let released = Cell::new(false);
    let caught = contain(async {
        let _held = Held(&released);
        tokio::task::yield_now().await;
        panic!("a bug after a wait");
    })
    .await;
    assert_eq!(caught.unwrap_err().message, "a bug after a wait");
    assert!(released.get(), "unwinding let it go");
}

#[test]
fn a_panic_that_carries_no_words_is_said_so() {
    let caught = contain_now(|| std::panic::panic_any(5_u8)).unwrap_err();
    assert_eq!(caught.message, "a panic with no words");
}

#[test]
fn the_hook_notes_where_a_panic_was_and_writes_nothing() {
    let _turn = HOOK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let before = panic::take_hook();
    install_hook();
    let caught = contain_now(|| panic!("over there")).unwrap_err();
    panic::set_hook(before);
    let location = caught.location.clone().expect("the hook noted it");
    // Rust names the file with the platform's separator.
    assert!(
        location.replace('\\', "/").contains("errors/tests.rs"),
        "{location}"
    );
    assert_eq!(
        caught.cause(),
        format!("panic: over there\n    at {location}")
    );
    // Taken by the one who caught it: the next panic's is its own.
    let before = panic::take_hook();
    install_hook();
    let again = contain_now(|| panic!("again")).unwrap_err();
    panic::set_hook(before);
    assert_ne!(again.location, None);
}

#[test]
fn without_the_hook_a_panic_has_its_words_and_no_place() {
    let _turn = HOOK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    WHERE.with(|noted| *noted.borrow_mut() = None);
    let caught = contain_now(|| panic!("nowhere")).unwrap_err();
    assert_eq!(caught.cause(), "panic: nowhere");
}

#[test]
fn a_caught_panic_goes_to_the_log_with_its_place_and_to_the_trace_as_a_daemon_error() {
    let (home, errors) = homed();
    errors.caught(
        "a pass failed",
        &Panicked {
            message: "index out of bounds".to_owned(),
            location: Some("crates/cf-engine/src/scheduler.rs:9:5".to_owned()),
        },
    );
    let log = std::fs::read_to_string(home.path().join("daemon.log")).unwrap();
    let mut lines = log.lines();
    assert!(lines.next().unwrap().ends_with(" error a pass failed"));
    assert_eq!(lines.next(), Some("    panic: index out of bounds"));
    assert_eq!(
        lines.next(),
        Some("        at crates/cf-engine/src/scheduler.rs:9:5")
    );
    let trace = std::fs::read_to_string(home.path().join("events.jsonl")).unwrap();
    let line: serde_json::Value = serde_json::from_str(trace.trim()).unwrap();
    assert_eq!(line["kind"], "daemon.error");
    assert_eq!(line["project"], serde_json::Value::Null);
    assert_eq!(line["reason"], "a pass failed: index out of bounds");
}

#[tokio::test]
async fn work_that_goes_on_apart_is_caught_and_written_down_and_the_rest_goes_on() {
    let (home, errors) = homed();
    tokio::task::LocalSet::new()
        .run_until(async {
            let ran = Rc::new(Cell::new(0));
            errors.spawn("a task failed", async { panic!("a bug apart") });
            let counted = Rc::clone(&ran);
            errors.spawn(
                "a task failed",
                async move { counted.set(counted.get() + 1) },
            );
            tokio::task::yield_now().await;
            tokio::task::yield_now().await;
            assert_eq!(ran.get(), 1, "the work after it ran");
        })
        .await;
    let log = std::fs::read_to_string(home.path().join("daemon.log")).unwrap();
    assert!(log.contains("error a task failed"), "{log}");
    assert!(log.contains("panic: a bug apart"), "{log}");
    let trace = std::fs::read_to_string(home.path().join("events.jsonl")).unwrap();
    assert!(
        trace.contains(r#""reason":"a task failed: a bug apart""#),
        "{trace}"
    );
}
